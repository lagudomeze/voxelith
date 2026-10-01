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
//! | [`actor`] | 角色三大数据类别：资源池 [`actor::Resources`] / 属性 [`actor::Stats`] / 状态标记 [`actor::ActorState`]，以及冷却、行动能量、阵营标记 |
//!
//! 详见 [docs/combat-design.md](../../../docs/combat-design.md)、[docs/layers.md](../../../docs/layers.md)。

pub mod actor;

pub use actor::{
    ActionEnergy, Actor, ActorPlugin, ActorRole, ActorState, ActorTag, ActorTags, Cooldowns,
    Faction, Monster, Player, Pool, Resources, StatModifier, Stats,
};
