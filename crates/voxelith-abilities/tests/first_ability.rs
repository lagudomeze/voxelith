//! **第一个端到端技能**：用三件套跑出一次真实伤害。
//!
//! 这条测试是新架构的"竖切"：一格里的一次攻击，从状态机进入 → 目标解析 → 效果传播 →
//! 属性扣除，全部走新管线，没有一行是旧战斗域的代码。
//!
//! ```text
//! gauge    目标 Health = 100；施法者 Strength = 5
//! gearbox  技能壳 #Ready → #Invoking → #Fire(TerminalState) → #Cooldown → #Ready
//! diesel   #Fire 带 GoOffConfig → 解析出"施法者瞄的那个人" → GoOff → InstantModifierSet
//! 结果     Health 100 → 40（(Strength@invoker + Damage@ability) × 10 = 60）
//! ```
//!
//! # 表达式里的角色（**实测结论**，不是猜的）
//!
//! `instant_set_system` 把角色表传给 gauge 求值：
//!
//! | 写法 | 指向谁 | 谁能用 |
//! |---|---|---|
//! | `@invoker` / `@attacker` | **施法者**（`InvokedBy` 链的根） | 技能的伤害公式读它最自然 |
//! | `@defender` / `@target` | 被打的那个（同时也是被改属性的那个） | 读对方的抗性 |
//! | `@ability` | **效果实体自己**（挂着 `InstantModifierSet` 的那个状态） | 技能自己的倍率：要种在**效果实体**上 |
//!
//! ⚠️ 实测：`Damage` 只种在**技能根**（`invoked` 壳默认会种 `Damage = 1.0`）时，
//! instant 表达式里的 `Damage@ability` 求值成 **0**（技能根上的那份只对**持久**属性节点
//! 生效——那是 `register_invoker_source` 按实体注册的 `source`，例如冷却边的
//! `"Delay" => "Cooldown@ability"`）。要让它进 instant 公式，就把 `Damage` 种在
//! **效果实体**上（本文件的第二条测试），或者干脆从 `@invoker` 的属性算。

use bevy::asset::AssetPlugin;
use bevy::prelude::*;
use bevy_diesel::prelude::*;
use bevy_diesel::target::Target;

use voxelith_abilities::grid::GridBackend;
use voxelith_axiom::atoms::grid::CellPos;

/// 一套最小可跑的 App：时间 / 任务池 + 资产（BSN 场景走 `ScenePatch` 资产）+ 后端。
fn test_app() -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        bevy::scene::ScenePlugin,
        <GridBackend as SpatialBackend>::plugin(),
    ));
    app
}

/// 一格一个：施法者在原点，目标在一格之外。
fn spawn_duel(app: &mut App, caster_extra: impl Bundle) -> (Entity, Entity) {
    let defender = app
        .world_mut()
        .spawn((
            CellPos::new(1, 0),
            Attributes::new(),
            AttributeInitializer::new(mod_set! { "Health" => 100.0 }),
        ))
        .id();

    let caster = app
        .world_mut()
        .spawn((
            CellPos::ZERO,
            // `InvokerTarget` 就是 diesel 问"我在打谁"的地方。
            InvokerTarget::entity(defender, IVec2::new(1, 0)),
            caster_extra,
        ))
        .id();

    (caster, defender)
}

/// 把技能场景落地、认好施法者、跑一帧让 gauge 属性铺开，然后触发一次。
fn fire_once(
    app: &mut App,
    scene: impl Scene,
    caster: Entity,
    ability_name: &'static str,
) -> Entity {
    let ability = app
        .world_mut()
        .spawn_scene(scene)
        .unwrap_or_else(|error| panic!("技能 `{ability_name}` 场景落地失败：{error:?}"))
        .id();
    // 技能要认施法者：`InvokedBy` 是那条关系，`resolve_invoker` 顺着它往上找。
    app.world_mut()
        .entity_mut(ability)
        .insert(InvokedBy(caster));

    // `AttributeInitializer` 靠 Required Components 自动挂 `Attributes`，是**延迟**生效的。
    app.update();

    app.world_mut().write_message(StartInvoke::<IVec2> {
        entity: ability,
        target: Target::default(),
    });
    for _ in 0..6 {
        app.update();
    }
    ability
}

fn health_of(app: &App, entity: Entity) -> f32 {
    app.world()
        .entity(entity)
        .get::<Attributes>()
        .expect("目标还有属性")
        .value("Health")
}

#[test]
fn the_initial_health_is_laid_down_by_gauge() {
    let mut app = test_app();
    let (_caster, defender) = spawn_duel(&mut app, ());
    app.update();
    assert_eq!(health_of(&app, defender), 100.0, "初始血量铺好了");
}

/// 链路自检：效果写成**字面量**时，整条管线必须跑通。
///
/// 它排除掉"表达式/角色解析"这一层的不确定性，剩下的失败就一定是管线问题。
#[test]
fn a_literal_instant_damage_lands() {
    let mut app = test_app();
    let (caster, defender) = spawn_duel(&mut app, ());

    let scene = invoked::<IVec2, _, _>("literal_attack", 0.6, |root| {
        single_shot::<GridBackend>(
            root,
            bsn! {
                template(|_| Ok(instant! { "Health" -= 10.0 }))
            },
        )
    });
    fire_once(&mut app, scene, caster, "literal_attack");

    assert_eq!(health_of(&app, defender), 90.0, "10 点就是 10 点");
}

/// 表达式的**完整**用法：施法者属性（`@invoker`）+ 技能自己的倍率（`@ability`）。
#[test]
fn a_gauge_expression_reads_both_the_caster_and_the_effect() {
    let mut app = test_app();
    let (caster, defender) = spawn_duel(
        &mut app,
        (
            Attributes::new(),
            AttributeInitializer::new(mod_set! { "Strength" => 5.0 }),
        ),
    );

    let scene = invoked::<IVec2, _, _>("expression_attack", 0.6, |root| {
        single_shot::<GridBackend>(
            root,
            bsn! {
                // 技能自己的倍率要种在**效果实体**上（`@ability` = 效果实体自己）。
                template(|_| Ok(AttributeInitializer::new(mod_set! { "Damage" => 1.0 })))
                template(|_| Ok(instant! { "Health" -= "(Strength@invoker + Damage@ability) * 10.0" }))
            },
        )
    });
    fire_once(&mut app, scene, caster, "expression_attack");

    assert_eq!(
        health_of(&app, defender),
        40.0,
        "(Strength@invoker(5) + Damage@ability(1)) × 10 = 60"
    );
}
