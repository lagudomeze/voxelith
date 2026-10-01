//! 时间控制：**用 `Time<Virtual>` 的倍率表达"冻结"**。
//!
//! 所有逻辑系统读 `Res<Time>`（虚拟时间），UI / 动画读 `Res<Time<Real>>`（真实时间）。
//! 冻结 = 倍率 `0.0`，于是逻辑系统的 `delta` 就是 `0`——**逻辑代码完全不需要知道相位**。
//!
//! 倍率只由 [`CombatPhase`] 决定（[`drive_virtual_time`]），别处不许写时间倍率。

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_state::prelude::*;
use bevy_time::{Time, Virtual};

use crate::behaviors::phase::CombatPhase;

/// 时间步进策略（**Resource**）：`Time<Virtual>` 的 `max_delta`（单帧最多推进多少虚拟时间）。
///
/// 默认 250ms 是 Bevy 的默认值。**它真的有消费者**：`TimeControlPlugin` 在 `build` 里把它
/// 写进 `Time<Virtual>`，所以内容层在 `TimeControlPlugin` 之前 `insert_resource` 就能改掉它。
///
/// 为什么需要它：一帧真实时长超过 `max_delta` 会被**夹掉**（Bevy 的防跳帧保护）。
/// 调试暂停、加载卡顿之后，逻辑时间不会突然跳一大截——但超过部分就真的丢了，
/// 想让它丢得少一点（或反过来"卡顿后追帧"）就得调这个值。
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct VirtualTimeConfig {
    /// 单帧最大推进量。
    pub max_delta: core::time::Duration,
}

impl Default for VirtualTimeConfig {
    fn default() -> Self {
        Self {
            max_delta: core::time::Duration::from_millis(250),
        }
    }
}

/// 按相位设置虚拟时间倍率：`Resolving` 流 1.0，冻结相位 0.0。
///
/// **放在 `First` 且 `.after(time_system)`，而且必须是"唯一写倍率的地方"**——
/// 这两条都不是风格问题，是 `Main` 的顺序决定的：
///
/// ```text
/// First            time_system      用当前倍率把本帧 delta 定死；然后本系统改倍率
/// PreUpdate        StateTransition  把上一帧写下的 NextState 落成 State
/// Update           update_phase     按世界状态算出下一帧的相位（写 NextState）
/// ```
///
/// 于是 `update_phase` 写下的相位，在**下一帧 `PreUpdate` 开头**成为 `State`，
/// 而本系统在**再下一帧 `First`** 读到它并改倍率——换句话说：
///
/// - 冻结的生效延迟是**恰好一帧**（`update_phase` 的帧还没冻结，次帧冻结）；
/// - 任意时刻 `delta != 0` ⟺ `State` 是 `Resolving`，**两者永远一致**（这才是要钉的性质）。
///
/// 反例（曾经的写法）：放在 `PreUpdate` 且不排序。那时它跑在 `time_system` **之后**，
/// 于是倍率改完还要等两帧才影响 `delta`，`AwaitingInput` 期间怪物会多走一步、状态会多跳一次。
pub fn drive_virtual_time(phase: Res<State<CombatPhase>>, mut virtual_time: ResMut<Time<Virtual>>) {
    let speed = match phase.get() {
        CombatPhase::Resolving => 1.0,
        CombatPhase::AwaitingInput | CombatPhase::AwaitingCounter => 0.0,
    };
    if (virtual_time.relative_speed() - speed).abs() > f32::EPSILON {
        virtual_time.set_relative_speed(speed);
    }
}

/// 装配时间控制：`Time<Virtual>` 资源兜底 + 倍率驱动。
///
/// `Time<Virtual>` 正常由 L2 的 `DefaultPlugins`（内含 `TimePlugin`）提供；
/// 这里 `init_resource` 只是让 axiom 在裸 `App::new()` 下也能跑（**Q21**：axiom 允许 `bevy_time`）。
///
/// 顺序是**契约**，理由见 [`drive_virtual_time`]。
pub struct TimeControlPlugin;

impl Plugin for TimeControlPlugin {
    fn build(&self, app: &mut App) {
        // 先 `init_resource`：内容层若在**本插件之前**插入了配置，这里不会被覆盖。
        app.init_resource::<Time<Virtual>>()
            .init_resource::<VirtualTimeConfig>()
            .add_systems(First, drive_virtual_time.after(bevy_time::time_system));

        // 把配置**落地**到 `Time<Virtual>`。
        //
        // 只在 `build` 里做一次（而不是每帧写）：`max_delta` 是"配置"，
        // 谁都可以在后面改；每帧覆写会让"运行中调它"这种用法失效。
        let max_delta = app.world().resource::<VirtualTimeConfig>().max_delta;
        app.world_mut()
            .resource_mut::<Time<Virtual>>()
            .set_max_delta(max_delta);
    }
}

/// 测试 / 内容层用：有没有正被冻结的相位。
pub fn is_frozen(phase: CombatPhase) -> bool {
    matches!(
        phase,
        CombatPhase::AwaitingInput | CombatPhase::AwaitingCounter
    )
}

/// 测试专用：把虚拟时间推进 `seconds` 秒。
///
/// 它模拟"时间插件在帧间推进时钟"，让 axiom 的测试不依赖 `TimePlugin`。
pub fn advance_virtual_time(app: &mut App, seconds: f32) {
    let mut time = app.world_mut().resource_mut::<Time<Virtual>>();
    time.advance_by(core::time::Duration::from_secs_f32(seconds));
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_time::{TimePlugin, TimeUpdateStrategy};

    /// 装一个**真实时间插件**的 App（否则 `time_system` 不存在，倍率也就没有消费者）。
    fn app() -> App {
        let mut app = App::new();
        // init_state 需要状态转移调度（线上由 DefaultPlugins 提供）。
        app.add_plugins((TimePlugin, bevy_state::app::StatesPlugin));
        app.init_state::<CombatPhase>();
        app.add_plugins(TimeControlPlugin);
        app
    }

    /// 走一帧，真实时长固定为 `seconds`（`max_delta` 一起放宽，免得被夹到 250ms）。
    fn frame(app: &mut App, seconds: f32) {
        let duration = core::time::Duration::from_secs_f32(seconds);
        app.world_mut()
            .resource_mut::<Time<Virtual>>()
            .set_max_delta(duration.max(core::time::Duration::from_millis(1)));
        app.world_mut()
            .insert_resource(TimeUpdateStrategy::ManualDuration(duration));
        app.update();
    }

    /// 把相位设过去（模拟 `update_phase`），并停在被冻结/流动的状态上。
    fn enter(app: &mut App, phase: CombatPhase) {
        app.world_mut()
            .resource_mut::<NextState<CombatPhase>>()
            .set(phase);
        // 第 1 帧应用转移，第 2 帧倍率跟上——之后相位不再变。
        frame(app, 0.0);
        frame(app, 0.0);
        assert_eq!(*app.world().resource::<State<CombatPhase>>().get(), phase);
    }

    fn speed(app: &App) -> f32 {
        app.world().resource::<Time<Virtual>>().relative_speed()
    }

    #[test]
    fn resolving_runs_at_normal_speed() {
        let mut app = app();
        enter(&mut app, CombatPhase::Resolving);
        assert_eq!(speed(&app), 1.0);
        frame(&mut app, 0.5);
        assert!(
            app.world().resource::<Time>().delta_secs() > 0.0,
            "流动相位下 delta 必须非零"
        );
    }

    #[test]
    fn awaiting_input_freezes_time() {
        let mut app = app();
        enter(&mut app, CombatPhase::AwaitingInput);
        assert_eq!(speed(&app), 0.0, "等输入时时间必须冻结");

        frame(&mut app, 0.5);
        assert_eq!(
            app.world().resource::<Time>().delta_secs(),
            0.0,
            "冻结相位下逻辑系统读到的 delta 必须是 0"
        );
    }

    #[test]
    fn awaiting_counter_freezes_time() {
        let mut app = app();
        enter(&mut app, CombatPhase::AwaitingCounter);
        assert_eq!(speed(&app), 0.0);
        frame(&mut app, 0.5);
        assert_eq!(app.world().resource::<Time>().delta_secs(), 0.0);
    }

    #[test]
    fn returning_to_resolving_resumes_time() {
        let mut app = app();
        enter(&mut app, CombatPhase::AwaitingInput);
        frame(&mut app, 0.5);
        assert_eq!(app.world().resource::<Time>().delta_secs(), 0.0);

        enter(&mut app, CombatPhase::Resolving);
        assert_eq!(speed(&app), 1.0, "回到流动相位后倍率恢复");
        frame(&mut app, 0.5);
        assert!(
            app.world().resource::<Time>().delta_secs() > 0.0,
            "恢复后 delta 也要恢复"
        );
    }

    #[test]
    fn virtual_time_config_reaches_the_clock() {
        // 配置必须**真的**落到 `Time<Virtual>`：文档说它"可注入"，那就得有人读它。
        let mut app = App::new();
        app.add_plugins((TimePlugin, bevy_state::app::StatesPlugin));
        app.init_state::<CombatPhase>();
        // 内容层在插件之前注入自己的策略。
        app.insert_resource(VirtualTimeConfig {
            max_delta: core::time::Duration::from_millis(50),
        });
        app.add_plugins(TimeControlPlugin);

        assert_eq!(
            app.world().resource::<Time<Virtual>>().max_delta(),
            core::time::Duration::from_millis(50),
            "配置得有人读：否则它就是个骗人的字段"
        );
    }

    #[test]
    fn ticks_are_clamped_to_the_config_ceiling() {
        // 夹断**真的发生**：一帧 1 秒只会推进 50ms（Bevy 的防跳帧保护）。
        // 直接给策略、不再经 `TimeControlPlugin`，免得又被 `frame()` 的放宽覆盖。
        let mut app = App::new();
        app.add_plugins((TimePlugin, bevy_state::app::StatesPlugin));
        app.init_state::<CombatPhase>();
        app.add_plugins(TimeControlPlugin);
        app.world_mut()
            .resource_mut::<Time<Virtual>>()
            .set_max_delta(core::time::Duration::from_millis(50));

        let duration = core::time::Duration::from_secs(1);
        app.world_mut()
            .insert_resource(TimeUpdateStrategy::ManualDuration(duration));
        app.update(); // 初始化帧：只记起始时刻，delta 仍是 0
        app.update();

        assert_eq!(
            app.world().resource::<Time>().delta(),
            core::time::Duration::from_millis(50),
            "超过上限的部分被夹掉了"
        );
    }

    #[test]
    fn delta_and_phase_never_disagree() {
        // 这条是要钉的核心性质：**逻辑系统读到的 delta 与 State 一致**。
        // 倍率改晚一帧的话，冻结期间 delta 会漏出一帧（怪物多走一步）。
        let mut app = app();
        let mut frozen_frames = 0;
        for (index, phase) in [
            CombatPhase::Resolving,
            CombatPhase::AwaitingInput,
            CombatPhase::AwaitingInput,
            CombatPhase::AwaitingInput,
            CombatPhase::Resolving,
            CombatPhase::Resolving,
        ]
        .into_iter()
        .enumerate()
        {
            enter(&mut app, phase);
            frame(&mut app, 1.0);
            let delta = app.world().resource::<Time>().delta_secs();
            let current = *app.world().resource::<State<CombatPhase>>().get();
            if is_frozen(current) {
                frozen_frames += 1;
                assert_eq!(delta, 0.0, "第 {index} 步：冻结相位却给了 delta={delta}");
            } else if index > 0 {
                assert!(delta > 0.0, "第 {index} 步：流动相位却给了 delta=0");
            }
        }
        assert_eq!(frozen_frames, 3, "三次冻结各测了一帧");
    }
}
