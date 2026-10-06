//! `EffectParams`：执行 `Effect` 所需的**一整套系统参数**（跨域只读查询 + 唯一写入口）。
//!
//! 它用 `#[derive(SystemParam)]` 打包，带来两个好处：
//!
//! 1. **一个参数顶十几个**：Bevy 的函数系统对参数个数有硬上限（`all_tuples!(.., 0, 16, F)`），
//!    而效果执行要看的组件很多；打包后在函数签名里只占一个位置。
//! 2. **顺序只写一次**：`Reads` 的字段顺序固定，避免"某个系统里把两个查询写反了"这类
//!    只会在运行时（注册系统时）才炸的隐式约束。
//!
//! 依赖方向：本模块依赖各领域（`actor` / `action` / `status` / `content` / `phase` / `contest`），
//! 由 [`combat`](crate::behaviors::combat) 装配层统一注册，**各领域的 Plugin 不注册它**。

use bevy_ecs::prelude::*;
use bevy_ecs::system::SystemParam;

use crate::atoms::actor::{ActorState, Cooldowns, Resources, Stats};
use crate::behaviors::action::{ActiveActions, InitiatedBy};
use crate::behaviors::content::{SkillCatalog, StatusCatalog};
use crate::behaviors::contest::CombatRng;
use crate::behaviors::effect::Blob;
use crate::behaviors::phase::CombatLog;
use crate::behaviors::skill::Skill;
use crate::behaviors::status::{ActiveStatus, StatusDef};
use crate::behaviors::threat::ThreatWindow;

/// 效果执行器要的只读数据 + 写入口。
///
/// `rng` / `threat` / `log` 是 `ResMut`，其余是只读查询。
#[derive(SystemParam)]
pub struct EffectParams<'w, 's> {
    /// 技能定义（`SpawnAction` 取时长）。
    pub skills: Query<'w, 's, &'static Skill>,
    /// 状态定义。
    pub status_defs: Query<'w, 's, &'static StatusDef>,
    /// 状态实例。
    pub statuses: Query<'w, 's, &'static ActiveStatus>,
    /// 状态 → 宿主（跑 `on_expire` / `on_remove` 时要宿主）。
    pub attached: Query<'w, 's, &'static crate::behaviors::status::AttachedTo>,
    /// 行动实例（`InitiatedBy` 是行动槽的反向关系）。
    pub initiated: Query<'w, 's, &'static InitiatedBy>,
    /// 行动槽。
    pub active_actions: Query<'w, 's, &'static ActiveActions>,
    /// 状态槽。
    pub actor_states: Query<'w, 's, &'static ActorState>,
    /// 施法者的特性标签（与阵营无关）。
    pub actor_tags: Query<'w, 's, &'static crate::atoms::actor::ActorTags>,
    /// 阵营（`TargetIsEnemy` 比较用；带 `Entity` 是为了直接喂给 `hostile_to`）。
    pub factions: Query<'w, 's, (Entity, &'static crate::atoms::actor::Faction)>,
    /// 技能目录（内容层没注入时可以缺席：相关效果自然什么都不做）。
    pub skill_catalog: Option<Res<'w, SkillCatalog>>,
    /// 状态目录（同上）。
    pub status_catalog: Option<Res<'w, StatusCatalog>>,
    /// 随机源（`RollUnder` 用）。
    pub rng: ResMut<'w, CombatRng>,
    /// 威胁窗口（未处理的威胁集合）。
    pub window: ResMut<'w, ThreatWindow>,
    /// 战斗日志。
    pub log: ResMut<'w, CombatLog>,
}

impl<'w, 's> EffectParams<'w, 's> {
    /// 造一份**只读视图**：全局查询从自己身上复制（只读 `Query` 是 `Copy`），
    /// 池与属性由调用方给（它们是**按实体**的数据，必须来自各系统自己的查询）。
    ///
    /// 借用检查要求"只读视图"与 `rng` / `threat` / `log`（可变）分开借，
    /// 所以调用方先把可变字段拆出去，再用这个方法重建只读那一半。
    /// 技能目录（没注入时为 None）。
    pub fn skills_catalog(&self) -> Option<&SkillCatalog> {
        self.skill_catalog.as_deref()
    }

    /// 状态目录（没注入时为 None）。
    pub fn statuses_catalog(&self) -> Option<&StatusCatalog> {
        self.status_catalog.as_deref()
    }

    pub fn reads_view<P, S>(&self, pools: P, stats: S) -> super::Reads<'w, 's, P, S> {
        super::Reads {
            resources: pools,
            stats_source: stats,
            stats_query: None,
            skills: Some(self.skills),
            status_defs: self.status_defs,
            active_statuses: self.statuses,
            initiated: Some(self.initiated),
            active_actions: Some(self.active_actions),
            actor_states: self.actor_states,
            cooldowns: None,
        }
    }
}

impl Blob for EffectParams<'_, '_> {
    fn pools(&self, _entity: Entity) -> Option<&Resources> {
        // 池是**按实体**的数据，放在各系统自己的查询里（否则会与同系统的
        // `Query<&mut Resources>` 撞车，触发 B0001）。
        None
    }
    fn stats(&self, _entity: Entity) -> Option<&Stats> {
        None
    }
    fn skills(&self) -> Option<Query<'_, '_, &'static Skill>> {
        Some(self.skills)
    }
    fn status_defs(&self) -> Query<'_, '_, &'static StatusDef> {
        self.status_defs
    }
    fn statuses(&self) -> Query<'_, '_, &'static ActiveStatus> {
        self.statuses
    }
    fn initiated(&self) -> Option<Query<'_, '_, &'static InitiatedBy>> {
        Some(self.initiated)
    }
    fn active_actions(&self) -> Option<Query<'_, '_, &'static ActiveActions>> {
        Some(self.active_actions)
    }
    fn actor_states(&self) -> Query<'_, '_, &'static ActorState> {
        self.actor_states
    }
    fn cooldowns(&self) -> Option<Query<'_, '_, &'static Cooldowns>> {
        // 冷却不放进共享参数包：各系统里已经有自己的 Resources / Cooldowns 查询，
        // 再加一份只读的会被 Bevy 判定为访问冲突（B0001）。
        None
    }
}
