//! 怪物侧：**能量 → 意图 → 行动**。
//!
//! 怪物没有独立的时间轴：它靠 [`ActionEnergy`] 攒能量。攒满之后**意图与提交在同一帧原子发生**
//! （见 [`intent`]）：没有决策实体、没有排队、没有"存起来等窗口"。
//!
//! ```text
//! monster_intent   槽空 + 窗口空 + 能量满 → 选招 → 付费用 / 上冷却 → 发 StartAction
//! commit_actions   唯一入口：检查槽 / 窗口 → 建 Action（威胁到 PC 就发威胁消息）
//! ```
//!
//! 这条链路是反制窗口的**唯一来源**：
//!
//! ```text
//! monster_intent ─► StartAction ─► commit_actions ─► Threat 标记 + ThreatensPlayer
//!                                                     ─► ThreatWindow ─► update_phase ─► AwaitingCounter
//! ```

mod intent;

pub use intent::{IntendingMonster, monster_intent};

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;

use crate::atoms::actor::Resources;
use crate::behaviors::content::SkillCatalog;
use crate::behaviors::content::SkillId;
use crate::behaviors::requirement::{CasterContext, Condition, skill_available};

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

// **`Threat` 在 L0**（`atoms::action`）：它标的是**行动**，零依赖。
pub use crate::atoms::action::Threat;

/// 选出来的一招（**纯函数的结果，不落地**）。
///
/// 它不存目标：目标由意图在提交时一并交给 [`StartAction`](crate::behaviors::action::StartAction)，
/// 因为"这一招放不放得出来"（`ctx.target`）与"打谁"是两件事
/// （见 [docs/combat-design.md](../../../../docs/combat-design.md) §3.0 的三条正交轴）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChosenSkill {
    /// 技能词汇 ID（日志 / 表现说话用）。
    pub skill: SkillId,
    /// 决定那一刻解析好的技能实体（执行时不再查目录）。
    pub skill_entity: Entity,
    /// 选中时的权重（依据的凭证）。
    pub weight: f32,
}

/// 选招（**纯函数**）：只有**条件成立且技能可用**的候选参与，取权重最大者；平局取靠前的（确定性）。
pub fn choose_skill<B: crate::behaviors::effect::Blob>(
    choices: &[AiChoice],
    ctx: &CasterContext<'_, B>,
    catalog: &SkillCatalog,
    reads: &B,
    resources: &Resources,
) -> Option<ChosenSkill> {
    let mut best: Option<ChosenSkill> = None;
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
            best = Some(ChosenSkill {
                skill: choice.skill,
                skill_entity,
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
        // 系统注册在 `CombatPlugin`：`monster_intent` 必须在 `commit_actions` 之前，
        // 且整条链排在 `update_phase` 之前，跨域顺序是装配层的事。
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atoms::actor::Pool;
    use crate::behaviors::content::{ResourceId, StatusId};
    use crate::behaviors::skill::Skill;

    /// 条件判定本身不查世界（`HasStatus` 读的是调用方给的快照），所以这里给个空 Blob。
    struct NoReads;

    impl crate::behaviors::effect::Blob for NoReads {
        fn pools(&self, _entity: Entity) -> Option<&Resources> {
            None
        }
        fn stats(&self, _entity: Entity) -> Option<&crate::atoms::actor::Stats> {
            None
        }
        fn skills(&self) -> Option<Query<'_, '_, &'static Skill>> {
            None
        }
        fn status_defs(&self) -> Query<'_, '_, &'static crate::behaviors::status::StatusDef> {
            unreachable!("条件判定不查状态定义")
        }
        fn statuses(&self) -> Query<'_, '_, &'static crate::behaviors::status::ActiveStatus> {
            unreachable!("条件判定不查状态实例")
        }
        fn initiated(
            &self,
        ) -> Option<Query<'_, '_, &'static crate::behaviors::action::InitiatedBy>> {
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
        fn cooldowns(&self) -> Option<Query<'_, '_, &'static crate::atoms::actor::Cooldowns>> {
            None
        }
    }

    /// 造一个只带指定状态快照的上下文。
    fn context<'a>(
        resources: &'a Resources,
        cooldowns: &'a crate::atoms::actor::Cooldowns,
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
        let cooldowns = crate::atoms::actor::Cooldowns::default();
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
        let cooldowns = crate::atoms::actor::Cooldowns::default();
        let ctx = context(&resources, &cooldowns, &[]);
        assert!(condition_holds(Condition::Always, &ctx, &resources));
        assert_eq!(resources.current(ResourceId(9)), 0.0);
    }

    #[test]
    fn has_status_follows_the_snapshot() {
        let resources = Resources::default();
        let cooldowns = crate::atoms::actor::Cooldowns::default();
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
