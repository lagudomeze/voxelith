//! **决策槽的原子上半**：怪物"已经下定决心、还没出手"的那条决策挂什么。
//!
//! 决定与执行的**系统**在 [`behaviors::monster`](crate::behaviors::monster)（L1）。
//!
//! ```text
//! 决策实体 ──DecidedBy──► 怪物      关系目标：DecisionSlot（**单个 Entity**，不是 Vec）
//! 决策实体 ──SettledBy──► 行动      关系目标：Settles（linked_spawn）
//! ```
//!
//! 一对一的两个坑（都不报错，只是沉默）：
//!
//! 1. **槽的"空"不是"长度为 0"，是没有 `DecisionSlot` 组件。** 0.19 的
//!    `RelationshipTarget` 不支持 `Option<Entity>`，所以"没决定"只能表达为拆掉关系。
//!    拆掉之后组件本身的清理是**排队命令**——同一帧里槽可能还在、只是空的。
//!    所以读槽一律走 [`decision_of`]（`iter()`），不直接读字段。
//! 2. **"换一条决策"不会销毁旧决策实体。** 一对一关系在新源顶掉旧源时只做**解绑**。
//!    所以 `monster_decide` 只在槽空时写新决策，从根上不给它顶掉的机会。

use bevy_ecs::prelude::*;

use crate::atoms::vocabulary::SkillId;

/// 一条 AI 决策（组件，挂在**决策实体**上）。
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct AiDecision {
    /// 想放哪个技能——**词汇 ID**：读日志 / 表现层不查目录也能说出"它要放什么"。
    pub skill: SkillId,
    /// 决定那一刻解析好的技能实体。
    ///
    /// 与 `skill` 由 `monster_decide` 的**同一次查表**写入，不会各说各话；
    /// 存下来是为了"决策就是一次解析的结果"——执行时不该再查一遍目录。
    pub skill_entity: Entity,
    /// 选中时的权重。
    pub weight: f32,
    /// 决定那一刻锁定的目标（**不会**因为场上目标换人而改主意）。
    pub target: Option<Entity>,
}

/// 关系：决策 → 它属于哪个怪物（一对一的关系源，**唯一真相在这一侧**）。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
#[relationship(relationship_target = DecisionSlot)]
pub struct DecidedBy(pub Entity);

/// 关系反向集（**一对一**）：怪物 → 它当前那条决策。
///
/// 字段是**单个 `Entity`** 而不是 `Vec<Entity>`：一个怪物同时只能有一条决策，
/// "第二条"在语义上根本不成立（`Vec` 会让人以为可以排队）。
///
/// `linked_spawn`：怪物被 despawn 时决策跟着销毁——决策不能脱离它的怪物独立存在。
///
/// ## 为什么是 `SparseSet`
///
/// 槽**每个决策周期插一次、摘一次**，而怪物身上有十来个组件。用默认的 `Table`
/// 存储，每插一次都要把这一整行**搬**到另一个 table；`SparseSet` 组件**不进 table**，
/// 插删只动稀疏集。判据见 [docs/bevy-queries.md](../../../../docs/bevy-queries.md)。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
#[component(storage = "SparseSet")]
#[relationship_target(relationship = DecidedBy, linked_spawn)]
pub struct DecisionSlot(Entity);

/// 怪物的当前决策实体：没槽、或槽正处在"已拆掉但组件还没被清理"的那一帧 → `None`。
///
/// 对标 [`action::active_of`](crate::atoms::action::active_of)——**L2 只读**，
/// 不需要知道关系是怎么维护的。
pub fn decision_of(slot: Option<&DecisionSlot>) -> Option<Entity> {
    slot.and_then(|slot| slot.iter().next())
}

/// 关系：决策 → 它落成的行动（一对一）。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
#[relationship(relationship_target = Settles)]
pub struct SettledBy(pub Entity);

/// 关系反向集（**一对一**）：行动 → 它结算的那条决策。
///
/// `linked_spawn`：**行动一 despawn，决策跟着销毁**。这是决策槽自动清空的主路径
/// （正常结算 / 被反制取消 / 怪物没了）。
///
/// 这个组件不由任何系统插入：它由 [`SettledBy`] 的钩子在行动实体上自动建出来。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
#[relationship_target(relationship = SettledBy, linked_spawn)]
pub struct Settles(Entity);
