//! L0 原子层（atoms）。
//!
//! 规则摘要：
//! - **R8**  组件只依赖自己，系统只查询自己，不跨组件查询。
//! - **R9**  可以发出事件，事件只含数据，不含渲染句柄。
//! - **R10** 禁止 `Sprite` / `Text` / `Mesh` / `Transform` / `Handle<Image>`。
//! - **R103** 数据组件不存 `Handle<Image>`。
//!
//! 领域：
//!
//! | 模块 | 内容 |
//! |---|---|
//! | [`stats`] | 属性（`Stat` / `StatBlock` / `StatId` / `StatModifiers`）与等级（`Level` / `LevelConfig`） |
//! | [`modifiers`] | 修饰符数据 + 聚合公式（纯数据 + 纯算法，无组件；供 stats 与 resistance 共用） |
//! | [`health`] | 血量（唯一写入口 + `ModifyHealthMessage`） |
//!
//! 详见 `docs/layers.md`、`docs/combat-mechanics.md`。

pub mod health;
pub mod modifiers;
pub mod stats;

pub use health::{Health, HealthPlugin, ModifyHealthMessage, apply_health_change};
pub use modifiers::{
    Modifier, ModifierCaps, ModifierOp, ModifierSet, ModifierSource, Rounding, evaluate,
};
pub use stats::{
    AddStatModifierMessage, AllocateStatRequest, DEFAULT_STAT, GrantStatPointsMessage, Level,
    LevelConfig, LevelCurve, RemoveStatModifiersMessage, RespecStatsMessage, Stat,
    StatAllocatedMessage, StatAllocationFailedMessage, StatBlock, StatConfig, StatError, StatId,
    StatModifiers, StatPlugin, StatStage,
};
