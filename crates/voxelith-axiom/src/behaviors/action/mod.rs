//! 行动（Action）：**技能实例**与它的推进 / 结算。
//!
//! | | 定义 | 实例 |
//! |---|---|---|
//! | 技能 | [`Skill`] 实体 | [`Action`] 实体 |
//! | 连接 | [`CastsSkill`] | [`InitiatedBy`] |
//!
//! 「一次只执行一个技能动作」的落点：
//!
//! - **唯一入口**是 [`commit_actions`]（见 [`commit`]）——所有写入方都只发消息；
//! - 槽是 `InitiatedBy` / `ActiveActions`；`ActiveActions` 带 `linked_spawn`、
//!   `InitiatedBy` 带释放钩子（**解绑即销毁**），两条防线都在 L0 的 `atoms::action` 里；
//! - 推进分**三段**：前摇 → 释放点 → 后摇（见下）。
//!
//! ```text
//! WindUp   [t0, t0+W)       推进 elapsed；到时长打 ReadyToResolve
//! Resolve  t0+W             跑 Skill.effects；摘掉 Threat；有后摇则切 Recovery，否则 despawn
//! Recovery [t0+W, t0+W+R)   继续推进；到时长就 despawn（**不再跑效果**）
//! ```
//!
//! **阶段字段就是判别式**：`resolve_actions` 靠 `Action.phase` 区分"释放点"与"后摇结束"，
//! 所以同一个 `ReadyToResolve` 标记在两种阶段下含义不同、不会重复结算。
//!
//! 释放请求的处理在 [`casting`]（要写池，参数形状与结算不同）。

mod casting;
mod commit;

pub use casting::{CastParams, cast_requests};
pub use commit::{StartAction, commit_actions};

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_ecs::query::QueryData;
use bevy_time::Time;

use crate::atoms::actor::{ActorRole, InputDriven, Monster, Resources, Stats};
use crate::behaviors::content::{SkillCatalog, StatusCatalog};
use crate::behaviors::contest::CombatRng;
use crate::behaviors::effect::{EffectContext, EffectParams, execute_effect};
use crate::behaviors::phase::{AvailableSkills, CombatLog};
use crate::behaviors::requirement::CasterContext;
use crate::behaviors::skill::Skill;
use crate::behaviors::targeting::{
    TargetingContext, faction_of as target_faction_of, hostile_to, resolve_target,
};
use crate::behaviors::threat::ThreatWindow;

// **行动的原子（组件 + 关系）在 L0**：它们零依赖，所以不该长在这里。
// 路径照旧可用（`behaviors::action::Action`），新增依赖时要先想清楚是不是越层。
pub use crate::atoms::action::{
    Action, ActionPhase, ActiveActions, CastingSkill, CastsSkill, InitiatedBy, Interrupts,
    ReadyToResolve, ResolveNow, SuperArmor, Threat, active_of,
};

/// 一条**到时长、待结算**的行动（`resolve_actions` 的查询项）。
///
/// **`With<ReadyToResolve>` 仍然是过滤器，不是这里的字段**：它是"扫哪些实体"，
/// 放进 `QueryData` 就变成"拿到了就是 `None`"，语义与性能都不一样
/// （见 [docs/bevy-queries.md](../../../../docs/bevy-queries.md)）。
#[derive(QueryData)]
pub struct ReadyAction {
    /// 行动实体本身。
    pub entity: Entity,
    /// 它释放的技能定义。
    pub cast: &'static CastsSkill,
    /// 发起它的角色。
    pub owner: &'static InitiatedBy,
    /// 行动本身（阶段 / 进度 / 目标）。
    pub action: &'static Action,
}

/// 释放请求（**Message**，请求型）：L2 输入 / AI → L1。
///
/// 定义在发出它的模块（**R33**），由 [`ActionPlugin`] 注册（**R34**）。
/// 校验通过后转成 [`StartAction`]，真正建行动的是 [`commit_actions`]。
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct CastRequest {
    /// 谁释放。
    pub caster: Entity,
    /// 释放哪个技能（`Skill` 实体）。
    pub skill: Entity,
    /// 指定目标（`Targeting::CurrentSelection` 用它）。
    pub target: Option<Entity>,
}

/// 推进行动：累加当前阶段的 `elapsed`，到时长打 [`ReadyToResolve`]。
///
/// 冻结（`Time` 倍率为 0）时 `delta = 0`，所以这里**不需要知道相位**。
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

/// 结算行动：解析目标 → 执行技能的全部效果 → 进后摇或销毁。
///
/// 两类路径靠 `Action.phase` 区分：
///
/// - `WindUp`：**释放点**。跑效果、摘掉 [`Threat`]；有后摇就切 `Recovery`，否则销毁。
/// - `Recovery`：后摇走完，**不跑效果**，直接销毁（槽自动空出来）。
///
/// 技能实体没了（目录被重建 / 内容被移除）→ **退役这条行动**：否则它会永久占着槽，
/// 而且什么都不报（热重载删技能就能走到这条路）。
pub fn resolve_actions(
    mut commands: Commands,
    ready: Query<ReadyAction, With<ReadyToResolve>>,
    skills: Query<&Skill>,
    monsters: Query<Entity, With<Monster>>,
    resources: Query<&Resources>,
    stats: Query<&Stats>,
    mut params: EffectParams,
) {
    for pending in &ready {
        let entity = pending.entity;

        if pending.action.phase == ActionPhase::Recovery {
            commands.entity(entity).try_despawn();
            continue;
        }

        let Ok(skill) = skills.get(pending.cast.0) else {
            // 技能没了 ⇒ 这条行动永远执行不了：退役，别占着槽。
            commands.entity(entity).try_despawn();
            continue;
        };

        let targeting = TargetingContext {
            explicit: pending.action.target,
            threat: params.window.first().copied(),
        };
        let owner = pending.owner.0;
        let target = resolve_target(skill.targeting, owner, &targeting, monsters.iter());

        // 只读视图由各查询的**副本**拼成（只读 `Query` 是 `Copy`），
        // 于是 rng / window / log 可以同时以 `&mut` 借出。
        let rng_ptr: *mut CombatRng = &mut *params.rng;
        let window_ptr: *mut ThreatWindow = &mut *params.window;
        let log_ptr: *mut CombatLog = &mut *params.log;
        // 目录缺席时用**空目录**兜底：相关效果自然什么都不做，
        // 但同一技能里别的效果（改池、写日志）照常执行。
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
            window: unsafe { &mut *window_ptr },
            log: unsafe { &mut *log_ptr },
            skill_power: 0.0,
        };
        for effect in &skill.effects {
            execute_effect(effect, owner, target, &mut context);
        }

        // 释放点之后：威胁标记摘掉（窗口立刻让给下一个），实体进后摇或直接销毁。
        commands.entity(entity).remove::<Threat>();
        commands.entity(entity).remove::<ReadyToResolve>();
        if skill.recovery > 0.0 {
            let mut next = *pending.action;
            next.enter_recovery(skill.recovery);
            commands.entity(entity).insert(next);
        } else {
            commands.entity(entity).try_despawn();
        }
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
    players: Query<Entity, InputDriven>,
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
            threat: params.window.first().copied(),
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
                // players 查询走 `InputDriven`（引擎角色轴的玩家侧），所以这里一定是玩家角色。
                role: Some(ActorRole::Player),
                faction,
                statuses: &statuses,
                reads: &params,
                active_action_count: params
                    .active_actions
                    .get(actor)
                    .map_or(0, |active| active.len()),
                threat: Some(&params.window),
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

/// 注册行动域：可用技能 Resource（派生数据）+ 两条消息（**R34**）。
///
/// **系统顺序由装配层 [`CombatPlugin`](crate::behaviors::combat::CombatPlugin) 统一注册**：
/// 顺序是契约，只在一个地方写，才不会让同一个系统在一帧里跑两遍
/// （跑两遍的话，`Commands` 会在两次运行之间被应用，一条请求就变成两个行动）。
pub struct ActionPlugin;

impl Plugin for ActionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AvailableSkills>()
            .add_message::<CastRequest>()
            .add_message::<StartAction>();
    }
}
