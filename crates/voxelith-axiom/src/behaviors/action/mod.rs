//! 行动（Action）：**技能实例**与它的推进 / 结算。
//!
//! | | 定义 | 实例 |
//! |---|---|---|
//! | 技能 | [`Skill`] 实体 | [`Action`] 实体 |
//! | 连接 | [`CastsSkill`] | [`InitiatedBy`] |
//!
//! **槽位只有"有没有"**：`ActiveActions.len() <= 1` 是唯一约束，不区分种类（没有 `SlotKind`）。
//! **`duration == 0` 即瞬发**：spawn 后同帧结算并 despawn，**从不真正占槽**。
//!
//! 释放请求的处理在 [`casting`]（要写池，参数形状与结算不同）。

mod casting;

pub use casting::{CastParams, cast_requests};

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_time::Time;

use crate::atoms::actor::{ActorRole, Monster, Player, Resources, Stats};
use crate::behaviors::content::{SkillCatalog, StatusCatalog};
use crate::behaviors::contest::CombatRng;
use crate::behaviors::effect::{EffectContext, EffectParams, execute_effect};
use crate::behaviors::phase::AvailableSkills;
use crate::behaviors::phase::{CombatLog, PendingThreat};
use crate::behaviors::requirement::CasterContext;
use crate::behaviors::skill::Skill;
use crate::behaviors::targeting::{
    TargetingContext, faction_of as target_faction_of, hostile_to, resolve_target,
};

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

/// 释放请求（**Message**，请求型）：L2 输入 / AI → L1。
///
/// 定义在发出它的模块（**R33**），由 [`ActionPlugin`] 注册（**R34**）。
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct CastRequest {
    /// 谁释放。
    pub caster: Entity,
    /// 释放哪个技能（`Skill` 实体）。
    pub skill: Entity,
    /// 指定目标（`Targeting::CurrentSelection` 用它）。
    pub target: Option<Entity>,
}

/// 推进行动：累加 `elapsed`，到时长打 [`ReadyToResolve`]。
///
/// 冻结（`Time<Virtual>` 倍率为 0）时 `delta = 0`，所以这里**不需要知道相位**。
pub fn tick_actions(
    time: Res<Time>,
    mut commands: Commands,
    mut actions: Query<(Entity, &mut Action), Without<ReadyToResolve>>,
    instant: Query<(), With<ResolveNow>>,
) {
    let delta = time.delta_secs();
    for (entity, mut action) in &mut actions {
        if instant.contains(entity) {
            commands.entity(entity).insert(ReadyToResolve);
            continue;
        }
        action.elapsed += delta;
        if action.is_complete() {
            commands.entity(entity).insert(ReadyToResolve);
        }
    }
}

/// 结算行动：解析目标 → 执行技能的全部效果 → 销毁实例（槽自动空出来）。
pub fn resolve_actions(
    mut commands: Commands,
    ready: Query<(Entity, &CastsSkill, &InitiatedBy, &Action), With<ReadyToResolve>>,
    skills: Query<&Skill>,
    monsters: Query<Entity, With<Monster>>,
    resources: Query<&Resources>,
    stats: Query<&Stats>,
    mut params: EffectParams,
) {
    for (action_entity, cast, owner, action) in &ready {
        let Ok(skill) = skills.get(cast.0) else {
            continue;
        };

        let targeting = TargetingContext {
            explicit: action.target,
            threat: Some(*params.threat),
        };
        let target = resolve_target(skill.targeting, owner.0, &targeting, monsters.iter());

        // 只读视图由各查询的**副本**拼成（只读 `Query` 是 `Copy`），
        // 于是 rng / threat / log 可以同时以 `&mut` 借出。
        // 三个可变资源的借用与"只读视图"不能重叠：先取裸指针，用时才重借
        // （裸指针本身不构成借用，所以下面仍可不可变地读 params）。
        let rng_ptr: *mut CombatRng = &mut *params.rng;
        let threat_ptr: *mut PendingThreat = &mut *params.threat;
        let log_ptr: *mut CombatLog = &mut *params.log;
        // 目录缺席时用**空目录**兜底：SpawnAction / ApplyStatus 这类效果自然什么都不做，
        // 但同一技能里别的效果（改池、写日志）照常执行——内容层的"目录注入"是可选前提。
        let empty_skills = SkillCatalog::default();
        let empty_statuses = StatusCatalog::default();
        let skill_catalog = params.skills_catalog().unwrap_or(&empty_skills);
        let status_catalog = params.statuses_catalog().unwrap_or(&empty_statuses);
        let mut context = EffectContext {
            commands: &mut commands,
            skill_catalog,
            status_catalog,
            reads: params.reads_view(&resources, &stats),
            rng: unsafe { &mut *rng_ptr },
            pending_threat: unsafe { &mut *threat_ptr },
            log: unsafe { &mut *log_ptr },
            skill_power: 0.0,
        };
        for effect in &skill.effects {
            execute_effect(effect, owner.0, target, &mut context);
        }

        commands.entity(action_entity).despawn();
    }
}

/// 计算当前可用技能（**派生数据**：每帧重算，L2 只读，不参与判定）。
///
/// 判据全部来自 [`skill_available`](crate::behaviors::requirement::skill_available)：
/// 状态门控 → 需求 → 消耗（**R8**：只查自己领域的东西）。
///
/// **整批重建**（先 `clear` 再填）：它是"当前状态"而不是"累积日志"，
/// 漏掉 `clear` 的话，敌人换阵营 / 状态到期之后 UI 会一直显示过期的按钮。
pub fn compute_available_skills(
    mut available: ResMut<AvailableSkills>,
    players: Query<Entity, With<Player>>,
    skills: Query<(Entity, &Skill)>,
    resources: Query<&Resources>,
    stats: Query<&Stats>,
    params: EffectParams,
) {
    available.clear();
    let actors: Vec<Entity> = players.iter().collect();
    for actor in actors {
        let Ok(actor_resources) = resources.get(actor) else {
            continue;
        };
        let status_list = params
            .actor_states
            .get(actor)
            .map(|state| state.statuses.clone())
            .unwrap_or_default();
        let statuses = crate::behaviors::requirement::status_snapshot(
            &status_list,
            &params.statuses,
            &params.status_defs,
        );
        let empty = crate::atoms::actor::Cooldowns::default();
        // 目标按**每个技能自己的** `Targeting` 解析：不然 `TargetIsEnemy` 这类要求
        // 永远拿不到目标，技能会被误判为不可用（UI 就少一个按钮）。
        let targeting = TargetingContext {
            explicit: None,
            threat: Some(*params.threat),
        };
        let faction = target_faction_of(&params.factions, Some(actor));

        for (entity, skill) in &skills {
            let target = resolve_target(
                skill.targeting,
                actor,
                &targeting,
                hostile_to(&params.factions, actor),
            );
            let context = CasterContext {
                actor,
                resources: actor_resources,
                stats: stats.get(actor).ok(),
                cooldowns: &empty,
                tags: params.actor_tags.get(actor).ok(),
                // players 查询带 With<Player>，所以这里一定是玩家角色。
                role: Some(ActorRole::Player),
                faction,
                statuses: &statuses,
                reads: &params,
                active_action_count: params
                    .active_actions
                    .get(actor)
                    .map_or(0, |active| active.len()),
                threat: Some(&params.threat),
                target,
                target_resources: target.and_then(|entity| resources.get(entity).ok()),
                target_faction: target_faction_of(&params.factions, target),
            };
            if crate::behaviors::requirement::skill_available(skill, &context) {
                available.push(entity);
            }
        }
    }
}

/// 注册行动域：可用技能 Resource（派生数据）+ 释放请求消息（**R34**）。
///
/// **系统顺序由装配层 [`CombatPlugin`](crate::behaviors::combat::CombatPlugin) 统一注册**：
/// 顺序是契约（`cast_requests → tick_actions → resolve_actions` 显式串行），
/// 只在一个地方写，才不会让同一个系统在一帧里跑两遍
/// （跑两遍的话，`Commands` 会在两次运行之间被应用，一条请求就变成两个行动）。
pub struct ActionPlugin;

impl Plugin for ActionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AvailableSkills>()
            .add_message::<CastRequest>();
    }
}

/// 行动槽里正在进行的行动（L2 只读）。
pub fn active_of(active: Option<&ActiveActions>) -> &[Entity] {
    active.map_or(&[], |active| active.actions())
}
