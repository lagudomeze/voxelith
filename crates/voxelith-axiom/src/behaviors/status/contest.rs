//! 统一概率豁免框架：**攻防类型只决定"取哪对数值"，判定算法只有一份**。
//!
//! - [`ContestKind`] 负责把"物理 / 法术 / 精神"映射到具体的攻防属性；
//! - [`contest_chance`] / [`contest_duration`] 是所有状态共用的唯一公式；
//! - 参数全部来自 [`ContestParams`]（**Resource**），调平衡不动代码。

use core::time::Duration;

use bevy_ecs::prelude::*;

use crate::atoms::stats::{Stat, StatId};

/// 攻防类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ContestKind {
    /// 物理：力量 vs 体质。
    #[default]
    Physical,
    /// 法术：魔力 vs 意志。
    Spell,
    /// 精神：机敏 vs 意志。
    Mental,
}

impl ContestKind {
    /// 攻击方使用的属性。
    pub const fn offense_stat(self) -> StatId {
        match self {
            ContestKind::Physical => StatId::Strength,
            ContestKind::Spell => StatId::Magic,
            ContestKind::Mental => StatId::Cunning,
        }
    }

    /// 防御方使用的属性。
    pub const fn defense_stat(self) -> StatId {
        match self {
            ContestKind::Physical => StatId::Constitution,
            ContestKind::Spell | ContestKind::Mental => StatId::Willpower,
        }
    }

    /// 取双方的强度（读的是 `Stat` 的**最终值视图**）。
    pub fn scores(self, attacker: &Stat, defender: &Stat) -> (f32, f32) {
        (
            attacker.get(self.offense_stat()) as f32,
            defender.get(self.defense_stat()) as f32,
        )
    }
}

/// 判定参数（**Resource**：内容层 / 文件注入）。
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct ContestParams {
    /// 强度差的影响力（0.5 = 强度差 100% 时概率偏移 50 个百分点）。
    pub slope: f32,
    /// 概率下限（再强也不能必中）。
    pub min_chance: f32,
    /// 概率上限（再弱也不会必失）。
    pub max_chance: f32,
    /// 时长随强度差放大的系数。
    pub duration_scale: f32,
    /// 时长放大上限（防止无限控）。
    pub max_duration_multiplier: f32,
}

impl Default for ContestParams {
    fn default() -> Self {
        Self {
            slope: 0.5,
            min_chance: 0.05,
            max_chance: 0.95,
            duration_scale: 1.0,
            max_duration_multiplier: 3.0,
        }
    }
}

/// 统一概率公式：`0.5 + slope * (off - def) / (off + def)`，夹在 `[min_chance, max_chance]`。
pub fn contest_chance(offense: f32, defense: f32, params: &ContestParams) -> f32 {
    let total = offense + defense;
    let ratio = if total > 0.0 {
        (offense - defense) / total
    } else {
        0.0
    };
    (0.5 + params.slope * ratio).clamp(params.min_chance, params.max_chance)
}

/// 时长加成：与概率**共用同一个强度比**，但不吃概率的上下限。
///
/// 理由：概率被夹在 `[min_chance, max_chance]`（"再强也不能必中"）之后，
/// 如果时长照夹过的概率算，就永远够不到 [`ContestParams::max_duration_multiplier`]，
/// "强到一定程度控得更久"会退化。
fn duration_advantage(offense: f32, defense: f32, params: &ContestParams) -> f32 {
    let total = offense + defense;
    let ratio = if total > 0.0 {
        (offense - defense) / total
    } else {
        0.0
    };
    ratio.max(0.0) * params.slope * 2.0 * params.duration_scale
}

/// 统一时长公式：强度差越大持续越久，倍数夹在 `[1, max_duration_multiplier]`。
pub fn contest_duration(
    base: Duration,
    offense: f32,
    defense: f32,
    params: &ContestParams,
) -> Duration {
    let multiplier = (1.0 + duration_advantage(offense, defense, params))
        .clamp(1.0, params.max_duration_multiplier);
    base.mul_f32(multiplier)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atoms::stats::StatBlock;

    fn stat_with(strength: u32, constitution: u32) -> Stat {
        let mut base = StatBlock::default();
        base.set(StatId::Strength, strength);
        base.set(StatId::Constitution, constitution);
        Stat::from_base(base)
    }

    #[test]
    fn equal_strength_is_a_coin_flip() {
        let params = ContestParams::default();
        assert!((contest_chance(10.0, 10.0, &params) - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn stronger_offense_raises_and_is_capped() {
        let params = ContestParams::default();
        let weak = contest_chance(12.0, 10.0, &params);
        let strong = contest_chance(1_000.0, 1.0, &params);
        assert!(weak > 0.5);
        assert_eq!(strong, params.max_chance, "上限必须生效");
        assert_eq!(contest_chance(1.0, 1_000.0, &params), params.min_chance);
    }

    #[test]
    fn duration_grows_with_advantage_but_is_capped() {
        let params = ContestParams::default();
        let base = Duration::from_secs(10);
        let even = contest_duration(base, 10.0, 10.0, &params);
        let better = contest_duration(base, 20.0, 10.0, &params);

        assert_eq!(even, base);
        assert!(better > even);

        // 放大系数拉满时，必须被 `max_duration_multiplier` 夹住
        let greedy = ContestParams {
            duration_scale: 10.0,
            ..params
        };
        let best = contest_duration(base, 1_000.0, 1.0, &greedy);
        assert_eq!(best, base.mul_f32(greedy.max_duration_multiplier));
    }

    #[test]
    fn contest_kind_picks_the_right_attribute_pair() {
        let attacker = stat_with(30, 5);
        let defender = stat_with(10, 12);
        let (offense, defense) = ContestKind::Physical.scores(&attacker, &defender);
        assert_eq!((offense, defense), (30.0, 12.0));
        assert_eq!(ContestKind::Spell.offense_stat(), StatId::Magic);
        assert_eq!(ContestKind::Mental.defense_stat(), StatId::Willpower);
    }
}
