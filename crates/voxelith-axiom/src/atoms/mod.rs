//! L0 原子层（atoms）：**"一个实体身上能挂什么"的完整清单**。
//!
//! 规则摘要：
//! - **R8**  组件只依赖自己，系统只查询自己，不跨组件查询。
//! - **R9**  可以发出事件，事件只含数据，不含渲染句柄。
//! - **R10** 禁止 `Sprite` / `Text` / `Mesh` / `Transform` / `Handle<Image>`。
//! - **R103** 数据组件不存 `Handle<Image>`。
//!
//! ## 进 L0 的判据：**它认不认识别的东西**
//!
//! | 认不认识 | 放哪 | 例 |
//! |---|---|---|
//! | 谁都不引用（只有 `Entity` / 词汇 ID / 自己的姊妹类型） | **L0** | `Action` / `ActiveStatus` / `DecisionSlot` / `Faction` |
//! | 引用**公式簇**（`Requirement` / `Condition` / `Effect` / `Value`） | L1（跟着公式走） | `Skill` / `StatusDef` / `MonsterDef` |
//! | 要**世界**才算得出来（多组件 / 资源 / `Time`） | L1（系统） | `tick_actions` / `monster_decide` / `apply_status_modifiers` |
//!
//! 这条判据的好处是**它会被编译器执行**：把 L0 的组件加一个引用 L1 的字段，
//! 立刻多出一条 `atoms → behaviors` 的边——看得见，而不是悄悄长在 L1 里没人管。
//!
//! 公式按 **R54 / R59**（"L1 可以在不知道具体层的情况下做判定、公式、筛选"）属于 L1，
//! 所以"定义组件"跟着公式留在 `behaviors`，L0 只管**实例**。
//!
//! ## 领域
//!
//! | 模块 | 内容 |
//! |---|---|
//! | [`actor`] | 角色数值（池 / 属性 / 冷却 / 状态槽 / 能量）+ 三条正交轴（引擎角色 / 阵营 / 特性） |
//! | [`vocabulary`] | 词汇原子：五种 `u16` 词汇 ID、`NameTable`、`UnknownName` |
//! | [`action`] | 行动实例与它的关系（行动槽）、瞬发 / 待结算 / 威胁三个标记 |
//! | [`status`] | 状态实例与它的关系（状态槽） |
//! | [`decision`] | 决策槽：怪物"已经决定、还没出手"的那条决策 |
//!
//! 详见 [docs/combat-design.md](../../../docs/combat-design.md)、
//! [docs/layers.md](../../../docs/layers.md)、[docs/bevy-queries.md](../../../docs/bevy-queries.md)。

pub mod action;
pub mod actor;
pub mod decision;
pub mod status;
pub mod vocabulary;

pub use action::{
    Action, ActiveActions, CastingSkill, CastsSkill, InitiatedBy, ReadyToResolve, ResolveNow,
    Threat, active_of,
};
pub use actor::{
    ActionEnergy, Actor, ActorPlugin, ActorRole, ActorState, ActorTags, AiDriven, Cooldowns,
    Faction, InputDriven, Monster, Player, Pool, Resources, StatModifier, Stats, actor_role,
};
pub use decision::{AiDecision, DecidedBy, DecisionSlot, SettledBy, Settles, decision_of};
pub use status::{ActiveStatus, AttachedTo, Statuses};
pub use vocabulary::{ActorTagId, NameTable, ResourceId, SkillId, StatId, StatusId, UnknownName};
