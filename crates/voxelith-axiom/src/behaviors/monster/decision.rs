//! 决策槽：怪物"已经下定决心、还没轮到出手"的那一条决策（**一对一关系**）。
//!
//! ```text
//! 决策实体 ──DecidedBy──► 怪物      关系目标：DecisionSlot（**单个 Entity**，不是 Vec）
//! 决策实体 ──SettledBy──► 行动      关系目标：Settles（linked_spawn）
//! ```
//!
//! ## 为什么值得单独成槽
//!
//! "攒满能量 → 选招 → 生成行动"原来挤在**同一个 `if` 里**，于是"决定"没有落脚点：
//!
//! * **决定会被丢弃重掷**：威胁窗口被别的怪物占着时，只能下一帧重新决定一遍；
//!   而条件是随时会变的（残血、中毒、目标换人），那实际上是"每帧重掷、留下最后一次"。
//! * **决定不可观察**：想显示"这只怪正在酝酿挥砍"时，世界上还没有任何东西可读。
//!
//! 拆成 [`monster_decide`]（决定）与 [`monster_act`]（执行）之后，决定**留在槽里等窗口**：
//! 条件再变也不改主意，也让"怪物意图"在行动出现之前就能被读出来。
//!
//! ## 一对一的两个坑（都不是"报错"，是"沉默"）
//!
//! 1. **槽的"空"不是"长度为 0"，是没有 `DecisionSlot` 组件。** 0.19 的 `RelationshipTarget`
//!    不支持 `Option<Entity>`，所以"没决定"只能表达为**拆掉关系**。拆掉之后组件本身会被
//!    清掉，但那次清理是**排队命令**——同一帧里槽可能还在、只是空的。所以下面一律走
//!    [`decision_of`]（`iter()`），不去直接读字段。
//! 2. **"换一条决策"不会销毁旧决策实体。** 一对一关系在新源顶掉旧源时只做**解绑**：
//!    旧决策上的 `DecidedBy` 被自动移除，实体却留在世界里。所以本模块**不给它顶掉的
//!    机会**——[`monster_decide`] 只在槽空时写新决策，从根上删掉"换决策"这条路径。
//!    谁哪天删掉那个 `slot.is_some() → continue`，旧决策实体就会静静堆在世界里而
//!    **什么都不报**；`a_new_decision_never_leaves_an_orphan` 专门钉这条。
//!
//! ## 清空槽的三条路径（**都是关系级联，没有"反推"**）
//!
//! | 路径 | 机制 |
//! |---|---|
//! | 行动正常结算 | `resolve_actions` despawn 行动 → `Settles` 的 `linked_spawn` 级联 despawn 决策 |
//! | 行动被反制取消（`DispelAction` / `Interrupt`） | 同上（`cancel_actions` 也只是 despawn 行动） |
//! | 怪物自己没了 | `DecisionSlot` 的 `linked_spawn` 级联 despawn 决策 |
//!
//! **为什么不写"每帧扫一遍 `PendingThreat` 反推谁该退役"**：`Effect::DispelAction` 会把
//! `PendingThreat` 整个清空（连 `source` 都不留），反推根本无从下手——决策会永久卡在槽里，
//! 于是怪物再也不出手，而且什么都不报。挂到行动上就完全不需要反推。

use bevy_ecs::prelude::*;
use bevy_ecs::query::QueryData;
use bevy_time::Time;

use crate::atoms::actor::{
    ActionEnergy, ActorRole, AiDriven, Cooldowns, InputDriven, Resources, Stats,
};
use crate::behaviors::action::{Action, CastsSkill, InitiatedBy, ResolveNow};
use crate::behaviors::effect::EffectParams;
use crate::behaviors::phase::PendingThreat;
use crate::behaviors::requirement::CasterContext;
use crate::behaviors::skill::Skill;
use crate::behaviors::targeting::faction_of;

use super::{MonsterDef, Threat, choose_skill};

// **决策槽的原子（组件 + 关系）在 L0**：它们零依赖。
// 这份文件留着的是**系统**：怎么决定、怎么执行、什么时候退役。
pub use crate::atoms::decision::{
    AiDecision, DecidedBy, DecisionSlot, SettledBy, Settles, decision_of,
};
/// 一个**可能要做决定**的怪物（`monster_decide` 的查询项）。
///
/// 六个字段的四元组早就不止"元组"了：它们合起来是"这个怪物现在能不能决定"这件事的
/// 全部输入。做成具名结构之后，"它是不是正忙"从散落的条件变成 [`Self::is_busy`]，
/// 加字段也不用再去数元组里的第几位是哪个。
#[derive(QueryData)]
#[query_data(mutable)]
pub struct DecidingMonster {
    /// 怪物实体（日志 / 上下文 / 关系源都用它）。
    pub entity: Entity,
    /// 出手能量。
    pub energy: &'static mut ActionEnergy,
    /// AI 候选。
    pub definition: &'static MonsterDef,
    /// 当前决策槽（**数据，不是过滤器**：没有槽也要遍历到）。
    pub slot: Option<&'static DecisionSlot>,
    /// 自己的池（写费用）。
    pub resources: &'static mut Resources,
    /// 自己的冷却（写冷却）。
    pub cooldowns: &'static mut Cooldowns,
}

impl DecidingMonsterItem<'_, '_> {
    /// 槽里**真的有**决策吗？
    ///
    /// 问的必须是"有没有决策"，不能只看组件在不在：`DecisionSlot` 被拆掉时组件本身的
    /// 清理是**排队命令**，同一帧里它还可能在、只是集合已经空了（模块文档坑 1）——
    /// 那时怪物应该能立刻重新决定。
    pub fn is_busy(&self) -> bool {
        decision_of(self.slot).is_some()
    }
}

/// 决定：能量攒满 → 选招 → **写进决策槽**（不生成行动）。
///
/// 顺带结账：上一次的威胁行动已经不存在了（已生效 / 被反制取消）→ 清掉 [`PendingThreat`]。
/// 这一步只写资源、**不用 `Commands`**，所以它和 [`monster_act`] 之间不需要同步点
/// （决策的退役由关系级联负责，见模块文档）。
///
/// **能量与相位无关**：它读 `Res<Time>`，冻结时 `delta = 0`，自然暂停。
pub fn monster_decide(
    time: Res<Time>,
    mut commands: Commands,
    mut monsters: Query<DecidingMonster, AiDriven>,
    players: Query<Entity, InputDriven>,
    stats: Query<&Stats>,
    threats: Query<(), With<Threat>>,
    mut params: EffectParams,
) {
    // 结账：威胁行动实体没了（已生效 / 被反制取消）→ 登记作废。
    // 决策实体**不在这里退役**：它挂在行动上（`SettledBy` + `linked_spawn`），行动一没它就跟着走。
    if let Some(action) = params.threat.action
        && !threats.contains(action)
    {
        *params.threat = PendingThreat::default();
    }

    let delta = time.delta();
    let target = players.iter().next();
    // 目标阵营（`TargetIsEnemy` 用）：怪物打人之前先看阵营，而不是看"谁带 Player 标记"。
    let target_faction = faction_of(&params.factions, target);

    for mut monster in &mut monsters {
        // 已经有决定了（还没执行）→ 不再攒能量，也不改主意。
        //
        // 这一句同时挡住的是一对一关系的**孤儿泄漏路径**：不给"换一条决策"任何机会，
        // 只在槽空时 spawn 新决策。删掉它，等窗口的怪物会每帧重写一条决策，
        // 旧实体被静静解绑留在世界里，什么都不报。
        if monster.is_busy() {
            continue;
        }
        if !monster.energy.tick(delta) {
            continue;
        }
        let actor = monster.entity;

        // 自己的池用可变那份（写费用），属性走参数包（只读）。
        let actor_pools: &Resources = &monster.resources;
        let actor_stats = stats.get(actor).ok();
        let reads = params.reads_view(actor_pools, actor_stats);
        // 怪物自己的状态也要进上下文：不然 `Condition::HasStatus` 永远不成立
        // （"残血时逃跑"能写、"中毒时抓狂"写不出来）。
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
        let context = CasterContext {
            actor,
            resources: actor_pools,
            stats: actor_stats,
            cooldowns: &monster.cooldowns,
            tags: params.actor_tags.get(actor).ok(),
            // monsters 查询走 `AiDriven`（引擎角色轴的非玩家侧），所以这里一定是怪物角色。
            role: Some(ActorRole::Monster),
            faction: faction_of(&params.factions, Some(actor)),
            statuses: &statuses,
            reads: &reads,
            active_action_count: 0,
            threat: None,
            target,
            target_resources: None,
            target_faction,
        };

        let Some(catalog) = params.skills_catalog() else {
            continue;
        };
        let Some(decision) = choose_skill(
            &monster.definition.ai,
            &context,
            catalog,
            &reads,
            actor_pools,
            target,
        ) else {
            continue;
        };
        let Ok(skill) = params.skills.get(decision.skill_entity) else {
            continue;
        };

        // 费用与冷却在**决定那一刻**付：决定一旦写下就成立，不会因为"等窗口"的这几帧里
        // 池子变了而变成付不起——付不起的决策会永远卡在槽里，等于这只怪再也不会出手。
        for cost in &skill.costs {
            monster.resources.modify(cost.pool, -cost.amount);
        }
        monster.cooldowns.start(skill.id, skill.duration.max(0.5));

        commands.spawn((decision, DecidedBy(actor)));
    }
}

/// 执行：决策槽有货 + 威胁窗口空 → 生成威胁行动，并把决策挂到行动上（生命周期）。
///
/// 全世界只有一格威胁窗口，所以是**先到先得**：没轮到的怪物**留在槽里等**，
/// 下一帧不改主意、也不重新掷一次。
pub fn monster_act(
    mut commands: Commands,
    monsters: Query<(Entity, &DecisionSlot), AiDriven>,
    decisions: Query<&AiDecision>,
    skills: Query<&Skill>,
    mut threat: ResMut<PendingThreat>,
) {
    for (monster, slot) in &monsters {
        let Some(decision_entity) = decision_of(Some(slot)) else {
            continue;
        };
        let Ok(decision) = decisions.get(decision_entity) else {
            continue;
        };
        let Ok(skill) = skills.get(decision.skill_entity) else {
            // 技能实体没了（目录被重建 / 内容被移除）：这条决策**永远**执行不了。
            // 必须在这里退役，否则槽被永久占住——怪物再也不出手，而且什么都不报。
            commands.entity(decision_entity).despawn();
            continue;
        };
        if threat.is_active() {
            continue;
        }

        let action = commands
            .spawn((
                Action {
                    elapsed: 0.0,
                    duration: skill.duration,
                    target: decision.target,
                },
                CastsSkill(decision.skill_entity),
                InitiatedBy(monster),
                Threat,
            ))
            .id();
        if skill.is_instant() {
            commands.entity(action).insert(ResolveNow);
        }
        // 决策挂到行动上：行动一没，决策跟着没，槽自动空出来。
        commands.entity(decision_entity).insert(SettledBy(action));

        *threat = PendingThreat {
            action: Some(action),
            source: Some(monster),
            target: decision.target,
        };
    }
}
