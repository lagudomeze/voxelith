//! 怪物侧：**能量 → 选招 → 生成威胁行动**。
//!
//! 怪物没有独立的时间轴：它靠 [`ActionEnergy`] 攒能量，攒满就出手，并把新生成的
//! 行动登记进 [`PendingThreat`]（[`Threat`] 标记 = "这个行动即将生效"）。
//!
//! 这一条链路是反制窗口的**唯一来源**：
//!
//! ```text
//! monster_tick ─► SpawnAction(怪物技能) ─► PendingThreat ─► update_phase ─► AwaitingCounter
//! ```

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_time::Time;

use crate::atoms::actor::{ActionEnergy, ActorRole, Cooldowns, Player, Resources};
use crate::behaviors::action::{Action, CastsSkill, InitiatedBy, ResolveNow};
use crate::behaviors::content::SkillCatalog;
use crate::behaviors::content::SkillId;
use crate::behaviors::effect::EffectParams;
use crate::behaviors::phase::PendingThreat;
use crate::behaviors::requirement::{CasterContext, Condition, skill_available};
use crate::behaviors::targeting::faction_of;

/// AI 的一个候选技能（运行时形态：技能已解析成 ID）。
///
/// **不派生 `Deserialize`**：RON 形态是 [`AiChoiceRon`](crate::behaviors::content::AiChoiceRon)。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AiChoice {
    /// 技能名（加载期解析成 ID）。
    pub skill: SkillId,
    /// 什么情况下考虑它。
    pub when: Condition,
    /// 权重（越大越优先）。
    pub weight: f32,
}

/// 怪物定义（组件）：内容层生成怪物实体时挂上。
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct MonsterDef {
    /// 显示名。
    pub name: String,
    /// 候选技能与权重（顺序 = 平局时的优先级，保证确定性）。
    pub ai: Vec<AiChoice>,
}

/// 威胁标记：挂在这个行动实体上 = "它即将生效"。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Threat;

/// 被选中的 AI 决策。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AiDecision {
    /// 技能标识。
    pub skill: SkillId,
    /// 选中时的权重。
    pub weight: f32,
}

/// 怪物 tick：推进能量、清掉失效威胁、攒满则生成威胁行动。
///
/// **能量与相位无关**：它读 `Res<Time>`，冻结时 `delta = 0`，自然暂停。
pub fn monster_tick(
    time: Res<Time>,
    mut commands: Commands,
    mut monsters: Query<
        (
            Entity,
            &mut ActionEnergy,
            &MonsterDef,
            &mut Resources,
            &mut Cooldowns,
        ),
        Without<Player>,
    >,
    players: Query<Entity, With<Player>>,
    stats: Query<&crate::atoms::actor::Stats>,
    threats: Query<Entity, With<Threat>>,
    mut params: EffectParams,
) {
    // 1. 威胁行动已经不存在了（被反制取消 / 已生效）→ 清登记。
    if let Some(action) = params.threat.action
        && threats.get(action).is_err()
    {
        *params.threat = PendingThreat::default();
    }

    let delta = time.delta();
    let target = players.iter().next();
    // 目标阵营（`TargetIsEnemy` 用）：怪物打人之前先看阵营，而不是看"谁带 Player 标记"。
    let target_faction = faction_of(&params.factions, target);
    let empty_cooldowns = Cooldowns::default();

    for (monster, mut energy, definition, mut resources, mut cooldowns) in &mut monsters {
        let ready = energy.tick(delta);
        if !ready {
            continue;
        }
        if params.threat.is_active() {
            // 同一时刻只挂一个威胁：本帧先不出手，能量留着下次再试。
            continue;
        }

        // 自己的池用可变那份（写费用），属性走参数包（只读）。
        let actor_pools: &Resources = &resources;
        let actor_stats = stats.get(monster).ok();
        let reads = params.reads_view(actor_pools, actor_stats);
        // 怪物自己的状态也要进上下文：不然 `Condition::HasStatus` 永远不成立
        // （"残血时逃跑"能写、"中毒时抓狂"写不出来）。
        let status_list = params
            .actor_states
            .get(monster)
            .map(|state| state.statuses.clone())
            .unwrap_or_default();
        let statuses = crate::behaviors::requirement::status_snapshot(
            &status_list,
            &params.statuses,
            &params.status_defs,
        );
        let context = CasterContext {
            actor: monster,
            resources: actor_pools,
            stats: actor_stats,
            cooldowns: &cooldowns,
            tags: params.actor_tags.get(monster).ok(),
            // monsters 查询带 Without<Player>，所以这里一定是怪物角色。
            role: Some(ActorRole::Monster),
            faction: faction_of(&params.factions, Some(monster)),
            statuses: &statuses,
            reads: &reads,
            active_action_count: 0,
            threat: None,
            target,
            target_resources: None,
            target_faction,
        };
        let _ = &empty_cooldowns;

        let Some(catalog) = params.skills_catalog() else {
            continue;
        };
        let Some(decision) = choose_skill(&definition.ai, &context, catalog, &reads, actor_pools)
        else {
            continue;
        };
        let Some(&skill_entity) = catalog.get(decision.skill) else {
            continue;
        };
        let Ok(skill) = params.skills.get(skill_entity) else {
            continue;
        };

        // 扣费。
        for cost in &skill.costs {
            resources.modify(cost.pool, -cost.amount);
        }
        cooldowns.start(skill.id, skill.duration.max(0.5));

        let action = commands
            .spawn((
                Action {
                    elapsed: 0.0,
                    duration: skill.duration,
                    target,
                },
                CastsSkill(skill_entity),
                InitiatedBy(monster),
                Threat,
            ))
            .id();
        if skill.is_instant() {
            commands.entity(action).insert(ResolveNow);
        }

        *params.threat = PendingThreat {
            action: Some(action),
            source: Some(monster),
            target,
        };
    }
}

/// 选招：只有**条件成立且技能可用**的候选参与，取权重最大者；平局取靠前的（确定性）。
pub fn choose_skill<B: crate::behaviors::effect::Blob>(
    choices: &[AiChoice],
    ctx: &CasterContext<'_, B>,
    catalog: &SkillCatalog,
    reads: &B,
    resources: &Resources,
) -> Option<AiDecision> {
    let mut best: Option<AiDecision> = None;
    for choice in choices {
        if !condition_holds(choice.when, ctx, resources) {
            continue;
        }
        let Some(&skill_entity) = catalog.get(choice.skill) else {
            continue;
        };
        let Some(skills) = reads.skills() else {
            continue;
        };
        let Ok(skill) = skills.get(skill_entity) else {
            continue;
        };
        if !skill_available(skill, ctx) {
            continue;
        }
        let better = best.is_none_or(|current| choice.weight > current.weight);
        if better {
            best = Some(AiDecision {
                skill: choice.skill,
                weight: choice.weight,
            });
        }
    }
    best
}

/// AI 条件（比 `Requirement` 少：它是"要不要考虑这招"，不是"能不能放"）。
pub fn condition_holds<B: crate::behaviors::effect::Blob>(
    cond: Condition,
    ctx: &CasterContext<'_, B>,
    resources: &Resources,
) -> bool {
    match cond {
        Condition::Always => true,
        Condition::ResourceBelow { pool, ratio } => resources
            .pool(pool)
            .is_some_and(|pool| pool.ratio() < ratio),
        Condition::HasStatus(status) => crate::behaviors::requirement::has_status(ctx, status),
        Condition::HasThreat => crate::behaviors::requirement::threat_active(ctx),
    }
}

/// 注册怪物域。
pub struct MonsterPlugin;

impl Plugin for MonsterPlugin {
    fn build(&self, _app: &mut App) {
        // 系统注册在 `CombatPlugin`：`monster_tick` 必须在 `update_phase` 之前，
        // 而 `update_phase` 属于相位域，跨域顺序是装配层的事。
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atoms::actor::Pool;
    use crate::behaviors::content::{ResourceId, StatusId};

    /// 条件判定本身不查世界（`HasStatus` 读的是调用方给的快照），所以这里给个空 Blob。
    struct NoReads;

    impl crate::behaviors::effect::Blob for NoReads {
        fn pools(&self, _entity: Entity) -> Option<&Resources> {
            None
        }
        fn stats(&self, _entity: Entity) -> Option<&crate::atoms::actor::Stats> {
            None
        }
        fn skills(&self) -> Option<Query<'_, '_, &'static crate::behaviors::skill::Skill>> {
            None
        }
        fn status_defs(&self) -> Query<'_, '_, &'static crate::behaviors::status::StatusDef> {
            unreachable!("条件判定不查状态定义")
        }
        fn statuses(&self) -> Query<'_, '_, &'static crate::behaviors::status::ActiveStatus> {
            unreachable!("条件判定不查状态实例")
        }
        fn initiated(&self) -> Option<Query<'_, '_, &'static InitiatedBy>> {
            None
        }
        fn active_actions(
            &self,
        ) -> Option<Query<'_, '_, &'static crate::behaviors::action::ActiveActions>> {
            None
        }
        fn actor_states(&self) -> Query<'_, '_, &'static crate::atoms::actor::ActorState> {
            unreachable!("条件判定不查状态槽")
        }
        fn cooldowns(&self) -> Option<Query<'_, '_, &'static Cooldowns>> {
            None
        }
    }

    /// 造一个只带指定状态快照的上下文。
    fn context<'a>(
        resources: &'a Resources,
        cooldowns: &'a Cooldowns,
        statuses: &'a [(StatusId, crate::behaviors::skill::SkillTags)],
    ) -> CasterContext<'a, NoReads> {
        CasterContext {
            actor: Entity::PLACEHOLDER,
            resources,
            stats: None,
            cooldowns,
            tags: None,
            role: None,
            faction: None,
            statuses,
            reads: &NoReads,
            active_action_count: 0,
            threat: None,
            target: None,
            target_resources: None,
            target_faction: None,
        }
    }

    #[test]
    fn resource_below_follows_the_pool_ratio() {
        let mut resources = Resources::default();
        resources.define(ResourceId(0), Pool::full(100.0));
        let cooldowns = Cooldowns::default();
        let low = Condition::ResourceBelow {
            pool: ResourceId(0),
            ratio: 0.5,
        };

        {
            let ctx = context(&resources, &cooldowns, &[]);
            assert!(!condition_holds(low, &ctx, &resources));
        }

        resources.modify(ResourceId(0), -95.0);
        let ctx = context(&resources, &cooldowns, &[]);
        assert!(condition_holds(low, &ctx, &resources));
    }

    #[test]
    fn always_holds_and_missing_pool_reads_zero() {
        let resources = Resources::default();
        let cooldowns = Cooldowns::default();
        let ctx = context(&resources, &cooldowns, &[]);
        assert!(condition_holds(Condition::Always, &ctx, &resources));
        assert_eq!(resources.current(ResourceId(9)), 0.0);
    }

    #[test]
    fn has_status_follows_the_snapshot() {
        let resources = Resources::default();
        let cooldowns = Cooldowns::default();
        let snapshot = [(StatusId(7), crate::behaviors::skill::SkillTags::NONE)];
        let ctx = context(&resources, &cooldowns, &snapshot);
        assert!(condition_holds(
            Condition::HasStatus(StatusId(7)),
            &ctx,
            &resources
        ));
        assert!(!condition_holds(
            Condition::HasStatus(StatusId(8)),
            &ctx,
            &resources
        ));
    }
}
