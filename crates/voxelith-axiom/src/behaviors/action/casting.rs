//! 释放技能（Casting）：`CastRequest` → 校验 → 扣费 → 生成 [`Action`](super::Action)。
//!
//! 这里把"释放技能需要的系统参数"打包成一个 `#[derive(SystemParam)]`（[`CastParams`]），
//! 原因和 [`EffectParams`](crate::behaviors::effect::EffectParams) 一样：
//! **Bevy 的函数系统对参数个数有硬上限**，而校验一条释放请求要看的组件很多。
//!
//! 两个 `Query<&Resources>` 用 `With<Player>` / `Without<Player>` 分成互斥的两半：
//! 这样才能在同一个系统里既**改**施法者的池，又**读**别人的池。

use bevy_ecs::prelude::*;
use bevy_ecs::system::SystemParam;

use crate::atoms::actor::{
    ActorRole, ActorState, ActorTags, Cooldowns, Faction, Player, Resources, Stats,
};
use crate::behaviors::action::{ActiveActions, CastRequest, CastsSkill, InitiatedBy, ResolveNow};
use crate::behaviors::contest::CombatRng;
use crate::behaviors::effect::Reads;
use crate::behaviors::phase::PendingThreat;
use crate::behaviors::requirement::{CasterContext, Cost, skill_available, status_snapshot};
use crate::behaviors::skill::Skill;
use crate::behaviors::status::{ActiveStatus, StatusDef};
use crate::behaviors::targeting::{TargetingContext, faction_of, hostile_to, resolve_target};

/// 释放技能需要的全部参数。
#[derive(SystemParam)]
pub struct CastParams<'w, 's> {
    /// 可写的池与冷却（只对 `Player` 生效）。
    pub actors: Query<'w, 's, (&'static mut Resources, &'static mut Cooldowns), With<Player>>,
    /// 只读的池（非玩家，用来避免与上面冲突）。
    pub others: Query<'w, 's, &'static Resources, Without<Player>>,
    /// 施法者的状态槽。
    pub actor_states: Query<'w, 's, &'static ActorState>,
    /// 施法者的特性标签。
    pub actor_tags: Query<'w, 's, &'static ActorTags>,
    /// 施法者的阵营（带 Entity，直接喂给 hostile_to）。
    pub factions: Query<'w, 's, (Entity, &'static Faction)>,
    /// 行动槽。
    pub active_actions: Query<'w, 's, &'static ActiveActions>,
    /// 技能定义。
    pub skills: Query<'w, 's, &'static Skill>,
    /// 属性。
    pub stats: Query<'w, 's, &'static Stats>,
    /// 状态实例。
    pub statuses: Query<'w, 's, &'static ActiveStatus>,
    /// 状态定义。
    pub status_defs: Query<'w, 's, &'static StatusDef>,
    /// 行动实例。
    pub initiated: Query<'w, 's, &'static InitiatedBy>,
    /// 挂起威胁。
    pub threat: Res<'w, PendingThreat>,
    /// 随机源（校验本身不用，但保持与效果执行一致的资源形状）。
    pub rng: ResMut<'w, CombatRng>,
}

impl<'w, 's> CastParams<'w, 's> {
    /// 造一份只读视图（供 [`skill_available`] 用；只读 `Query` 是 `Copy`）。
    ///
    /// `resources` 用 `others`（`Without<Player>`）：施法者自己的池由调用方从 `actors`
    /// 那份**可变**查询里取，避免同一个系统里出现两份碰 `Resources` 的查询。
    pub fn reads_view(
        &self,
    ) -> Reads<
        'w,
        's,
        Query<'w, 's, &'static Resources, Without<Player>>,
        Query<'w, 's, &'static Stats>,
    > {
        Reads {
            resources: self.others,
            stats_source: self.stats,
            stats_query: Some(self.stats),
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

/// 施法校验：读 `CastRequest`，满足条件就**立刻**扣费并生成行动实例。
pub fn cast_requests(
    mut requests: MessageReader<CastRequest>,
    mut commands: Commands,
    mut params: CastParams,
) {
    for request in requests.read() {
        if !try_cast(request, &mut commands, &mut params) {
            continue;
        }
    }
}

/// 试一次释放；成功返回 `true`。
fn try_cast(request: &CastRequest, commands: &mut Commands, params: &mut CastParams) -> bool {
    let Ok(skill) = params.skills.get(request.skill) else {
        return false;
    };

    let slot_count = params
        .active_actions
        .get(request.caster)
        .map_or(0, |active| active.len());
    if skill.duration > 0.0 && slot_count > 0 {
        return false;
    }

    let status_list = params
        .actor_states
        .get(request.caster)
        .map(|state| state.statuses.clone())
        .unwrap_or_default();
    let statuses = status_snapshot(&status_list, &params.statuses, &params.status_defs);
    let reads = params.reads_view();
    // 池与冷却一起取（可变查询只能借一次）。
    let Ok(actor_read) = params.actors.get(request.caster) else {
        return false;
    };
    let (caster_pools, actor_cooldowns): (&Resources, &Cooldowns) = (actor_read.0, actor_read.1);

    // **先解析目标再判定**：`TargetIsEnemy` 之类的要求看的是"这一招打谁"，
    // 所以必须把目标（以及它的阵营）喂进上下文，否则这类技能永远判为不可用。
    // 目标解析归 [`resolve_target`]，"谁能当敌人"归 [`hostile_to`]——两份语义各只有一处。
    let targeting = TargetingContext {
        explicit: request.target,
        threat: Some(*params.threat),
    };
    let target = resolve_target(
        skill.targeting,
        request.caster,
        &targeting,
        hostile_to(&params.factions, request.caster),
    );
    let target_resources = target.and_then(|entity| params.others.get(entity).ok());
    let target_faction = faction_of(&params.factions, target);

    let context = CasterContext {
        actor: request.caster,
        resources: caster_pools,
        stats: params.stats.get(request.caster).ok(),
        cooldowns: actor_cooldowns,
        tags: params.actor_tags.get(request.caster).ok(),
        // cast_requests 的 ctors 查询带 With<Player> 过滤，所以这里一定是玩家角色。
        role: Some(ActorRole::Player),
        faction: faction_of(&params.factions, Some(request.caster)),
        statuses: &statuses,
        reads: &reads,
        active_action_count: slot_count,
        threat: Some(&params.threat),
        target,
        target_resources,
        target_faction,
    };
    if !skill_available(skill, &context) {
        return false;
    }

    let Ok((mut resources, mut cooldowns)) = params.actors.get_mut(request.caster) else {
        return false;
    };
    for Cost { pool, amount } in &skill.costs {
        resources.modify(*pool, -*amount);
    }
    cooldowns.start(skill.id, skill.duration.max(0.5));

    let action = commands
        .spawn((
            super::Action {
                elapsed: 0.0,
                duration: skill.duration,
                target: request.target,
            },
            CastsSkill(request.skill),
            InitiatedBy(request.caster),
        ))
        .id();
    if skill.is_instant() {
        commands.entity(action).insert(ResolveNow);
    }
    true
}
