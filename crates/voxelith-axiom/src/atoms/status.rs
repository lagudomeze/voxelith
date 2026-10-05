//! **状态的原子上半**：一个状态实例身上挂什么（定义在
//! [`behaviors::status`](crate::behaviors::status)，它引用公式簇）。
//!
//! | 组件 | 是什么 |
//! |---|---|
//! | [`ActiveStatus`] | 状态实例：定义实体 / 剩余时间 / 层数 / 施加者 |
//! | [`AttachedTo`] / [`Statuses`] | 关系：实例 ↔ 宿主（**这就是状态槽**） |
//!
//! 生命周期（施加 / 倒计时 / 到期 / 净化 / 派生修饰符）全在 L1——那些要读
//! `Time`、要同时看几个组件。
//!
//! ## 为什么"定义"不在这里
//!
//! [`StatusDef`](crate::behaviors::status::StatusDef) 引用 `Effect` / `Value` /
//! `SkillTags`，也就是**公式簇**。公式按 R54 / R59 属于 L1，所以定义跟着公式走。
//! 实例不引用任何别的东西，所以它是原子。

use bevy_ecs::prelude::*;

use crate::atoms::vocabulary::StatusId;

/// 状态实例（组件，挂在自己的实体上）。
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct ActiveStatus {
    /// 指向 [`StatusDef`](crate::behaviors::status::StatusDef) 实体。
    pub def: Entity,
    /// 哪种状态（离了定义也能答"这是什么"；`def` 被销毁时仍可用）。
    pub id: Option<StatusId>,
    /// 剩余时间（秒）。
    pub remaining: f32,
    /// 当前层数。
    pub stacks: u8,
    /// 施加者（效果的 `Caster`）。
    pub source: Entity,
    /// 距离下一个结算周期的累计时间。
    pub tick_accumulator: f32,
}

/// 关系：状态实例 → 宿主。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
#[relationship(relationship_target = Statuses)]
pub struct AttachedTo(pub Entity);

/// 关系反向集：宿主 → 身上的状态实例（由 Bevy 自动维护）。
///
/// 它是 [`AttachedTo`] 的反向集，所以"清空状态"就是对槽做 `retain`，
/// 不需要遍历世界。
#[derive(Component, Debug, Clone, Default, PartialEq, Eq)]
#[relationship_target(relationship = AttachedTo)]
pub struct Statuses(Vec<Entity>);

impl Statuses {
    /// 身上的状态实例。
    pub fn statuses(&self) -> &[Entity] {
        &self.0
    }

    /// 有几个。
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// 空？
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}
