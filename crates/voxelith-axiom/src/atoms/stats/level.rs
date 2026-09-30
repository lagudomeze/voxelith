//! L0 等级与经验。
//!
//! - [`Level`] 是数据组件：字段私有，唯一改值入口是 `gain_exp`（`pub(crate)`，由 L1 的
//!   `behaviors::progression` 消费 `GainExperienceMessage` 后调用）。
//! - [`LevelConfig`] 是**配置 Resource**（可来自文件 / 内容层）：经验曲线数据驱动，
//!   曲线本身通过 [`LevelCurve`] 抽象，方便测试注入桩实现或换表。

use bevy_ecs::prelude::*;
use bevy_reflect::Reflect;

/// 经验曲线：`Level` 不认识具体数值，规则由外部传入。
pub trait LevelCurve: Send + Sync + 'static {
    /// 等级上限。
    fn max_level(&self) -> u32;
    /// 每升一级发放的可分配点数。
    fn points_per_level(&self) -> u32;
    /// 从 `current_level` 升到下一级所需经验。
    fn exp_required(&self, current_level: u32) -> u64;
}

/// 通用经验曲线配置（**Resource**）：`exp = base_exp * level^exp_power * exp_penalty`。
#[derive(Resource, Debug, Clone, Copy, Reflect)]
pub struct LevelConfig {
    /// 等级上限。
    pub max_level: u32,
    /// 每级发放的可分配点数。
    pub points_per_level: u32,
    /// 曲线基数。
    pub base_exp: u64,
    /// 曲线指数。
    pub exp_power: f32,
    /// 种族经验惩罚（人类 = 1.0）。
    pub exp_penalty: f32,
}

impl Default for LevelConfig {
    fn default() -> Self {
        Self {
            max_level: 50,
            points_per_level: 5,
            base_exp: 100,
            exp_power: 2.0,
            exp_penalty: 1.0,
        }
    }
}

impl LevelCurve for LevelConfig {
    fn max_level(&self) -> u32 {
        self.max_level
    }

    fn points_per_level(&self) -> u32 {
        self.points_per_level
    }

    fn exp_required(&self, current_level: u32) -> u64 {
        let raw = self.base_exp as f64
            * (current_level as f64).powf(self.exp_power as f64)
            * self.exp_penalty as f64;
        raw as u64
    }
}

/// 等级与经验（组件）。字段私有：唯一改值入口是 [`Level::gain_exp`]。
#[derive(Component, Debug, Clone, Copy)]
pub struct Level {
    current: u32,
    experience: u64,
}

impl Level {
    /// 从 1 级、0 经验开始。
    pub const fn new() -> Self {
        Self {
            current: 1,
            experience: 0,
        }
    }

    /// 当前等级。
    pub fn current(&self) -> u32 {
        self.current
    }

    /// 当前等级内已累积的经验。
    pub fn experience(&self) -> u64 {
        self.experience
    }

    /// 投入经验，返回**本次升了几级**（可能一次升多级）。
    ///
    /// 升级消耗按 `curve.exp_required(当前等级)` 逐级扣；到达 `max_level` 后停住，经验继续累积。
    pub(crate) fn gain_exp(&mut self, amount: u64, curve: &impl LevelCurve) -> u32 {
        self.experience += amount;
        let mut gained = 0;
        while self.current < curve.max_level() {
            let required = curve.exp_required(self.current);
            if self.experience < required {
                break;
            }
            self.experience -= required;
            self.current += 1;
            gained += 1;
        }
        gained
    }
}

impl Default for Level {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试桩曲线：每级都要 10 点经验，每级给 2 点属性点，上限 3 级。
    struct StubCurve;

    impl LevelCurve for StubCurve {
        fn max_level(&self) -> u32 {
            3
        }
        fn points_per_level(&self) -> u32 {
            2
        }
        fn exp_required(&self, _current_level: u32) -> u64 {
            10
        }
    }

    #[test]
    fn gain_exp_can_level_up_multiple_times() {
        let mut level = Level::new();
        assert_eq!(level.gain_exp(25, &StubCurve), 2);
        assert_eq!(level.current(), 3);
        assert_eq!(level.experience(), 5);
    }

    #[test]
    fn level_stops_at_max_level_but_keeps_experience() {
        let mut level = Level::new();
        level.gain_exp(1000, &StubCurve);
        assert_eq!(level.current(), 3, "上限 3 级");
        assert!(level.experience() > 0, "溢出经验保留");
    }

    #[test]
    fn config_curve_grows_with_level() {
        let config = LevelConfig::default();
        assert!(config.exp_required(2) > config.exp_required(1));
    }
}
