//! **行动的原子上半**：一个行动实体身上挂什么。
//!
//! 这里只有组件与关系，**一行系统都没有**——推进与结算在
//! [`behaviors::action`](crate::behaviors::action)（L1，那才需要世界）。
//!
//! | 组件 | 是什么 |
//! |---|---|
//! | [`Action`] | 行动实例本身：进度 / 时长 / 目标 |
//! | [`CastsSkill`] / [`CastingSkill`] | 关系：行动 ↔ 它释放的技能定义 |
//! | [`InitiatedBy`] / [`ActiveActions`] | 关系：行动 ↔ 发起它的角色（**这就是行动槽**） |
//! | [`ResolveNow`] / [`ReadyToResolve`] | 标记：瞬发 / 已到时长 |
//! | [`Threat`] | 标记：这个行动**即将生效**（反制窗口的目标） |
//!
//! ## 为什么它们在 L0
//!
//! 判据是**"它认不认识别的东西"**：上面每一个要么不引用任何类型，要么只引用
//! `Entity` / 自己的姊妹类型。把它们放在 L0 之后，**"给行动加一个引用 L1 的字段"
//! 会立刻变成一条看得见的越层边**——而不是悄悄长在 L1 里没人管。
//!
//! 对照：`Skill` / `StatusDef` 这些**定义**组件引用公式簇（`Requirement` / `Effect`），
//! 所以它们留在 L1 和公式一起（R54 / R59）。
//!
//! ## 槽位只有"有没有"
//!
//! `ActiveActions.len() <= 1` 是唯一约束，不区分种类（没有 `SlotKind`）。
//! **`duration == 0` 即瞬发**：同帧结算并 despawn，**从不真正占槽**。

use bevy_ecs::prelude::*;

/// 一个正在进行的行动（组件，挂在**行动实体**上）。
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Action {
    /// 已经过去的时长（秒）。
    pub elapsed: f32,
    /// 总时长（秒）；`0.0` = 瞬发。
    pub duration: f32,
    /// 释放时指定的目标（`Targeting` 的第一步输入）。
    pub target: Option<Entity>,
}

impl Action {
    /// 是否已经到时长。
    pub fn is_complete(&self) -> bool {
        self.elapsed >= self.duration
    }

    /// 进度（0..=1；瞬发恒为 1）。
    pub fn progress(&self) -> f32 {
        if self.duration <= 0.0 {
            1.0
        } else {
            (self.elapsed / self.duration).clamp(0.0, 1.0)
        }
    }
}

/// 关系：行动 → 它释放的技能定义。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
#[relationship(relationship_target = CastingSkill)]
pub struct CastsSkill(pub Entity);

/// 关系反向集：技能定义 → 正在释放它的行动。
#[derive(Component, Debug, Clone, Default, PartialEq, Eq)]
#[relationship_target(relationship = CastsSkill)]
pub struct CastingSkill(Vec<Entity>);

/// 关系：行动 → 发起它的角色（**这就是行动槽**）。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
#[relationship(relationship_target = ActiveActions)]
pub struct InitiatedBy(pub Entity);

/// 关系反向集：角色 → 它正在进行的行动（**槽位约束的载体**）。
#[derive(Component, Debug, Clone, Default, PartialEq, Eq)]
#[relationship_target(relationship = InitiatedBy)]
pub struct ActiveActions(Vec<Entity>);

impl ActiveActions {
    /// 当前行动列表。
    pub fn actions(&self) -> &[Entity] {
        &self.0
    }

    /// 槽是否为空（空 = 这个角色在等输入）。
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// 槽里有几个行动。
    pub fn len(&self) -> usize {
        self.0.len()
    }
}

/// 瞬发标记：本次推进里直接判定为"可结算"。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolveNow;

/// 已到时长、待结算。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadyToResolve;

/// 威胁标记：挂在这个行动实体上 = "它即将生效"。
///
/// 它标的是**行动**（不是怪物）：反制窗口要取消的就是这条行动。
/// 挂在 `monster` 域只是因为它由 `monster_act` 登记——组件本身零依赖，属于这里。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Threat;

/// 行动槽里正在进行的行动（L2 只读）。
pub fn active_of(active: Option<&ActiveActions>) -> &[Entity] {
    active.map_or(&[], |active| active.actions())
}
