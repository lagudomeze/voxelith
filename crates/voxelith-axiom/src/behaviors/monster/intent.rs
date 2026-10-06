//! 怪物**意图**：每帧重算，**不落地**（L1）。
//!
//! 这是"意图是代码、不留存"的落点：没有决策组件、没有决策实体、没有排队。
//!
//! ```text
//! 每帧：自己的槽空？窗口空？能量满？→ 选招 → 付费用 / 上冷却 → 发 StartAction
//!       任何一步不成立就什么都不做（下一帧重算，无残留）
//! ```
//!
//! 于是"提交失败 = 无副作用"：抢不到窗口 / 槽被占的怪物不会留下半成品状态。
//! 原来那种"先决定、存起来、等窗口"的写法需要"条件变了也不改主意"，
//! 而现在**根本不存在等待期**——决定与提交在同一帧原子发生。
//!
//! **代价（内容侧要补偿）**：能量在窗口被占期间不再累积（这里只在准备出手时才 `tick`），
//! 于是"窗口占用时间"不影响怪物的出手节奏；这是"不排队"的直接推论。

use bevy_ecs::prelude::*;
use bevy_ecs::query::QueryData;
use bevy_time::Time;

use crate::atoms::action::ActiveActions;
use crate::atoms::actor::{
    ActionEnergy, ActorRole, ActorState, ActorTags, AiDriven, Cooldowns, Faction, Resources,
};
use crate::behaviors::action::StartAction;
use crate::behaviors::effect::EffectParams;
use crate::behaviors::requirement::CasterContext;
use crate::behaviors::targeting::{TargetingContext, faction_of, hostile_to, resolve_target};

use super::{MonsterDef, choose_skill};

/// 一个**可能要做决定**的怪物（查询项）。
#[derive(QueryData)]
#[query_data(mutable)]
pub struct IntendingMonster {
    /// 怪物实体（日志 / 上下文都用它）。
    pub entity: Entity,
    /// 出手能量。
    pub energy: &'static mut ActionEnergy,
    /// AI 候选。
    pub definition: &'static MonsterDef,
    /// 自己的池（付费用）。
    pub resources: &'static mut Resources,
    /// 自己的冷却（写冷却）。
    pub cooldowns: &'static mut Cooldowns,
    /// 状态槽（门控快照）。
    pub state: Option<&'static ActorState>,
    /// 特性标签。
    pub tags: Option<&'static ActorTags>,
    /// 阵营。
    pub faction: Option<&'static Faction>,
}

impl IntendingMonsterItem<'_, '_> {
    /// 槽里有没有正在进行的行动？
    ///
    /// 问的必须是"槽里真的有没有东西"，而不是"组件在不在"——关系组件的清理是**排队命令**，
    /// 同一帧里槽可能还在、只是集合已空（见 [docs/bevy-queries.md](../../../../docs/bevy-queries.md)）。
    pub fn is_busy(&self, slot: Option<&ActiveActions>) -> bool {
        slot.is_some_and(|active| !active.is_empty())
    }
}

/// **意图**：攒满能量就出手，但只在"槽空 + 窗口空"时。
///
/// 顺带结账：付技能的费用与冷却（**决定即付**——取消不退，这是设计）。
pub fn monster_intent(
    time: Res<Time>,
    mut starts: MessageWriter<StartAction>,
    mut monsters: Query<IntendingMonster, AiDriven>,
    slots: Query<Option<&ActiveActions>>,
    skills: Query<(Entity, &crate::behaviors::skill::Skill)>,
    params: EffectParams,
) {
    let delta = time.delta();

    for mut monster in &mut monsters {
        let actor = monster.entity;

        // ① 自己的槽空？（槽被占就没有意图，不去抢）
        if monster.is_busy(slots.get(actor).ok().flatten()) {
            continue;
        }
        // ② 全局窗口空？（窗口开着时时间本来也是停的；这里把它写成显式前提）
        if params.window.is_open() {
            continue;
        }
        // ③ 能量满？——只在"准备出手"这一条路上 tick，所以窗口被占期间不白攒。
        if !monster.energy.tick(delta) {
            continue;
        }

        // ④ 造上下文（只看得到自己的池；目标池与 AI 判定无关）
        let status_list = monster
            .state
            .map(|state| state.statuses.clone())
            .unwrap_or_default();
        let statuses = crate::behaviors::requirement::status_snapshot(
            &status_list,
            &params.statuses,
            &params.status_defs,
        );
        let targeting = TargetingContext {
            explicit: None,
            threat: params.window.first().copied(),
        };
        // 目标：按候选技能自己的 `Targeting` 解析，这里先用"最近的敌人"兜底。
        let target = {
            let any = skills
                .iter()
                .next()
                .map(|(_, skill)| skill.targeting)
                .unwrap_or(crate::behaviors::requirement::Targeting::NearestEnemy);
            resolve_target(any, actor, &targeting, hostile_to(&params.factions, actor))
        };
        let context = CasterContext {
            actor,
            resources: &monster.resources,
            stats: None,
            cooldowns: &monster.cooldowns,
            tags: monster.tags,
            role: Some(ActorRole::Monster),
            faction: monster.faction.copied(),
            statuses: &statuses,
            reads: &params,
            active_action_count: 0,
            threat: Some(&params.window),
            target,
            // 目标池不参与 AI 的门控（`TargetAlive` 之类的语义见 OPEN-QUESTIONS Q39）。
            target_resources: None,
            target_faction: faction_of(&params.factions, target),
        };

        // ⑤ 选招（纯函数）
        let Some(catalog) = params.skills_catalog() else {
            continue;
        };
        let Some(chosen) = choose_skill(
            &monster.definition.ai,
            &context,
            catalog,
            &params,
            &monster.resources,
        ) else {
            continue;
        };

        // ⑥ 决定即付：费用与冷却在**这一刻**结账（行动被取消也不退）。
        if let Some(skill) = skills.get(chosen.skill_entity).ok().map(|(_, skill)| skill) {
            for cost in &skill.costs {
                monster.resources.modify(cost.pool, -cost.amount);
            }
            monster.cooldowns.start(skill.id, skill.duration.max(0.5));
        }

        // ⑦ 提交（**唯一入口**：检查槽 / 窗口、每帧一次、唯一的 spawn 点都在那边）
        starts.write(StartAction::new(actor, chosen.skill_entity, target));
    }
}
