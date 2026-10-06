//! **迁移样板**：把真的 `basic_attack` 从旧模型搬到新栈，并钉住数值一致。
//!
//! 旧内容（`assets/data/skills.ron` 第一条）：
//!
//! ```ron
//! (id: "basic_attack", duration: 1.0,
//!  requirements: [Resource(pool:"action", min:1.0), NoActiveAction],
//!  costs: [(pool:"action", amount:1.0)],
//!  targeting: CurrentSelection,
//!  effects: [Contest((
//!      attacker: Sum([CasterStat("strength"), Literal(5.0)]),
//!      defender: TargetStat("armor"),
//!      formula: Difference, threshold: 0.0, crit_margin: 5.0,
//!      outcomes: [ (Success, [ModifyResource(pool:"hp", delta: Neg(SkillPower)), Log("命中！")]),
//!                  (Crit,    [ModifyResource(pool:"hp", delta: Neg(Mul(SkillPower, Literal(1.5)))), Log("重击！")]),
//!                  (Fail,    [Log("被挡下了。")]) ]))])
//! ```
//!
//! 迁移映射（**这张表就是 97 个技能的施工图**）：
//!
//! | 旧 | 新 | 在哪 |
//! |---|---|---|
//! | `duration: 1.0`（前摇） | gearbox 的 `#Invoking` → `#Fire` 时间轴 | `invoked` 壳 |
//! | `requirements` / `costs` | 状态边的守卫 + 进状态时的 instant | 内容（下一步） |
//! | `targeting: CurrentSelection` | `TargetType::InvokerTarget` | `GoOffConfig` |
//! | `Contest{Difference, threshold, crit_margin}` | `contest` 的消息链 | `AttackEffect` |
//! | `CasterStat("strength") + Literal(5.0)` | `attack_attribute` + `power_bonus` | `AttackEffect` |
//! | `TargetStat("armor")` | `defense_attribute` | `AttackEffect` |
//! | `ModifyResource(pool:"hp", delta: Neg(SkillPower))` | `Attributes` 上的 instant（`damage_attribute`） | `apply_attack_outcome` |
//! | `Mul(SkillPower, Literal(1.5))` | `crit_multiplier` | `AttackEffect` |
//! | `Log(text)` | `CombatLogLine` 消息 | `contest` |
//! | `CasterStat` / `TargetStat` 是**闭集枚举** | gauge 表达式（`Strength@invoker`） | `attributes` 模块 |

use bevy::asset::AssetPlugin;
use bevy::prelude::*;
use bevy_diesel::prelude::*;
use bevy_diesel::target::Target;

use voxelith_abilities::attributes;
use voxelith_abilities::contest::{AttackEffect, CombatLogLine};
use voxelith_abilities::grid::GridBackend;
use voxelith_axiom::atoms::grid::CellPos;

/// 属性来自 `.ron`（**走内容管线**），不是硬编码在测试里。
///
/// ⚠️ 这里用 `format!` 把参数写进 RON，而**不是**先读 RON 再 `ModifierSet::add` 覆盖：
/// gauge 的 `ModifierSet::add` 是**叠加修饰符**（`+=`），不是"覆盖基值"——
/// 在 RON 的 `Strength 10` 之上再 `add(10)` 会得到 **20**（第一次跑就是这样，
/// 所有伤害数字都跟着偏）。要覆盖得用 `AttributesMut::set_base`。
fn attributes_ron(strength: f32, armor: f32) -> String {
    format!(
        r#"(
    base: [
        (name: "Strength", literal: {strength}),
        (name: "Armor",    literal: {armor}),
        (name: "Health",   literal: 100.0),
    ],
)"#
    )
}

/// 收集战斗日志（L2 该干的事：订阅消息，不改机制）。
#[derive(Resource, Default)]
struct Log(Vec<String>);

fn collect_log(mut reader: MessageReader<CombatLogLine>, mut log: ResMut<Log>) {
    for line in reader.read() {
        log.0.push(line.0.clone());
    }
}

fn test_app() -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        bevy::scene::ScenePlugin,
        voxelith_abilities::plugin(),
    ));
    app.init_resource::<Log>().add_systems(Update, collect_log);
    app
}

/// 造一场对局：施法者在原点（瞄着目标），目标在一格之外。
///
/// 力量与护甲由参数写进 `.ron`；血量固定 100。
fn spawn_duel(app: &mut App, strength: f32, armor: f32) -> (Entity, Entity) {
    let source = attributes_ron(strength, armor);
    let base = attributes::parse(&source).expect("RON 语法没问题");
    let set = base.build_base().expect("属性集能构造");

    let defender = app
        .world_mut()
        .spawn((
            CellPos::new(1, 0),
            Attributes::new(),
            AttributeInitializer::new(set.clone()),
        ))
        .id();
    let caster = app
        .world_mut()
        .spawn((
            CellPos::ZERO,
            Attributes::new(),
            AttributeInitializer::new(set),
            // diesel 问"我在打谁"的地方。
            InvokerTarget::entity(defender, IVec2::new(1, 0)),
        ))
        .id();
    (caster, defender)
}

/// 把 `basic_attack` 的场景落地并触发一次。
fn fire_basic_attack(app: &mut App, caster: Entity) {
    let scene = invoked::<IVec2, _, _>("basic_attack", 1.0, |root| {
        single_shot::<GridBackend>(
            root,
            bsn! {
                // 命中态的规则参数：这一块就是旧 `Contest { .. }` 的迁移落点。
                AttackEffect::default()
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
        target: Target::default(),
    });
    for _ in 0..6 {
        app.update();
    }
}

fn health_of(app: &App, entity: Entity) -> f32 {
    app.world()
        .entity(entity)
        .get::<Attributes>()
        .expect("有属性")
        .value("Health")
}

fn log_lines(app: &App) -> Vec<String> {
    app.world().resource::<Log>().0.clone()
}

#[test]
fn the_dot_ron_attributes_land_on_both_sides() {
    let mut app = test_app();
    let (caster, defender) = spawn_duel(&mut app, 10.0, 3.0);
    app.update();

    assert_eq!(health_of(&app, defender), 100.0, "血量来自 .ron");
    assert_eq!(
        app.world()
            .entity(caster)
            .get::<Attributes>()
            .unwrap()
            .value("Strength"),
        10.0,
        "力量来自 .ron"
    );
}

#[test]
fn a_crit_lands_the_same_numbers_as_the_old_model() {
    let mut app = test_app();
    let (caster, defender) = spawn_duel(&mut app, 10.0, 3.0);
    app.update();

    fire_basic_attack(&mut app, caster);

    // 旧模型：攻 = 10 + 5 = 15，防 = 3，raw = 12；crit_margin = 5 ⇒ 12 >= 5 ⇒ **Crit**
    // 伤害 = SkillPower(12) × 1.5 = 18
    assert_eq!(
        health_of(&app, defender),
        82.0,
        "暴击 18 点：100 - (10 + 5 - 3) × 1.5"
    );
    let lines = log_lines(&app);
    assert!(
        lines.iter().any(|line| line.contains("重击")),
        "日志说的是暴击，实际：{lines:?}"
    );
}

#[test]
fn a_hit_lands_normally_below_the_crit_line() {
    let mut app = test_app();
    // 攻 = 10 + 5 = 15，防 = 12 ⇒ raw = 3：>= 阈值 0 ⇒ 命中，但 < 暴击线 5 ⇒ 不暴击。
    let (caster, defender) = spawn_duel(&mut app, 10.0, 12.0);
    app.update();

    fire_basic_attack(&mut app, caster);

    assert_eq!(health_of(&app, defender), 97.0, "命中 3 点：100 - 3");
    let lines = log_lines(&app);
    assert!(
        lines.iter().any(|line| line.contains("命中")),
        "实际：{lines:?}"
    );
}

#[test]
fn a_miss_deals_nothing() {
    let mut app = test_app();
    // 攻 = 3 + 5 = 8，防 = 12 ⇒ raw = -4 < 阈值 0 ⇒ 未命中。
    let (caster, defender) = spawn_duel(&mut app, 3.0, 12.0);
    app.update();

    fire_basic_attack(&mut app, caster);

    assert_eq!(health_of(&app, defender), 100.0, "没打中就不掉血");
    let lines = log_lines(&app);
    assert!(
        lines.iter().any(|line| line.contains("挡下")),
        "实际：{lines:?}"
    );
}

#[test]
fn a_hit_with_zero_margin_deals_nothing() {
    let mut app = test_app();
    // 攻 = 3 + 5 = 8，防 = 8 ⇒ raw = 0：**命中**但余量 0 ⇒ 伤害 0。
    // 这条怪癖是旧模型如实存在的，迁移要一并保留（不是"顺手修掉"）。
    let (caster, defender) = spawn_duel(&mut app, 3.0, 8.0);
    app.update();

    fire_basic_attack(&mut app, caster);

    assert_eq!(
        health_of(&app, defender),
        100.0,
        "命中但余量 0 ⇒ 0 伤害（旧模型就是这样）"
    );
}
