//! `atoms::health` 的行为测试。
//!
//! 除了验证数值结算，这些测试还**固化 Message / Event 的用法约定**
//! （见 `docs/bevy-events.md`）：
//!
//! - `ModifyHealthMessage` 是 `Message`：用 `MessageWriter` 发、在调度点被 `MessageReader` 拉取。
//! - `DeathEvent` 是 `EntityEvent`：用 observer 监听、可拿到 `#[event_target]` 实体。
//!
//! 这套测试也是"L1 公式 → `ModifyHealthMessage` → L0 执行"流水线（R50）的最小验证样板。

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use voxelith_axiom::atoms::{Health, HealthPlugin, ModifyHealthMessage};

/// 跑一帧：`app.update()` 会推进调度（执行 `apply_health_change`）并应用命令缓冲，
/// 因此 `commands.trigger(DeathEvent { .. })` 触发的 observer 也在这一帧内执行完。
fn tick(app: &mut App) {
    app.update();
}

#[test]
fn damage_and_heal_share_the_same_message() {
    let mut app = App::new();
    app.add_plugins(HealthPlugin);
    let entity = app.world_mut().spawn(Health::new(100)).id();

    app.world_mut()
        .resource_mut::<Messages<ModifyHealthMessage>>()
        .write(ModifyHealthMessage {
            entity,
            amount: -30,
        });
    tick(&mut app);
    assert_eq!(app.world().get::<Health>(entity).unwrap().current, 70);

    // 治疗走同一条消息，只是正数（R51）
    app.world_mut()
        .resource_mut::<Messages<ModifyHealthMessage>>()
        .write(ModifyHealthMessage { entity, amount: 20 });
    tick(&mut app);
    assert_eq!(app.world().get::<Health>(entity).unwrap().current, 90);
}

#[test]
fn health_is_clamped_to_zero_and_max() {
    let mut app = App::new();
    app.add_plugins(HealthPlugin);
    let entity = app.world_mut().spawn(Health::new(50)).id();

    // 过量伤害不会变成负数
    app.world_mut()
        .resource_mut::<Messages<ModifyHealthMessage>>()
        .write(ModifyHealthMessage {
            entity,
            amount: -9999,
        });
    tick(&mut app);
    assert_eq!(app.world().get::<Health>(entity).unwrap().current, 0);

    // 过量治疗不会超过 max
    app.world_mut()
        .resource_mut::<Messages<ModifyHealthMessage>>()
        .write(ModifyHealthMessage {
            entity,
            amount: 9999,
        });
    tick(&mut app);
    assert_eq!(app.world().get::<Health>(entity).unwrap().current, 50);
}

#[derive(Resource, Default)]
struct DeathCount(usize);

#[test]
fn death_event_is_triggered_once_when_health_reaches_zero() {
    let mut app = App::new();
    app.add_plugins(HealthPlugin);
    app.init_resource::<DeathCount>();

    // observer 是 system，状态放在 Resource 里才能被测试读取。
    // （闭包捕获 `mut` 变量会搬进 observer 自己的 system 状态，外部读不到——
    //  这也是 observer 的可测试性弱于 Message 的一个具体表现。）
    app.add_observer(
        |_: On<voxelith_axiom::atoms::health::DeathEvent>, mut count: ResMut<DeathCount>| {
            count.0 += 1;
        },
    );

    let entity = app.world_mut().spawn(Health::new(10)).id();
    app.world_mut()
        .resource_mut::<Messages<ModifyHealthMessage>>()
        .write(ModifyHealthMessage {
            entity,
            amount: -10,
        });
    tick(&mut app);
    assert_eq!(
        app.world().resource::<DeathCount>().0,
        1,
        "归零应触发一次 DeathEvent"
    );

    // 已经是 0 再受伤，不应重复触发（`was_alive` 守卫）
    app.world_mut()
        .resource_mut::<Messages<ModifyHealthMessage>>()
        .write(ModifyHealthMessage {
            entity,
            amount: -10,
        });
    tick(&mut app);
    assert_eq!(
        app.world().resource::<DeathCount>().0,
        1,
        "已死亡的实体不应重复触发 DeathEvent"
    );
}

#[test]
fn message_for_unknown_entity_is_ignored() {
    let mut app = App::new();
    app.add_plugins(HealthPlugin);
    let ghost = app.world_mut().spawn_empty().id();

    app.world_mut()
        .resource_mut::<Messages<ModifyHealthMessage>>()
        .write(ModifyHealthMessage {
            entity: ghost,
            amount: -10,
        });
    // 没有 Health 组件的实体，消息应被安全跳过（get_mut 失败即 continue）
    tick(&mut app);
    assert!(app.world().get::<Health>(ghost).is_none());
}
