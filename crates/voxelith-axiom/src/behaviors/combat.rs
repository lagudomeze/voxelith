//! L1 战斗装配与配置汇总（**Q17 采纳 A**）。
//!
//! 这个模块**没有业务系统**，它只做两件事：
//!
//! 1. 持有 [`CombatConfig`]（**Resource**）：把各子域的配置聚成一份，
//!    内容层从文件 / 关卡数据加载后**一次注入**；
//! 2. 把配置**分发**成各子域自己的 Resource，并装配各子域的 Plugin。
//!
//! 因为持有独立配置，它不是"空壳 Plugin"（**R41.1**、**R46**、**R102**）。
//!
//! ```text
//! CombatConfig ─┬─► StatConfig / ModifierCaps        （属性）
//!               ├─► ResistanceCaps                   （抵抗）
//!               ├─► PipelineConfig                   （伤害管线）
//!               ├─► RollConfig / CombatRng(seed)     （判定层）
//!               └─► ContestParams / StatusConfig     （状态）
//! ```

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;

use crate::atoms::stats::{ModifierCaps, StatConfig};
use crate::behaviors::damage::DamagePlugin;
use crate::behaviors::damage_pipeline::{DamagePipelinePlugin, PipelineConfig};
use crate::behaviors::progression::ProgressionPlugin;
use crate::behaviors::resistance::{ResistanceCaps, ResistancePlugin};
use crate::behaviors::rolls::{CombatRng, DEFAULT_RNG_SEED, RollConfig, RollsPlugin};
use crate::behaviors::status::{ContestParams, StatusConfig, StatusPlugin};

/// 战斗配置汇总（**Resource**）：内容层注入一次，由 [`CombatPlugin`] 分发。
#[derive(Resource, Debug, Clone, Copy, PartialEq, Default)]
pub struct CombatConfig {
    /// 属性：取整口径。
    pub stats: StatConfig,
    /// 修饰符聚合上限。
    pub modifier_caps: ModifierCaps,
    /// 抵抗上限与保底伤害。
    pub resistance: ResistanceCaps,
    /// 伤害管线：取整口径与暴击倍率。
    pub pipeline: PipelineConfig,
    /// 判定层：闪避 / 暴击参数。
    pub rolls: RollConfig,
    /// 状态：结算步长与时长上限。
    pub status: StatusConfig,
    /// 状态判定参数（物理 / 法术 / 精神共用）。
    pub contest: ContestParams,
    /// 战斗 RNG 种子（同种子 = 同结果，便于复现与测试）。
    pub rng_seed: u64,
}

impl CombatConfig {
    /// 用默认参数构造（种子用 [`DEFAULT_RNG_SEED`]）。
    pub fn new() -> Self {
        Self {
            rng_seed: DEFAULT_RNG_SEED,
            ..Default::default()
        }
    }
}

/// 战斗装配器：分发配置 + 装配子域 Plugin。
pub struct CombatPlugin;

impl Plugin for CombatPlugin {
    fn build(&self, app: &mut App) {
        // 内容层若已注入 `CombatConfig` 就照它分发；否则用默认配置。
        let config = app
            .world()
            .get_resource::<CombatConfig>()
            .copied()
            .unwrap_or_default();

        app.insert_resource(config.stats)
            .insert_resource(config.modifier_caps)
            .insert_resource(config.resistance)
            .insert_resource(config.pipeline)
            .insert_resource(config.rolls)
            .insert_resource(config.status)
            .insert_resource(config.contest)
            .insert_resource(CombatRng::new(config.rng_seed))
            .add_plugins((
                ProgressionPlugin,
                DamagePlugin,
                ResistancePlugin,
                RollsPlugin,
                DamagePipelinePlugin,
                StatusPlugin,
            ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_distributes_every_sub_config() {
        let mut app = App::new();
        app.insert_resource(CombatConfig::new());
        app.add_plugins(CombatPlugin);

        let world = app.world();
        assert!(world.get_resource::<StatConfig>().is_some());
        assert!(world.get_resource::<ModifierCaps>().is_some());
        assert!(world.get_resource::<ResistanceCaps>().is_some());
        assert!(world.get_resource::<PipelineConfig>().is_some());
        assert!(world.get_resource::<RollConfig>().is_some());
        assert!(world.get_resource::<StatusConfig>().is_some());
        assert!(world.get_resource::<ContestParams>().is_some());
        assert!(world.get_resource::<CombatRng>().is_some());
    }

    #[test]
    fn content_config_overrides_defaults() {
        let mut config = CombatConfig::new();
        config.pipeline.crit_multiplier = 3.0;
        config.rng_seed = 7;

        let mut app = App::new();
        app.insert_resource(config);
        app.add_plugins(CombatPlugin);

        let world = app.world();
        assert_eq!(
            world
                .get_resource::<PipelineConfig>()
                .unwrap()
                .crit_multiplier,
            3.0
        );
        assert_eq!(
            world.get_resource::<CombatRng>().copied(),
            Some(CombatRng::new(7))
        );
    }
}
