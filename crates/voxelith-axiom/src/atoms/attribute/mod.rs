//! L0 属性：**只存数值，不算公式**。
//!
//! - 唯一定义点在 [`defs`]（`voxelith_defs!` 生成 [`AttributeId`]，新增属性只改一行）。
//! - 唯一写入口在 [`systems`]：基础值用消息改，最终值收 L1 的 [`AttributeFinalMessage`]。
//! - 公式（基础值 + 修饰符 → 最终值）在 L1 [`crate::behaviors::attributes`]（**R11**：
//!   需要多个组件同时在场，所以不放 L0）。
//!
//! [`AttributeStage`] 是 L0 给 L1 留的**固定插槽**，把三方串成一帧内的确定顺序：
//!
//! ```text
//! ChangeBase（L0 改基础值） → RecomputeFinal（L1 算公式） → StoreFinal（L0 存最终值）
//! ```

pub mod defs;
pub mod systems;

pub use defs::{AttributeId, AttributeValues};
pub use systems::{
    AttributeAllocationMessage, AttributeBaseChangedMessage, AttributeFinalMessage,
    AttributeGrowthMessage, AttributeRespecMessage, Attributes, apply_attribute_allocation,
    apply_attribute_final, apply_attribute_growth, apply_attribute_respec,
};

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;

/// 属性变更的固定阶段（跨模块的排序契约，不允许插队）。
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AttributeStage {
    /// L0：基础值变更（加点 / 成长 / 洗点）。
    ChangeBase,
    /// L1：公式（基础值 + 修饰符 → 最终值）。L0 不在此阶段放系统。
    RecomputeFinal,
    /// L0：把 L1 算好的最终值写进缓存。
    StoreFinal,
}

/// 注册属性的原子数据、消息与唯一写入口（**R34**：谁定义谁注册）。
pub struct AttributePlugin;

impl Plugin for AttributePlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<AttributeAllocationMessage>()
            .add_message::<AttributeGrowthMessage>()
            .add_message::<AttributeRespecMessage>()
            .add_message::<AttributeBaseChangedMessage>()
            .add_message::<AttributeFinalMessage>()
            .configure_sets(
                Update,
                (
                    AttributeStage::ChangeBase,
                    AttributeStage::RecomputeFinal,
                    AttributeStage::StoreFinal,
                )
                    .chain(),
            )
            .add_systems(
                Update,
                (
                    apply_attribute_allocation,
                    apply_attribute_growth,
                    apply_attribute_respec,
                )
                    .chain()
                    .in_set(AttributeStage::ChangeBase),
            )
            .add_systems(
                Update,
                apply_attribute_final.in_set(AttributeStage::StoreFinal),
            );
    }
}
