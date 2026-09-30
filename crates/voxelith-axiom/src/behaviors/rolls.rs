//! L1 概率判定：命中 / 闪避 / 暴击 —— **全项目唯一允许出现随机的地方**（不变量 I4）。
//!
//! - 管线是确定性的，所以随机只能发生在这里，并把结果以 `missed` / `crit` 布尔写进
//!   [`DamageRequest`]，让管线保持"同输入同输出"。
//! - RNG 自带（SplitMix64，十几行，不引入 `rand`），种子来自 [`CombatConfig`]，
//!   测试注入固定种子即可复现整条链路。
//! - 判定顺序**显式固定**：先规避、后暴击（`.chain()`），不依赖调度顺序。
//!
//! [`CombatConfig`]: crate::behaviors::combat::CombatConfig

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;

use crate::behaviors::damage::{DamageRequest, DamageTags};
use crate::behaviors::resistance::Resistance;

/// 默认种子：内容层没给配置时也保证可复现。
pub const DEFAULT_RNG_SEED: u64 = 0x5EED_1234_5678_9ABC;

/// SplitMix64：小而快的确定性 PRNG（`next_u64` 是其标准实现）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    /// 用种子构造。
    pub const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// 下一个 64 位随机数。
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// `[0, 1)` 区间的小数（取高 24 位，float 精度足够）。
    pub fn next_unit(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    /// 以概率 `p` 命中（`p` 会被夹到 `[0, 1]`）。
    pub fn chance(&mut self, p: f32) -> bool {
        self.next_unit() < p.clamp(0.0, 1.0)
    }
}

/// 战斗 RNG（**Resource**）：整个战斗流程唯一的随机源。
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct CombatRng(SplitMix64);

impl CombatRng {
    /// 用种子构造。
    pub const fn new(seed: u64) -> Self {
        Self(SplitMix64::new(seed))
    }

    /// 掷一次 `[0, 1)`。
    pub fn roll(&mut self) -> f32 {
        self.0.next_unit()
    }

    /// 按概率掷骰。
    pub fn chance(&mut self, p: f32) -> bool {
        self.0.chance(p)
    }
}

impl Default for CombatRng {
    fn default() -> Self {
        Self::new(DEFAULT_RNG_SEED)
    }
}

/// 判定配置（**Resource**：内容层 / 文件注入）。
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct RollConfig {
    /// 基础闪避率（叠加到目标 `Resistance::evasion` 之上）。
    pub base_evasion: f32,
    /// 闪避率上限（防止 100% 规避）。
    pub max_evasion: f32,
    /// 基础暴击率。
    pub base_crit_chance: f32,
    /// 暴击倍率（在管线阶段二生效）。
    pub crit_multiplier: f32,
}

impl Default for RollConfig {
    fn default() -> Self {
        Self {
            base_evasion: 0.0,
            max_evasion: 0.75,
            base_crit_chance: 0.05,
            crit_multiplier: 2.0,
        }
    }
}

/// 规避判定：读目标 `evasion`，掷骰后在请求上打 `missed`。
///
/// 排在管线之前（`.before(DamageStage::Base)`），管线因此保持确定性。
pub fn roll_avoidance(
    mut requests: MessageMutator<DamageRequest>,
    mut rng: ResMut<CombatRng>,
    config: Res<RollConfig>,
    targets: Query<&Resistance>,
) {
    for request in requests.read() {
        if request.missed {
            continue;
        }
        let evasion = targets
            .get(request.target)
            .map(|resistance| resistance.evasion)
            .unwrap_or(0.0);
        let chance = (config.base_evasion + evasion).clamp(0.0, config.max_evasion);
        if rng.chance(chance) {
            request.missed = true;
        }
    }
}

/// 暴击判定：掷骰后在请求上打 `crit` 与 `DamageTags::CRIT`（倍率在管线阶段二生效）。
pub fn roll_crit(
    mut requests: MessageMutator<DamageRequest>,
    mut rng: ResMut<CombatRng>,
    config: Res<RollConfig>,
) {
    for request in requests.read() {
        if request.missed {
            continue;
        }
        if rng.chance(config.base_crit_chance) {
            request.crit = true;
            request.tags.insert(DamageTags::CRIT);
        }
    }
}

/// 注册判定层：RNG 资源、配置、按固定顺序串起来的两个判定系统。
pub struct RollsPlugin;

impl Plugin for RollsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CombatRng>()
            .init_resource::<RollConfig>()
            .add_systems(Update, (roll_avoidance, roll_crit).chain());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_gives_the_same_sequence() {
        let mut a = SplitMix64::new(42);
        let mut b = SplitMix64::new(42);
        let first: Vec<u64> = (0..8).map(|_| a.next_u64()).collect();
        let second: Vec<u64> = (0..8).map(|_| b.next_u64()).collect();
        assert_eq!(first, second);
    }

    #[test]
    fn different_seeds_diverge() {
        let mut a = SplitMix64::new(1);
        let mut b = SplitMix64::new(2);
        assert_ne!(a.next_u64(), b.next_u64());
    }

    #[test]
    fn unit_values_stay_in_range() {
        let mut rng = SplitMix64::new(DEFAULT_RNG_SEED);
        for _ in 0..1_000 {
            let value = rng.next_unit();
            assert!((0.0..1.0).contains(&value), "越界：{value}");
        }
    }

    #[test]
    fn chance_extremes_are_absolute() {
        let mut rng = SplitMix64::new(7);
        for _ in 0..100 {
            assert!(!rng.chance(0.0));
            assert!(rng.chance(1.0));
        }
    }

    #[test]
    fn chance_is_roughly_the_requested_probability() {
        let mut rng = SplitMix64::new(2024);
        let hits = (0..10_000).filter(|_| rng.chance(0.25)).count();
        assert!((2_300..2_700).contains(&hits), "命中数偏离：{hits}");
    }
}
