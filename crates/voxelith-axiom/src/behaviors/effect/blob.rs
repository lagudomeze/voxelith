//! `Blob` 与 `Reads`：效果执行器要的**只读数据接口**。
//!
//! 抽出来是为了让 `mod.rs` 只讲 `Effect` 的语义（单文件 < 500 行，**R26**）。

use bevy_ecs::prelude::*;

use crate::atoms::actor::{ActorState, Cooldowns, Resources, Stats};
use crate::behaviors::action::{ActiveActions, InitiatedBy};
use crate::behaviors::skill::Skill;
use crate::behaviors::status::{ActiveStatus, StatusDef};
use crate::behaviors::value::Provider;

/// 只读查询集合：执行效果需要看的全部数据（**R8**：只用各领域自己的组件）。
///
/// 池与属性走 [`Provider`]（数据从哪来由实现决定：世界查询或调用方手里已有的值），
/// 其余是**只读查询**（按值返回：`Query` 对数据参数不变，用引用会把生命周期绑死）。
/// 调用方没提供的查询是 `None`：相关效果自然什么都不做，也不会 panic。
///
/// [`Provider`]: crate::behaviors::value::Provider
pub trait Blob {
    /// 取某个实体的池。
    fn pools(&self, entity: Entity) -> Option<&Resources>;
    /// 取某个实体的属性。
    fn stats(&self, entity: Entity) -> Option<&Stats>;
    /// 技能定义（`SpawnAction` 要）。
    fn skills(&self) -> Option<Query<'_, '_, &'static Skill>>;
    /// 状态定义。
    fn status_defs(&self) -> Query<'_, '_, &'static StatusDef>;
    /// 状态实例（**每个系统里只能有一份碰 `ActiveStatus` 的查询**，否则 Bevy 判定访问冲突）。
    fn statuses(&self) -> Query<'_, '_, &'static ActiveStatus>;
    /// 行动实例（只读，`InitiatedBy` 是行动槽的反向关系）。
    fn initiated(&self) -> Option<Query<'_, '_, &'static InitiatedBy>>;
    /// 行动槽（`InitiatedBy` 的反向集）。
    fn active_actions(&self) -> Option<Query<'_, '_, &'static ActiveActions>>;
    /// 状态槽（`AttachedTo` 的反向集）。
    fn actor_states(&self) -> Query<'_, '_, &'static ActorState>;
    /// 冷却查询（`Effect` 里没人写冷却，这里只给内容层查询用）。
    fn cooldowns(&self) -> Option<Query<'_, '_, &'static Cooldowns>>;
}

/// `Blob` 的实现体：把调用方的数据来源与只读查询收集到一处。
///
/// `R` / `S` 分别是池与属性的来源：`Query<&Resources>` 或 `&Resources` 都行。
/// 没提供的只读查询留 `None`（见 [`Blob`] 的说明）。
pub struct Reads<'w, 's, R, S> {
    /// 池的来源。
    pub resources: R,
    /// 属性的来源。
    pub stats_source: S,
    /// 属性查询（`stats_source` 覆盖不到时才用）。
    pub stats_query: Option<Query<'w, 's, &'static Stats>>,
    /// 技能定义。
    pub skills: Option<Query<'w, 's, &'static Skill>>,
    /// 状态定义。
    pub status_defs: Query<'w, 's, &'static StatusDef>,
    /// 状态实例。
    pub active_statuses: Query<'w, 's, &'static ActiveStatus>,
    /// 行动实例。
    pub initiated: Option<Query<'w, 's, &'static InitiatedBy>>,
    /// 行动槽。
    pub active_actions: Option<Query<'w, 's, &'static ActiveActions>>,
    /// 状态槽。
    pub actor_states: Query<'w, 's, &'static ActorState>,
    /// 冷却。
    pub cooldowns: Option<Query<'w, 's, &'static Cooldowns>>,
}

impl<R, S> Blob for Reads<'_, '_, R, S>
where
    R: Provider<Resources>,
    S: Provider<Stats>,
{
    fn pools(&self, entity: Entity) -> Option<&Resources> {
        self.resources.provide(entity)
    }
    fn stats(&self, entity: Entity) -> Option<&Stats> {
        self.stats_source
            .provide(entity)
            .or_else(|| self.stats_query.as_ref().and_then(|q| q.get(entity).ok()))
    }
    fn skills(&self) -> Option<Query<'_, '_, &'static Skill>> {
        self.skills.as_ref().copied()
    }
    fn status_defs(&self) -> Query<'_, '_, &'static StatusDef> {
        self.status_defs
    }
    fn statuses(&self) -> Query<'_, '_, &'static ActiveStatus> {
        self.active_statuses
    }
    fn initiated(&self) -> Option<Query<'_, '_, &'static InitiatedBy>> {
        self.initiated.as_ref().copied()
    }
    fn active_actions(&self) -> Option<Query<'_, '_, &'static ActiveActions>> {
        self.active_actions.as_ref().copied()
    }
    fn actor_states(&self) -> Query<'_, '_, &'static ActorState> {
        self.actor_states
    }
    fn cooldowns(&self) -> Option<Query<'_, '_, &'static Cooldowns>> {
        self.cooldowns.as_ref().copied()
    }
}

impl<'w, 's, R, S> Reads<'w, 's, R, S> {
    /// 只给数据来源（池 / 属性）与**必备**的三份查询；其余只读查询按需补。
    pub fn new(
        resources: R,
        stats_source: S,
        status_defs: Query<'w, 's, &'static StatusDef>,
        active_statuses: Query<'w, 's, &'static ActiveStatus>,
        actor_states: Query<'w, 's, &'static ActorState>,
    ) -> Self {
        Self {
            resources,
            stats_source,
            stats_query: None,
            skills: None,
            status_defs,
            active_statuses,
            initiated: None,
            active_actions: None,
            actor_states,
            cooldowns: None,
        }
    }

    /// 同 [`Reads::new`]，但**另给**一份状态实例查询（调用方自己那份带过滤 / 复合数据，
    /// 一个系统里只能有一份碰 `ActiveStatus` 的查询，否则 Bevy 判定访问冲突）。
    pub fn new_with_statuses(
        resources: R,
        stats_source: S,
        status_defs: Query<'w, 's, &'static StatusDef>,
        active_statuses: Query<'w, 's, &'static ActiveStatus>,
        actor_states: Query<'w, 's, &'static ActorState>,
    ) -> Self {
        Self::new(
            resources,
            stats_source,
            status_defs,
            active_statuses,
            actor_states,
        )
    }

    /// 补上属性查询（`stats_source` 是 `&Resources` 之外的场景用）。
    pub fn with_stats_query(mut self, stats: Query<'w, 's, &'static Stats>) -> Self {
        self.stats_query = Some(stats);
        self
    }

    /// 补上技能定义查询。
    pub fn with_skills(mut self, skills: Query<'w, 's, &'static Skill>) -> Self {
        self.skills = Some(skills);
        self
    }

    /// 补上行动实例查询。
    pub fn with_initiated(mut self, initiated: Query<'w, 's, &'static InitiatedBy>) -> Self {
        self.initiated = Some(initiated);
        self
    }

    /// 补上行动槽查询。
    pub fn with_active_actions(mut self, actions: Query<'w, 's, &'static ActiveActions>) -> Self {
        self.active_actions = Some(actions);
        self
    }

    /// 补上冷却查询。
    pub fn with_cooldowns(mut self, cooldowns: Query<'w, 's, &'static Cooldowns>) -> Self {
        self.cooldowns = Some(cooldowns);
        self
    }
}
