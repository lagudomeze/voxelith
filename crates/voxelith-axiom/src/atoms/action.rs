//! **行动的原子**：一条正在执行的技能动作（L0，零依赖）。
//!
//! | 组件 | 是什么 |
//! |---|---|
//! | [`Action`] | 行动实例：阶段 / 当前阶段已过时长 / 阶段时长 / 目标 |
//! | [`CastsSkill`] / [`CastingSkill`] | 关系：行动 ↔ 它释放的技能定义 |
//! | [`InitiatedBy`] / [`ActiveActions`] | 关系：行动 ↔ 发起者（**这就是行动槽**） |
//! | [`ResolveNow`] / [`ReadyToResolve`] | 标记：瞬发 / 已到时长待结算 |
//! | [`Threat`] | 标记：这条行动**会威胁到 PC**（窗口要暂停的凭据） |
//! | [`Interrupts`] / [`SuperArmor`] | 交互语义：可打断他人 / 不可被打断（由 L1 裁决） |
//!
//! ## 三阶段
//!
//! ```text
//! WindUp   [t0, t0+W)       前摇：可被取消；移动会取消它
//! Resolve  t0+W             释放点：跑 Skill.effects；摘掉 Threat；**不是**实体销毁
//! Recovery [t0+W, t0+W+R)   后摇：技能已生效；可移动；被打断只是提前结束
//! despawn                   正常结束
//! ```
//!
//! 阶段状态**就是行动实体本身**（`Action.phase` + `elapsed`）：L2 通过 `InitiatedBy` /
//! `ActiveActions` 读"这个角色自己的动作阶段"，不需要另找地方存。
//!
//! ## 槽的形态
//!
//! `ActiveActions` 现在是 `Vec<Entity>`（关系反向集天生是集合）。
//! **是否收成单个 `Entity`** 取决于"瞬发占不占槽"这条未决问题
//! （[docs/OPEN-QUESTIONS.md](../../../../docs/OPEN-QUESTIONS.md) Q36）：
//! 一对一要求"瞬发不入槽"，否则放一个瞬发就会顶掉自己正在跑的长行动。
//!
//! 两条防线已经就位，与形态无关：
//!
//! 1. `ActiveActions` 带 `linked_spawn`：**发起者被 despawn 时，它名下的行动跟着销毁**。
//! 2. `InitiatedBy` 带 `on_discard` 释放钩子：**关系被摘掉（被顶替 / 被 remove）就销毁这条行动**。
//!    Bevy 的一对一"换源"只做**解绑**、不销毁（旧实体还在 tick、还会结算，而且什么都不报），
//!    所以"解绑即销毁"必须显式做。

use bevy_ecs::lifecycle::HookContext;
use bevy_ecs::prelude::*;
use bevy_ecs::world::DeferredWorld;

/// 行动当前处于哪个阶段。
///
/// `Resolve` 是**那一瞬**，不驻留：实体只在 `WindUp` / `Recovery` 里停留。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ActionPhase {
    /// 前摇：还没生效，可以被打断 / 被移动取消。
    #[default]
    WindUp,
    /// 后摇：效果已生效，只是还占着槽（不能再出手，但可以移动）。
    Recovery,
}

/// 一个正在进行的行动（组件，挂在**行动实体**上）。
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Action {
    /// 当前阶段。
    pub phase: ActionPhase,
    /// **当前阶段**已经过去的时长（秒）。
    pub elapsed: f32,
    /// **当前阶段**的时长（秒）；前摇 = `Skill.duration`，后摇 = `Skill.recovery`。
    pub duration: f32,
    /// 释放时指定的目标（`Targeting` 的第一步输入）。
    pub target: Option<Entity>,
}

impl Action {
    /// 造一条前摇中的行动。
    pub fn wind_up(duration: f32, target: Option<Entity>) -> Self {
        Self {
            phase: ActionPhase::WindUp,
            elapsed: 0.0,
            duration,
            target,
        }
    }

    /// 当前阶段是否已经走完。
    pub fn is_complete(&self) -> bool {
        self.elapsed >= self.duration
    }

    /// 当前阶段进度（0..=1；时长为 0 时恒为 1）。
    pub fn progress(&self) -> f32 {
        if self.duration <= 0.0 {
            1.0
        } else {
            (self.elapsed / self.duration).clamp(0.0, 1.0)
        }
    }

    /// 是不是"瞬发"（前摇为 0）：同帧结算，**不驻留**。
    pub fn is_instant(&self) -> bool {
        self.phase == ActionPhase::WindUp && self.duration <= 0.0
    }

    /// 切到后摇阶段（`duration` 是后摇时长）。
    pub fn enter_recovery(&mut self, duration: f32) {
        self.phase = ActionPhase::Recovery;
        self.elapsed = 0.0;
        self.duration = duration.max(0.0);
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

/// 关系：行动 → 发起它的角色（**这就是行动槽的关系源**）。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
#[relationship(relationship_target = ActiveActions)]
#[component(on_discard = release_on_unbind)]
pub struct InitiatedBy(pub Entity);

/// 关系反向集：角色 → 它正在进行的行动（**行动槽**）。
///
/// `linked_spawn`：发起者被 despawn 时，它名下的行动跟着销毁。
#[derive(Component, Debug, Clone, Default, PartialEq, Eq)]
#[relationship_target(relationship = InitiatedBy, linked_spawn)]
pub struct ActiveActions(Vec<Entity>);

impl ActiveActions {
    /// 当前行动列表。
    pub fn actions(&self) -> &[Entity] {
        &self.0
    }

    /// 槽是否为空（空 = 这个角色在等输入 / 可以再出手）。
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// 槽里有几个。
    pub fn len(&self) -> usize {
        self.0.len()
    }
}

/// **解绑即销毁**：`InitiatedBy` 被摘掉（被顶替 / 被 remove）时，这条行动不再属于任何槽。
///
/// 正常路径（行动自己 despawn）下这个钩子也会触发，但 `try_despawn` 对"正在销毁的实体"是
/// 静默 no-op，所以不会互相打架。命令是**延迟**的，也不会打断关系钩子自己的维护。
///
/// 两个边界：钩子**分不清"为什么"被销毁**（正常结算 / 被取消 / 被顶替都一样），
/// 依赖原因的副作用必须走显式的 `cancel()`；**同值重新 insert 也会触发它**。
fn release_on_unbind(mut world: DeferredWorld, ctx: HookContext) {
    world.commands().entity(ctx.entity).try_despawn();
}

/// 瞬发标记：本次推进里直接判定为"可结算"。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolveNow;

/// 已到时长、待结算。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadyToResolve;

/// 威胁标记：挂在行动实体上 = "这条行动**会威胁到 PC**"。
///
/// 它是"该不该开威胁窗口"的凭据；窗口自己是一份集合（见 `behaviors::threat`）。
/// `Resolve` 时被摘掉 —— 于是"窗口让给下一个"与"实体还在走后摇"并存。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Threat;

/// 交互语义：这条行动**可以打断**别人的行动。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Interrupts;

/// 交互语义：这条行动**不可被打断**（霸体）。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SuperArmor;

/// 行动槽里正在进行的行动（L2 只读）。
pub fn active_of(active: Option<&ActiveActions>) -> &[Entity] {
    active.map_or(&[], |active| active.actions())
}
