//! L0 原子层（atoms）。
//!
//! 规则摘要：
//! - **R8**  组件只依赖自己，系统只查询自己，不跨组件查询。
//! - **R9**  可以发出事件，事件只含数据，不含渲染句柄。
//! - **R10** 禁止 `Sprite` / `Text` / `Mesh` / `Transform` / `Handle<Image>`。
//! - **R103** 数据组件不存 `Handle<Image>`。
//!
//! 详见 `docs/layers.md`。

pub mod attribute;

pub mod health;

pub use attribute::{
    AttributeAllocationMessage, AttributeBaseChangedMessage, AttributeFinalMessage,
    AttributeGrowthMessage, AttributeId, AttributePlugin, AttributeRespecMessage, AttributeStage,
    AttributeValues, Attributes,
};
pub use health::{Health, HealthPlugin, ModifyHealthMessage, apply_health_change};
