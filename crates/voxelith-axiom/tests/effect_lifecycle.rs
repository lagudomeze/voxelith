//! 「事件 vs 消息」在**表现层**的边界测试：回答"特效这种跨帧动画，用同步执行的 Event 有问题吗"。
//!
//! 结论（均由本文件实测得出，配合 `docs/bevy-events.md` §2）：
//!
//! 1. **没有问题，但前提是分清两件事**：
//!    - Event 负责的是「**通知**：此刻这个实体死了」——一次性的、瞬时的。
//!    - 动画/特效负责的是「**跨帧状态**：闪 3 帧、渐隐 0.5 秒」——它必须住在**组件**里，
//!      由每帧运行的系统推进。Event 不承载生命周期，也不应该承载。
//! 2. observer 的**同步执行只影响"通知处理"这几微秒**，不影响动画本身：observer 里
//!    只做"起一个特效实体 + 写初始数值"，之后每一帧由普通系统推进（见 `effect_outlives_target`）。
//! 3. observer 里 `commands` 产生的效果，在同一帧后续 schedule 之前的同步点就会生效，
//!    因此"通知后同帧就能读到特效组件"（见 `observer_commands_are_visible_same_frame`）。
//! 4. **真正不能依赖的是 observer 之间的顺序**：Bevy 明示同一事件的多个 observer 执行顺序
//!    是任意的（bevy#14890）。所以凡是**顺序有意义**的结算，必须用 Message + 显式排序。

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use voxelith_axiom::atoms::health::DeathEvent;
use voxelith_axiom::atoms::{Health, HealthPlugin, ModifyHealthMessage};

/// 跨帧表现的载体：纯数据，不含任何渲染类型（R10）。
/// 真实项目里这里是"动画已播放帧数 / 剩余淡出时长"，渲染层读它。
#[derive(Component, Debug)]
struct EffectFade {
    frames_left: u32,
}

#[derive(Resource, Default)]
struct ObserverRuns(u32);

fn tick_effects(mut commands: Commands, mut q: Query<(Entity, &mut EffectFade)>) {
    for (entity, mut fade) in &mut q {
        if fade.frames_left <= 1 {
            commands.entity(entity).despawn();
        } else {
            fade.frames_left -= 1;
        }
    }
}

fn count_effects(world: &mut World) -> usize {
    let mut q = world.query::<&EffectFade>();
    q.iter(world).count()
}

/// 通知（Event）与表现（跨帧状态）分工：observer 只负责"起特效 + 给初值"。
#[test]
fn effect_outlives_target() {
    let mut app = App::new();
    app.add_plugins(HealthPlugin);
    app.init_resource::<ObserverRuns>();
    app.add_systems(PostUpdate, tick_effects);

    app.add_observer(
        |death: On<DeathEvent>, mut commands: Commands, mut runs: ResMut<ObserverRuns>| {
            runs.0 += 1;
            // 表现层做两件事：销毁死者、起一个独立特效实体（3 帧后自行消散）。
            commands.entity(death.entity).despawn();
            commands.spawn(EffectFade { frames_left: 3 });
        },
    );

    let victim = app.world_mut().spawn(Health::new(10)).id();
    app.update(); // 稳定帧

    app.world_mut()
        .resource_mut::<Messages<ModifyHealthMessage>>()
        .write(ModifyHealthMessage {
            entity: victim,
            amount: -10,
        });

    app.update(); // 致命一击：Message 被读 → 触发 DeathEvent → observer 起特效
    assert!(
        app.world().get_entity(victim).is_err(),
        "死者应已被 observer 销毁"
    );
    assert_eq!(
        app.world().resource::<ObserverRuns>().0,
        1,
        "observer 应只跑一次"
    );
    // 特效此刻已存在（同帧可见，见下一个测试）
    let after_hit = count_effects(app.world_mut());
    assert_eq!(after_hit, 1, "特效实体应在命中当帧已存在");

    // 关键：特效比触发它的实体活得更久，并且靠**每帧系统**逐帧推进，与 observer 无关
    app.update();
    app.update();
    app.update();
    let left = count_effects(app.world_mut());
    assert_eq!(
        left, 0,
        "3 帧后特效应自行消散（生命周期由系统推进，不由 Event 承载）"
    );
}

/// observer 里 `commands` 的效果会在同一帧的后续 schedule 之前生效。
///
/// 这依赖 Bevy 在 schedule/系统集边界自动插入的同步点（`ApplyDeferred`）。
/// 如果哪天这条断言挂了，说明该帧的同步点位置变了 —— 那是有价值的信号，不要直接删测试。
#[test]
fn observer_commands_are_visible_same_frame() {
    let mut app = App::new();
    app.add_plugins(HealthPlugin);

    app.add_observer(|_: On<DeathEvent>, mut commands: Commands| {
        commands.spawn(EffectFade { frames_left: 1 });
    });

    let victim = app.world_mut().spawn(Health::new(10)).id();
    app.update();
    app.world_mut()
        .resource_mut::<Messages<ModifyHealthMessage>>()
        .write(ModifyHealthMessage {
            entity: victim,
            amount: -10,
        });
    app.update();

    // 命中当帧，PostUpdate 兄弟系统查询就能看到 observer 生成的特效
    let visible = count_effects(app.world_mut());
    assert_eq!(visible, 1, "observer 的 commands 应在同帧后续系统可见");
}

/// 同一个事件挂多个 observer 时，**不要假设谁先跑**（Bevy 官方：顺序任意，bevy#14890）。
///
/// 这个测试只断言"都跑了"，不断言顺序 —— 正因为无法保证顺序，
/// 凡是有顺序语义的结算（叠加、抵消、优先级）都应该走 `Message` + 显式排序。
#[test]
fn multiple_observers_all_run_but_order_is_unspecified() {
    #[derive(Resource, Default)]
    struct Log(Vec<&'static str>);

    let mut app = App::new();
    app.add_plugins(HealthPlugin);
    app.init_resource::<Log>();

    app.add_observer(|_: On<DeathEvent>, mut log: ResMut<Log>| log.0.push("a"));
    app.add_observer(|_: On<DeathEvent>, mut log: ResMut<Log>| log.0.push("b"));

    let victim = app.world_mut().spawn(Health::new(10)).id();
    app.update();
    app.world_mut()
        .resource_mut::<Messages<ModifyHealthMessage>>()
        .write(ModifyHealthMessage {
            entity: victim,
            amount: -10,
        });
    app.update();

    let log = &app.world().resource::<Log>().0;
    assert_eq!(log.len(), 2, "两个 observer 都应执行");
    assert!(
        log.contains(&"a") && log.contains(&"b"),
        "两个 observer 都应留下记录（此处刻意不断言顺序）"
    );
}
