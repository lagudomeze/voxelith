//! 怪物侧：**能量 → 决定 → 执行**。
//!
//! 怪物没有独立的时间轴：它靠 [`ActionEnergy`] 攒能量，攒满就选一招。但"选招"与
//! "出手"是**两个时刻**，中间隔着决策槽（[`DecisionSlot`]）：
//!
//! ```text
//! monster_decide  攒满能量 → 选招 → 写进决策槽（只决定，不出手）
//! monster_act     决策槽有货 + 威胁窗口空 → 生成威胁行动 + 登记 PendingThreat
//! ```
//!
//! 两步为什么必须拆开、决策槽为什么是**一对一**关系，见 [`decision`] 的模块文档。
//! 这条链路是反制窗口的**唯一来源**：
//!
//! ```text
//! monster_decide ─► DecisionSlot ─► monster_act ─► PendingThreat ─► update_phase ─► AwaitingCounter
//! ```

mod decision;

pub use decision::{
    AiDecision, DecidedBy, DecisionSlot, SettledBy, Settles, decision_of, monster_act,
    monster_decide,
};

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

/// 选招（**纯函数**）：只有**条件成立且技能可用**的候选参与，取权重最大者；平局取靠前的（确定性）。
///
/// 返回的是**一条完整的决策**：技能词汇 ID、决定那一刻解析好的技能实体、权重，以及锁定目标。
///
/// `target` 由调用方传入，而不是从 `ctx.target` 里取：`ctx.target` 回答的是"这一招放不放得
/// 出来"，决策要记的是"打谁"。两者此刻恰好相同，但**语义不同**，不该互相顶替
/// （见 [docs/combat-design.md](../../../../docs/combat-design.md) §4.0 的三条正交轴）。
pub fn choose_skill<B: crate::behaviors::effect::Blob>(
    choices: &[AiChoice],
    ctx: &CasterContext<'_, B>,
    catalog: &SkillCatalog,
    reads: &B,
    resources: &Resources,
    target: Option<Entity>,
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
                skill_entity,
                weight: choice.weight,
                target,
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
        // 系统注册在 `CombatPlugin`：`monster_decide` / `monster_act` 必须在 `update_phase`
        // 之前，而且两者之间要落一次 `ApplyDeferred`（刚写下的决策要当场可见），
        // 跨域顺序是装配层的事。
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
