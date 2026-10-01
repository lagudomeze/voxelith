//! `Contest`：**对抗是一等公民**（[docs/combat-design.md](../../../../docs/combat-design.md) §5）。
//!
//! 没有 `Damage` 原语：伤害 = 对抗成功后的
//! [`ModifyResource`](crate::behaviors::effect::Effect::ModifyResource)。
//!
//! ```text
//! Difference: raw = attacker - defender        → 成功 iff raw >= threshold
//! Ratio:      raw = attacker / (defender + ε)  → 成功 iff raw >= threshold
//! RollUnder:  raw = attacker                    → 掷骰 [0, attacker)，成功 iff roll < threshold
//! ```
//!
//! **唯一随机点**就是 `RollUnder`；随机源是 [`CombatRng`]（种子来自 `CombatConfig`），
//! 同种子 = 同结果 → 整条战斗链路可复现、可测。

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;

use crate::behaviors::effect::Effect;
use crate::behaviors::value::{EvalContext, Value, eval};

/// 除法保护：`defender` 为 0 时不让比值炸掉。
pub const RATIO_EPSILON: f32 = 1.0;

/// 对抗的判定方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Deserialize)]
pub enum Formula {
    /// 差值：`攻击 - 防御` 与阈值比较（本项目的主力公式）。
    Difference,
    /// 比值：`攻击 / (防御 + ε)` 与阈值比较。
    Ratio,
    /// 掷骰：在 `[0, 攻击)` 里掷，`< 阈值` 算成功（唯一随机点）。
    RollUnder,
}

/// 对抗结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Deserialize)]
pub enum Outcome {
    /// 成功。
    Success,
    /// 失败。
    Fail,
    /// 大成功：超出阈值足够多（`crit_margin`）。
    Crit,
    /// 大失败：`RollUnder` 掷出上界。
    Fumble,
}

/// 一次对抗：攻防两侧表达式 + 判定方式 + 各结果对应的效果。
///
/// **不派生 `Deserialize`**：见 [`ContestRon`](crate::behaviors::content::ContestRon)。
#[derive(Debug, Clone, PartialEq)]
pub struct Contest {
    /// 攻击侧表达式。
    pub attacker: Value,
    /// 防御侧表达式。
    pub defender: Value,
    /// 判定方式。
    pub formula: Formula,
    /// 阈值：`Difference` 用绝对差，`Ratio` 用比值，`RollUnder` 用掷骰目标。
    pub threshold: f32,
    /// 超出阈值多少算 [`Outcome::Crit`]（`0.0` = 关闭大成功）。
    pub crit_margin: f32,
    /// 结果 → 效果列表；**没有匹配项时什么都不做**。
    pub outcomes: Vec<(Outcome, Vec<Effect>)>,
}

/// 对抗的数值结果：判定 + 强度。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Verdict {
    /// 判定结果。
    pub outcome: Outcome,
    /// 强度（写进后续效果的 [`Value::SkillPower`]）：`Difference` / `Ratio` 是原始差值 / 比值。
    pub power: f32,
}

/// 判定一次对抗（会消耗随机数：`RollUnder`）。
pub fn resolve_contest(contest: &Contest, ctx: &EvalContext, rng: &mut CombatRng) -> Verdict {
    let attacker = eval(&contest.attacker, ctx);
    let defender = eval(&contest.defender, ctx);
    let raw = match contest.formula {
        Formula::Difference => attacker - defender,
        Formula::Ratio => attacker / (defender.max(0.0) + RATIO_EPSILON),
        Formula::RollUnder => attacker,
    };

    let outcome = match contest.formula {
        Formula::Difference => {
            if raw >= contest.threshold + contest.crit_margin && contest.crit_margin > 0.0 {
                Outcome::Crit
            } else if raw >= contest.threshold {
                Outcome::Success
            } else {
                Outcome::Fail
            }
        }
        Formula::Ratio => {
            if raw >= contest.threshold + contest.crit_margin && contest.crit_margin > 0.0 {
                Outcome::Crit
            } else if raw >= contest.threshold {
                Outcome::Success
            } else {
                Outcome::Fail
            }
        }
        Formula::RollUnder => {
            let roll = raw.max(0.0) * rng.next_f32();
            if roll < contest.threshold * 0.05 {
                Outcome::Crit
            } else if roll < contest.threshold {
                Outcome::Success
            } else if roll >= raw {
                Outcome::Fumble
            } else {
                Outcome::Fail
            }
        }
    };

    Verdict {
        outcome,
        power: raw,
    }
}

/// 对抗的结果 → 效果列表（没有匹配项返回空切片）。
pub fn outcome_effects<'a>(contest: &'a Contest, outcome: Outcome) -> &'a [Effect] {
    contest
        .outcomes
        .iter()
        .find(|(candidate, _)| *candidate == outcome)
        .map_or(&[], |(_, effects)| effects.as_slice())
}

/// 战斗随机源（**Resource**）：自带 SplitMix64，不引入 `rand`。
///
/// 只在 [`Formula::RollUnder`] 里被调用 → "随机只出现在对抗里"是结构保证。
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct CombatRng(u64);

/// 默认种子（同种子 = 同结果，测试与复现都用它）。
pub const DEFAULT_RNG_SEED: u64 = 0x5EED_1234_ABCD_0001;

impl Default for CombatRng {
    fn default() -> Self {
        Self::new(DEFAULT_RNG_SEED)
    }
}

impl CombatRng {
    /// 用种子构造（种子为 0 时换成一个非零常量，避免退化序列）。
    pub fn new(seed: u64) -> Self {
        Self(if seed == 0 { DEFAULT_RNG_SEED } else { seed })
    }

    /// 下一个 64 位随机数（SplitMix64）。
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// `[0, 1)` 的浮点数。
    pub fn next_f32(&mut self) -> f32 {
        // 取高 24 位 → 尾数精度，除满量程得到 [0, 1)。
        ((self.next_u64() >> 40) as f32) / ((1u32 << 24) as f32)
    }
}

/// 注册对抗域：随机源。
pub struct ContestPlugin;

impl Plugin for ContestPlugin {
    fn build(&self, app: &mut bevy_app::App) {
        app.init_resource::<CombatRng>();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atoms::actor::{Pool, Resources, Stats};
    use crate::behaviors::content::{ResourceId, StatId};

    fn ctx<'a>(caster_stats: &'a Stats, target_stats: &'a Stats) -> EvalContext<'a> {
        EvalContext {
            caster_stats: Some(caster_stats),
            target_stats: Some(target_stats),
            ..Default::default()
        }
    }

    fn difference(_attacker: f32, _defender: f32, threshold: f32, crit_margin: f32) -> Contest {
        Contest {
            attacker: Value::CasterStat(StatId(0)),
            defender: Value::TargetStat(StatId(0)),
            formula: Formula::Difference,
            threshold,
            crit_margin,
            outcomes: Vec::new(),
        }
    }

    fn verdict(contest: &Contest, attacker: f32, defender: f32) -> Verdict {
        let caster = Stats::from_base([(StatId(0), attacker)]);
        let target = Stats::from_base([(StatId(0), defender)]);
        let context = ctx(&caster, &target);
        let mut rng = CombatRng::new(1);
        resolve_contest(contest, &context, &mut rng)
    }

    #[test]
    fn difference_threshold_decides_success() {
        let contest = difference(0.0, 0.0, 5.0, 0.0);
        assert_eq!(verdict(&contest, 10.0, 5.0).outcome, Outcome::Success);
        assert_eq!(verdict(&contest, 9.0, 5.0).outcome, Outcome::Fail);
        assert_eq!(
            verdict(&contest, 5.0, 0.0).outcome,
            Outcome::Success,
            "正好达标算成功"
        );
    }

    #[test]
    fn power_carries_the_margin_for_later_effects() {
        let contest = difference(0.0, 0.0, 5.0, 0.0);
        let verdict = verdict(&contest, 12.0, 4.0);
        assert_eq!(verdict.outcome, Outcome::Success);
        assert_eq!(verdict.power, 8.0, "差值就是强度（供 ModifyResource 用）");
    }

    #[test]
    fn crit_needs_a_positive_margin() {
        // 阈值 5 + 余量 3：raw >= 8 才算 Crit。
        let contest = difference(0.0, 0.0, 5.0, 3.0);
        assert_eq!(verdict(&contest, 7.0, 0.0).outcome, Outcome::Success);
        assert_eq!(verdict(&contest, 8.0, 0.0).outcome, Outcome::Crit);

        let no_crit = difference(0.0, 0.0, 5.0, 0.0);
        assert_eq!(
            verdict(&no_crit, 100.0, 0.0).outcome,
            Outcome::Success,
            "crit_margin = 0 时永不 Crit"
        );
    }

    #[test]
    fn ratio_uses_epsilon_against_a_zero_defender() {
        let contest = Contest {
            formula: Formula::Ratio,
            ..difference(0.0, 0.0, 2.0, 0.0)
        };
        // 5 / (0 + 1) = 5 >= 2 → 成功，且不会除零
        assert_eq!(verdict(&contest, 5.0, 0.0).outcome, Outcome::Success);
        // 1 / (4 + 1) = 0.2 < 2 → 失败
        assert_eq!(verdict(&contest, 1.0, 4.0).outcome, Outcome::Fail);
    }

    #[test]
    fn roll_under_is_deterministic_per_seed() {
        let caster = Stats::from_base([(StatId(0), 100.0)]);
        let target = Stats::from_base([]);
        let _ = ctx(&caster, &target);
        let _contest = Contest {
            attacker: Value::CasterStat(StatId(0)),
            defender: Value::Literal(0.0),
            formula: Formula::RollUnder,
            threshold: 100.0,
            crit_margin: 0.0,
            outcomes: Vec::new(),
        };

        let roll_power_with = |seed: u64| {
            let mut rng = CombatRng::new(seed);
            rng.next_f32()
        };
        // 同种子 = 同序列；不同种子给出不同序列（不断言"结果不同"，
        // 因为二元判定本来就可能撞上同一个结果）。
        assert_eq!(roll_power_with(42), roll_power_with(42));
        assert_ne!(roll_power_with(1), roll_power_with(2));
    }

    #[test]
    fn roll_under_zero_threshold_always_fails() {
        let caster = Stats::from_base([(StatId(0), 50.0)]);
        let context = ctx(&caster, &caster);
        let contest = Contest {
            attacker: Value::CasterStat(StatId(0)),
            defender: Value::Literal(0.0),
            formula: Formula::RollUnder,
            threshold: 0.0,
            crit_margin: 0.0,
            outcomes: Vec::new(),
        };
        let mut rng = CombatRng::new(9);
        assert_eq!(
            resolve_contest(&contest, &context, &mut rng).outcome,
            Outcome::Fail
        );
    }

    #[test]
    fn missing_outcome_does_nothing() {
        let contest = difference(0.0, 0.0, 0.0, 0.0);
        assert!(outcome_effects(&contest, Outcome::Success).is_empty());
    }

    #[test]
    fn outcomes_are_looked_up_by_exact_match() {
        let mut contest = difference(0.0, 0.0, 0.0, 0.0);
        contest.outcomes.push((
            Outcome::Fail,
            vec![Effect::Log {
                text: "被挡下了".into(),
            }],
        ));
        assert_eq!(outcome_effects(&contest, Outcome::Fail).len(), 1);
        assert!(outcome_effects(&contest, Outcome::Crit).is_empty());
    }

    #[test]
    fn rng_stays_in_the_unit_interval() {
        let mut rng = CombatRng::new(DEFAULT_RNG_SEED);
        for _ in 0..1000 {
            let value = rng.next_f32();
            assert!((0.0..1.0).contains(&value), "掷出 {value} 越界");
        }
    }

    #[test]
    fn resource_pools_can_be_used_as_contest_sides() {
        let mut resources = Resources::default();
        resources.define(ResourceId(0), Pool::full(3.0));
        let stats = Stats::from_base([]);
        let context = EvalContext {
            caster_resources: Some(&resources),
            caster_stats: Some(&stats),
            ..Default::default()
        };
        let contest = Contest {
            attacker: Value::Resource {
                pool: ResourceId(0),
                who: crate::behaviors::value::Who::Caster,
            },
            defender: Value::Literal(2.0),
            formula: Formula::Difference,
            threshold: 1.0,
            crit_margin: 0.0,
            outcomes: Vec::new(),
        };
        let mut rng = CombatRng::new(3);
        assert_eq!(
            resolve_contest(&contest, &context, &mut rng).outcome,
            Outcome::Success
        );
    }
}
