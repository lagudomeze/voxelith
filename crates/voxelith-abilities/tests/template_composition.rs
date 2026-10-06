//! **模板组合**：技能按名字复用部件（diesel 的 `TemplateRegistry`）。
//!
//! 这是三件套里最后一块没碰过的能力，也是"可复用部件"那件事的落地：
//! 一份 `"explosion"` 模板被登记一次，任何技能都能按名字引用它
//! （火球、火墙、地雷共用同一份爆炸）——而**爆炸自己也是一台状态机**，
//! 于是"生成物再生效果"可以无限嵌套，每一层都是独立可测的小件。
//!
//! ```text
//! 技能 #Fire ──SpawnConfig(template_id="explosion")──► spawn_system
//!              ├─ 用 GridBackend 解析落点（本测试：施法者的 InvokerTarget 那一格）
//!              ├─ B::insert_position → 给新实体挂 CellPos
//!              └─ registry.apply("explosion", …) → 铺上模板场景
//! 爆炸模板（InitialState(#Fire) + TerminalState + GoOffConfig）立刻进入 #Fire
//!              └─ go_off_on_entry → GoOff → instant_set_system → 扣 Health
//! ```

use bevy::asset::AssetPlugin;
use bevy::prelude::*;
use bevy_diesel::prelude::*;
use bevy_diesel::target::{Target, TargetGenerator};

use voxelith_abilities::grid::{GridBackend, GridOffset};
use voxelith_axiom::atoms::grid::CellPos;

/// 爆炸的标记：测试靠它把"模板生成出来的实体"找出来。
///
/// BSN 里把它当**裸组件记号**写（`Explosion`），所以要 `Clone + Default`。
#[derive(Component, Clone, Default)]
struct Explosion;

type GridConfig = SpawnConfig<GridBackend>;

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

/// 登记 `"explosion"` 模板：一台一进入 `#Fire` 就炸的极简状态机。
///
/// **它是按名字复用的**——任何技能的 `SpawnConfig` 写 `"explosion"` 都能用它。
///
/// ⚠️ 两个必须记住的细节：
///
/// 1. BSN 里 `#Fire` 是一个**独立的状态子实体**，`GoOffConfig` / instant 都挂在它身上，
///    而不是模板根上（根上只有 `Explosion` + `StateMachine`）。
/// 2. 状态**必须自己写 `InvokedBy(#根)`**（`single_shot` 里的 `InvokedBy(root)` 就是这事）。
///    缺了它，`resolve_invoker` 走不到施法者 → `GoOffConfig` 解析出空目标 →
///    `instant_set_system` **静默跳过**（只留一条编译期被裁掉的 debug 日志），
///    症状是"模板生成了、什么都不发生"。
fn register_explosion(app: &mut App) {
    app.world_mut()
        .resource_mut::<TemplateRegistry>()
        .register("explosion", || {
            Box::new(bsn! {
                #Boom Explosion
                StateMachine
                InitialState(#Fire)
                Substates [
                    #Fire InvokedBy(#Boom) TerminalState
                        GoOffConfig::<GridBackend>::default()
                        { template(|_| Ok(instant! { "Health" -= 25.0 })) }
                ]
            })
        });
}

#[test]
fn a_skill_spawns_a_named_template_which_then_fires_its_own_effect() {
    let mut app = test_app();
    register_explosion(&mut app);

    // 目标在一格之外，身上 100 血。
    let defender = app
        .world_mut()
        .spawn((
            CellPos::new(1, 0),
            Attributes::new(),
            AttributeInitializer::new(mod_set! { "Health" => 100.0 }),
        ))
        .id();
    // 施法者站在原点，瞄着目标。
    let caster = app
        .world_mut()
        .spawn((
            CellPos::ZERO,
            InvokerTarget::entity(defender, IVec2::new(1, 0)),
        ))
        .id();

    // 技能：命中时不直接扣血，而是**按名字生成一个模板**，落点是"目标那一格"。
    let scene = invoked::<IVec2, _, _>("fireball", 0.6, |root| {
        single_shot::<GridBackend>(
            root,
            bsn! {
                GridConfig::invoker_offset_target(
                    "explosion",
                    GridOffset::new(1, 0),
                    TargetGenerator::at_invoker_target(),
                )
            },
        )
    });
    let ability = app
        .world_mut()
        .spawn_scene(scene)
        .expect("技能场景能落地")
        .id();
    app.world_mut()
        .entity_mut(ability)
        .insert(InvokedBy(caster));
    app.update();

    app.world_mut().write_message(StartInvoke::<IVec2> {
        entity: ability,
        target: Target::entity(defender, IVec2::new(1, 0)),
    });
    for _ in 0..6 {
        app.update();
    }

    // ① 模板被生成出来了，而且**落在目标那一格**（落点走的是 GridBackend 的 insert_position）
    let mut explosions = app
        .world_mut()
        .query_filtered::<&CellPos, With<Explosion>>();
    let placed: Vec<IVec2> = explosions
        .iter(app.world())
        .map(|cell| IVec2::new(cell.x, cell.y))
        .collect();
    assert_eq!(placed, vec![IVec2::new(1, 0)], "爆炸落在目标格里");

    // ①.5 模板铺开了吗、状态机进状态了吗（缺哪一条，链路就断在哪一步）
    let world = app.world_mut();
    let mut effects = world.query_filtered::<Entity, With<InstantModifierSet>>();
    let effect_states: Vec<Entity> = effects.iter(world).collect();
    assert_eq!(effect_states.len(), 1, "爆炸的效果态存在（模板铺开了）");
    let effect_state = effect_states[0];
    assert!(
        world
            .get::<GoOffConfig<GridBackend>>(effect_state)
            .is_some(),
        "效果态带 GoOffConfig"
    );
    assert!(
        world.get::<Active>(effect_state).is_some(),
        "效果态进了状态（`Added<Active>` 是效果触发的扳机）"
    );
    let invoker = world
        .get::<InvokedBy>(effect_state)
        .map(|invoked| invoked.0);
    assert!(
        invoker.is_some(),
        "效果态认了施法者（`InvokedBy(#根)` 那条线）"
    );

    // ② 生成物自己的效果也生效了（模板内部的 GoOffConfig → 扣血）
    let health = app
        .world()
        .entity(defender)
        .get::<Attributes>()
        .expect("目标还有属性")
        .value("Health");
    assert_eq!(health, 75.0, "爆炸模板自己炸掉 25 点");
}

/// 名字写错时 diesel 会**直接 panic**（不是静默跳过）。
///
/// 这是 `spawn_system` 的明文选择：*"Ensure the template is registered before any
/// `SpawnConfig` references it."* 所以**内容校验必须在加载期拦住未知模板名**——
/// 否则一个拼写错误会在运行时把整局打崩。这条测试把这个下限钉住。
#[test]
#[should_panic(expected = "not found in TemplateRegistry")]
fn an_unregistered_template_id_panics_at_runtime() {
    let mut app = test_app();
    // 故意**不**注册 "explosion"。
    let defender = app
        .world_mut()
        .spawn((CellPos::new(1, 0), Attributes::new()))
        .id();
    let caster = app
        .world_mut()
        .spawn((
            CellPos::ZERO,
            InvokerTarget::entity(defender, IVec2::new(1, 0)),
        ))
        .id();

    let scene = invoked::<IVec2, _, _>("typo_skill", 0.6, |root| {
        single_shot::<GridBackend>(root, bsn! { GridConfig::target("does_not_exist") })
    });
    let ability = app
        .world_mut()
        .spawn_scene(scene)
        .expect("技能场景能落地")
        .id();
    app.world_mut()
        .entity_mut(ability)
        .insert(InvokedBy(caster));
    app.update();
    app.world_mut().write_message(StartInvoke::<IVec2> {
        entity: ability,
        target: Target::entity(defender, IVec2::new(1, 0)),
    });
    for _ in 0..6 {
        app.update();
    }
}
