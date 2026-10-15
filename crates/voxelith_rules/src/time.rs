//! 世界时钟与冻结。
//!
//! 模型见 [`docs/design/04-time-and-actions.md`](../../../docs/design/04-time-and-actions.md)，
//! Bevy 侧的取舍与实测见 [`docs/design/bevy/03-time-and-actions.md`](../../../docs/design/bevy/03-time-and-actions.md)。
//!
//! - **用引擎的钟**：`Time<Virtual>` 当暂停开关 ✓ 逻辑系统住 `FixedUpdate`（64 步/秒）✓
//! - **时间轴基准是 `Time<Fixed>::elapsed()`** ✓（整数 tick ✓ 帧率无关 ✓）不是 Virtual 的 ✓
//! - **推进权只有一个**：只有 [`apply_freeze`] 会 `pause` / `unpause` ✓ 别处不许动 ✓

use bevy_ecs::prelude::*;
use bevy_time::{Time, Virtual};
use core::time::Duration;

/// 世界为什么停。三个触发任何一个成立就停（可以同时成立）。
#[derive(Resource, Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Freeze {
    /// 玩家没有在飞的前摇 → 该他做决定了。
    pub waiting_for_player: bool,
    /// 有指着玩家、还没生效的行动悬着（`GP-32`）。
    pub threat: bool,
    /// 玩家自己按的暂停：看盘用 ✓ **可以下指令** ✓ 代价照付 ✓
    pub manual: bool,
}

impl Freeze {
    /// 任何一个触发成立就停。
    pub fn any(&self) -> bool {
        self.waiting_for_player || self.threat || self.manual
    }
}

/// 把 [`Freeze`] 的结论施加到时钟上。放在 `RunFixedMainLoop` 之前（`PreUpdate`）。
///
/// **当帧的 `delta` 已经在 `First` 里算完了** ✓ 所以这里改动最多让世界多跑一拍（15.6ms ✓）。
pub fn apply_freeze(freeze: Res<Freeze>, mut virtual_time: ResMut<Time<Virtual>>) {
    let should_pause = freeze.any();
    if virtual_time.is_paused() != should_pause {
        if should_pause {
            virtual_time.pause();
        } else {
            virtual_time.unpause();
        }
    }
}

/// 把一段时长**四舍五入到最近的逻辑步**（`D-011`：内容写秒，内部换算成 tick）。
///
/// 0.2 秒 = 12.8 步 → 13 步 = 0.203125 秒 ✓
pub fn round_to_step(duration: Duration, step: Duration) -> Duration {
    let step = step.as_nanos();
    if step == 0 {
        return duration;
    }
    let rounded = (duration.as_nanos() + step / 2) / step * step;
    Duration::from_nanos(u64::try_from(rounded).unwrap_or(u64::MAX))
}

/// 秒 → 四舍五入到最近一步的时长。内容里写秒，进代码前过这一道。
pub fn seconds_to_step(seconds: f32, step: Duration) -> Duration {
    round_to_step(Duration::from_secs_f32(seconds.max(0.0)), step)
}

#[cfg(test)]
mod tests {
    use super::*;

    const STEP: Duration = Duration::from_micros(15625); // 1/64 秒

    #[test]
    fn rounds_to_the_nearest_step() {
        // 0.2 秒 = 12.8 步 → 13 步
        assert_eq!(
            seconds_to_step(0.2, STEP),
            Duration::from_micros(15625 * 13)
        );
        // 0.1 秒 = 6.4 步 → 6 步
        assert_eq!(seconds_to_step(0.1, STEP), Duration::from_micros(15625 * 6));
        // 已经正好是整数步就不动
        assert_eq!(seconds_to_step(0.25, STEP), Duration::from_micros(15625 * 16));
    }

    #[test]
    fn freeze_needs_only_one_trigger() {
        assert!(!Freeze::default().any());
        assert!(Freeze { manual: true, ..Default::default() }.any());
        assert!(Freeze { threat: true, ..Default::default() }.any());
        assert!(Freeze { waiting_for_player: true, ..Default::default() }.any());
    }
}
