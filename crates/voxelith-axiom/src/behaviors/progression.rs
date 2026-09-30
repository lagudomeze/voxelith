//! L1 成长：**经验 → 升级 → 发点数**。
//!
//! 为什么在 L1（**R11**）：它要同时读 `Level`（L0 数据）、`LevelConfig`（曲线 Resource）
//! 并给 `Stat`（L0 数据）发点数消息；单个组件自己跑不通。
//!
//! **R13**：不写 `Stat` / `Level` 之外的任何东西，也不直接改 L0——点数通过
//! [`GrantStatPointsMessage`] 交给 L0 记账，升级事实用 [`LevelUpMessage`] 广播给 UI / 表现。
//!
//! 配置：曲线来自 [`LevelConfig`]（**Resource**，内容层可换成文件加载的表）。

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;

use crate::atoms::stats::{
    GrantStatPointsMessage, Level, LevelConfig, LevelCurve, Stat, StatStage,
};

/// 外部（战斗 / 任务 / 调试）投放经验。
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct GainExperienceMessage {
    /// 目标实体。
    pub entity: Entity,
    /// 经验值。
    pub amount: u64,
}

/// 升级通知（广播给 UI / 表现 / 成就系统）。
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct LevelUpMessage {
    /// 目标实体。
    pub entity: Entity,
    /// 升级后的等级。
    pub new_level: u32,
    /// 本次发放的可分配点数（可能跨多级）。
    pub points_granted: u32,
}

/// 消费经验：升级 → 广播 [`LevelUpMessage`] → 发 [`GrantStatPointsMessage`]。
///
/// 放在 [`StatStage::Intake`]：同帧内先于 L0 的记账阶段，所以"打怪一帧内就看到可分配点数"。
pub fn apply_experience_gain(
    mut messages: MessageReader<GainExperienceMessage>,
    mut actors: Query<(&mut Level, Option<&Stat>)>,
    config: Res<LevelConfig>,
    mut level_ups: MessageWriter<LevelUpMessage>,
    mut grants: MessageWriter<GrantStatPointsMessage>,
) {
    for message in messages.read() {
        let Ok((mut level, stat)) = actors.get_mut(message.entity) else {
            continue;
        };

        let gained_levels = level.gain_exp(message.amount, &*config);
        if gained_levels == 0 {
            continue;
        }

        // 用 `LevelCurve` 的方法读配置（字段与方法同名，UFCS 避免歧义）。
        let points_granted = gained_levels * LevelCurve::points_per_level(&*config);

        if stat.is_some() {
            grants.write(GrantStatPointsMessage {
                entity: message.entity,
                amount: points_granted,
            });
        }

        level_ups.write(LevelUpMessage {
            entity: message.entity,
            new_level: level.current(),
            points_granted,
        });
    }
}

/// 注册成长模块（消息 + 系统），并把自己放进 L0 的 `Intake` 阶段。
pub struct ProgressionPlugin;

impl Plugin for ProgressionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LevelConfig>()
            .add_message::<GainExperienceMessage>()
            .add_message::<LevelUpMessage>()
            .add_systems(Update, apply_experience_gain.in_set(StatStage::Intake));
    }
}
