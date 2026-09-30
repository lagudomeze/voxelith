//! 属性 / 成长的端到端行为测试。
//!
//! 固化四条设计约束（见 `docs/combat-mechanics.md`）：
//!
//! - **组件自成一体**：`Stat` 收到变更消息后自己刷新最终值视图（没有回写/脏标记消息）。
//! - **一个组件一个写入口**：加点 / 发放 / 洗点 / 修饰符全走消息，测试无法绕过。
//! - **Q25**：`f32` 聚合只在最后取整一次，口径来自 `StatConfig`。
//! - **成长走 L1**：经验 → 升级 → 发点数，点数再由 L0 记账。

use core::time::Duration;

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_time::Time;
use voxelith_axiom::atoms::modifiers::{Modifier, ModifierSource, Rounding};
use voxelith_axiom::atoms::stats::{
    AddStatModifierMessage, AllocateStatRequest, GrantStatPointsMessage, Level, LevelConfig,
    RemoveStatModifiersMessage, RespecStatsMessage, Stat, StatBlock, StatConfig, StatId,
    StatPlugin,
};
use voxelith_axiom::behaviors::progression::{
    GainExperienceMessage, LevelUpMessage, ProgressionPlugin,
};

fn test_app() -> App {
    let mut app = App::new();
    // 临时修饰符计时读 `Time`；测试里手动 `advance_by` 精确推进（不装 TimePlugin，保持确定性）。
    app.init_resource::<Time>();
    app.add_plugins((StatPlugin, ProgressionPlugin));
    app
}

/// 精确推进时间并跑一帧。
fn advance(app: &mut App, delta: Duration) {
    app.world_mut().resource_mut::<Time>().advance_by(delta);
    app.update();
}

fn spawn_actor(app: &mut App, strength: u32, unspent: u32) -> Entity {
    // 显式字段构造：写代码时明确知道要哪个属性，直接写字段即可。
    let base = StatBlock {
        strength,
        ..StatBlock::default()
    };
    let entity = app
        .world_mut()
        .spawn((Stat::from_base(base), Level::new()))
        .id();
    if unspent > 0 {
        app.world_mut()
            .resource_mut::<Messages<GrantStatPointsMessage>>()
            .write(GrantStatPointsMessage {
                entity,
                amount: unspent,
            });
        app.update();
    }
    entity
}

fn write<M: Message + Send + Sync + 'static>(app: &mut App, message: M) {
    app.world_mut().resource_mut::<Messages<M>>().write(message);
}

/// 读最终值视图（`Deref` 暴露的就是它）。
fn final_value(app: &App, entity: Entity, id: StatId) -> u32 {
    app.world()
        .get::<Stat>(entity)
        .expect("实体应带 Stat")
        .get(id)
}

fn spawn_source(app: &mut App) -> ModifierSource {
    ModifierSource::new(app.world_mut().spawn_empty().id())
}

// ---------- 加点 / 洗点 ----------

#[test]
fn allocating_points_updates_the_final_value_in_the_same_frame() {
    let mut app = test_app();
    let entity = spawn_actor(&mut app, 10, 5);
    assert_eq!(final_value(&app, entity, StatId::Strength), 10);

    write(
        &mut app,
        AllocateStatRequest {
            entity,
            stat: StatId::Strength,
            amount: 3,
        },
    );
    app.update();

    let stat = app.world().get::<Stat>(entity).unwrap();
    assert_eq!(stat.get(StatId::Strength), 13, "最终值 = base + allocated");
    assert_eq!(stat.strength, 13, "字段直读同一个视图");
    assert_eq!(stat.allocated().get(StatId::Strength), 3);
    assert_eq!(stat.unspent_points(), 2);
}

#[test]
fn allocation_fails_as_a_whole_when_points_are_missing() {
    let mut app = test_app();
    let entity = spawn_actor(&mut app, 10, 1);

    write(
        &mut app,
        AllocateStatRequest {
            entity,
            stat: StatId::Strength,
            amount: 5,
        },
    );
    app.update();

    let stat = app.world().get::<Stat>(entity).unwrap();
    assert_eq!(stat.unspent_points(), 1, "点数不足不应部分扣费");
    assert_eq!(stat.get(StatId::Strength), 10);
}

#[test]
fn respec_refunds_allocated_points_and_keeps_base() {
    let mut app = test_app();
    // base 12 = 内容层设定的初始值，另有 4 点未分配
    let entity = spawn_actor(&mut app, 12, 4);

    write(
        &mut app,
        AllocateStatRequest {
            entity,
            stat: StatId::Strength,
            amount: 4,
        },
    );
    app.update();
    assert_eq!(final_value(&app, entity, StatId::Strength), 16);

    write(&mut app, RespecStatsMessage { entity });
    app.update();

    let stat = app.world().get::<Stat>(entity).unwrap();
    assert_eq!(stat.allocated().total(), 0);
    assert_eq!(stat.unspent_points(), 4, "退回全部分配点");
    assert_eq!(stat.get(StatId::Strength), 12, "初始值不动");
}

// ---------- 修饰符 ----------

#[test]
fn modifiers_stack_by_fixed_op_order() {
    let mut app = test_app();
    let entity = spawn_actor(&mut app, 10, 0);
    let ring = spawn_source(&mut app);
    let potion = spawn_source(&mut app);

    // flat +10 → 20；percent_add +50% → 30；percent_mul +50% → 45
    for modifier in [
        Modifier::flat(10.0, ring),
        Modifier::percent_add(0.5, potion),
        Modifier::percent_mul(0.5, ring),
    ] {
        write(
            &mut app,
            AddStatModifierMessage {
                entity,
                stat: StatId::Strength,
                modifier,
            },
        );
    }
    app.update();

    assert_eq!(final_value(&app, entity, StatId::Strength), 45);
}

#[test]
fn modifiers_only_touch_their_own_attribute() {
    let mut app = test_app();
    let entity = spawn_actor(&mut app, 10, 0);
    let source = spawn_source(&mut app);

    write(
        &mut app,
        AddStatModifierMessage {
            entity,
            stat: StatId::Magic,
            modifier: Modifier::flat(5.0, source),
        },
    );
    app.update();

    let stat = app.world().get::<Stat>(entity).unwrap();
    assert_eq!(stat.modifiers().slot_len(StatId::Magic), 1);
    assert_eq!(stat.modifiers().slot_len(StatId::Strength), 0);
    assert_eq!(stat.get(StatId::Strength), 10);
    assert_eq!(stat.get(StatId::Magic), 5);
}

#[test]
fn removing_a_source_drops_its_modifiers_on_every_stat() {
    let mut app = test_app();
    let entity = spawn_actor(&mut app, 10, 0);
    let belt = spawn_source(&mut app);

    for stat in [StatId::Strength, StatId::Magic] {
        write(
            &mut app,
            AddStatModifierMessage {
                entity,
                stat,
                modifier: Modifier::flat(4.0, belt),
            },
        );
    }
    app.update();
    assert_eq!(final_value(&app, entity, StatId::Strength), 14);
    assert_eq!(final_value(&app, entity, StatId::Magic), 4);

    write(
        &mut app,
        RemoveStatModifiersMessage {
            entity,
            source: belt,
        },
    );
    app.update();

    let stat = app.world().get::<Stat>(entity).unwrap();
    assert!(stat.modifiers().is_empty());
    assert_eq!(stat.get(StatId::Strength), 10);
    assert_eq!(stat.get(StatId::Magic), 0);
}

#[test]
fn temporary_modifier_expires_and_the_value_returns_to_base() {
    let mut app = test_app();
    let entity = spawn_actor(&mut app, 10, 0);
    let potion = spawn_source(&mut app);

    write(
        &mut app,
        AddStatModifierMessage {
            entity,
            stat: StatId::Strength,
            modifier: Modifier::flat(5.0, potion).lasting(Duration::from_secs(2)),
        },
    );
    app.update();
    assert_eq!(final_value(&app, entity, StatId::Strength), 15);

    advance(&mut app, Duration::from_secs(1));
    assert_eq!(final_value(&app, entity, StatId::Strength), 15, "还没到点");

    advance(&mut app, Duration::from_secs(1));
    assert_eq!(
        final_value(&app, entity, StatId::Strength),
        10,
        "到期后自己回落"
    );
    assert!(
        app.world()
            .get::<Stat>(entity)
            .unwrap()
            .modifiers()
            .is_empty(),
        "到期的修饰符应被移除"
    );
}

// ---------- 取整口径（Q25） ----------

#[test]
fn floor_rounding_is_the_default() {
    let mut app = test_app();
    let entity = spawn_actor(&mut app, 10, 0);
    let source = spawn_source(&mut app);

    write(
        &mut app,
        AddStatModifierMessage {
            entity,
            stat: StatId::Strength,
            modifier: Modifier::percent_add(0.05, source),
        },
    );
    app.update();

    // 10 * 1.05 = 10.5 → 向下取整 10
    assert_eq!(final_value(&app, entity, StatId::Strength), 10);
}

#[test]
fn rounding_can_be_configured_by_content() {
    let mut app = test_app();
    app.insert_resource(StatConfig {
        rounding: Rounding::Nearest,
    });
    let entity = spawn_actor(&mut app, 10, 0);
    let source = spawn_source(&mut app);

    write(
        &mut app,
        AddStatModifierMessage {
            entity,
            stat: StatId::Strength,
            modifier: Modifier::percent_add(0.05, source),
        },
    );
    app.update();

    assert_eq!(
        final_value(&app, entity, StatId::Strength),
        11,
        "10.5 → 四舍五入 11"
    );
}

// ---------- 无消息就不动 ----------

#[test]
fn without_messages_nothing_changes() {
    let mut app = test_app();
    let entity = spawn_actor(&mut app, 10, 0);

    for _ in 0..3 {
        app.update();
    }

    let stat = app.world().get::<Stat>(entity).unwrap();
    assert_eq!(stat.get(StatId::Strength), 10);
    assert_eq!(stat.allocated().total(), 0);
    assert!(stat.modifiers().is_empty());
}

// ---------- 成长（L1 → L0） ----------

#[test]
fn experience_levels_up_and_grants_allocatable_points() {
    let mut app = test_app();
    let entity = spawn_actor(&mut app, 10, 0);

    // 默认曲线：1 级所需经验 = base_exp * 1^power = 100
    write(
        &mut app,
        GainExperienceMessage {
            entity,
            amount: 100,
        },
    );
    app.update();

    let level = app.world().get::<Level>(entity).unwrap();
    assert_eq!(level.current(), 2, "满 100 经验升到 2 级");
    assert_eq!(
        app.world().get::<Stat>(entity).unwrap().unspent_points(),
        LevelConfig::default().points_per_level,
        "升级应发放可分配点数"
    );
}

#[test]
fn level_up_message_is_broadcast() {
    #[derive(Resource, Default)]
    struct Captured(Vec<LevelUpMessage>);

    fn capture(mut reader: MessageReader<LevelUpMessage>, mut captured: ResMut<Captured>) {
        for message in reader.read() {
            captured.0.push(*message);
        }
    }

    let mut app = test_app();
    app.init_resource::<Captured>();
    app.add_systems(Update, capture);
    let entity = spawn_actor(&mut app, 10, 0);

    write(
        &mut app,
        GainExperienceMessage {
            entity,
            amount: 100,
        },
    );
    app.update();
    app.update();

    let captured = app.world().resource::<Captured>();
    assert_eq!(captured.0.len(), 1);
    assert_eq!(captured.0[0].new_level, 2);
    assert_eq!(
        captured.0[0].points_granted,
        LevelConfig::default().points_per_level
    );
}

// ---------- 未知实体 ----------

#[test]
fn messages_for_unknown_entities_are_ignored() {
    let mut app = test_app();
    let ghost = app.world_mut().spawn_empty().id();

    write(
        &mut app,
        AllocateStatRequest {
            entity: ghost,
            stat: StatId::Strength,
            amount: 1,
        },
    );
    app.update();

    assert!(app.world().get::<Stat>(ghost).is_none());
}
