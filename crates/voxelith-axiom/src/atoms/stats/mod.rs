//! L0 属性 / 等级领域：**组件自成一体**。
//!
//! | 文件 | 内容 |
//! |---|---|
//! | [`stat`] | `StatId` / `StatBlock` / `StatModifiers` / `Stat`（自成一体：账本 + 修饰符 + 最终值视图） |
//! | [`level`] | `Level` 组件 + `LevelConfig`（经验曲线 Resource）+ `LevelCurve` |
//! | [`systems`] | 变更消息与唯一写路径（改完自己 `refresh`） |
//!
//! 设计要点：
//!
//! - `base` 是**初始值**，运行期不变；`allocated` 是分配的点数（升级成长发下来的点数也在里面）；
//!   最终值 `cached` = `(base + allocated)` 经修饰符聚合后取整，由 `Stat::refresh` 自己刷新。
//! - 因此**没有**"基础值变了 / 最终值算好了"这类来回消息，也没有脏标记：
//!   公式需要的信息全在这个组件内部，所以它属于 L0（**R8**：系统只查询自己）。
//! - L1 只做跨领域编排：成长链 [`crate::behaviors::progression`] 消费经验 → 升级 → 发点数消息。
//!
//! [`StatStage`] 是给 L1 留的两段插槽，保证"成长入口"排在"属性变更"之前（同帧生效）。

mod level;
mod stat;
mod systems;

pub use level::{Level, LevelConfig, LevelCurve};
pub use stat::{DEFAULT_STAT, Stat, StatBlock, StatError, StatId, StatModifiers};
pub use systems::{
    AddStatModifierMessage, AllocateStatRequest, GrantStatPointsMessage,
    RemoveStatModifiersMessage, RespecStatsMessage, StatAllocatedMessage,
    StatAllocationFailedMessage, apply_stat_allocation, apply_stat_modifier_add,
    apply_stat_modifier_remove, apply_stat_point_grant, apply_stat_respec,
    tick_stat_modifier_lifetimes,
};

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;

use crate::atoms::modifiers::{ModifierCaps, Rounding};

/// 属性模块配置（**Resource**：内容层 / 文件加载后注入）。
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StatConfig {
    /// 取整口径（见 [`Rounding`]）。
    pub rounding: Rounding,
}

/// 属性变更的阶段契约。
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StatStage {
    /// L1：成长入口（经验 → 升级 → 发点数）。排在最前，保证同帧内后续阶段能看到新点数。
    Intake,
    /// L0：属性变更（加点 / 发点数 / 洗点 / 修饰符增删与计时）。
    Apply,
}

/// 注册属性的数据、消息、配置与唯一写路径（**R34**：谁定义谁注册）。
pub struct StatPlugin;

impl Plugin for StatPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<StatConfig>()
            .init_resource::<ModifierCaps>()
            .add_message::<AllocateStatRequest>()
            .add_message::<GrantStatPointsMessage>()
            .add_message::<RespecStatsMessage>()
            .add_message::<AddStatModifierMessage>()
            .add_message::<RemoveStatModifiersMessage>()
            .add_message::<StatAllocatedMessage>()
            .add_message::<StatAllocationFailedMessage>()
            .configure_sets(Update, (StatStage::Intake, StatStage::Apply).chain())
            .add_systems(
                Update,
                (
                    tick_stat_modifier_lifetimes,
                    apply_stat_modifier_add,
                    apply_stat_modifier_remove,
                    apply_stat_point_grant,
                    apply_stat_allocation,
                    apply_stat_respec,
                )
                    .chain()
                    .in_set(StatStage::Apply),
            );
    }
}
