//! `Effect`：**唯一的世界突变原语**（[docs/combat-design.md](../../../../docs/combat-design.md) §3.3）。
//!
//! 技能结算与状态生命周期**共用同一套 Effect**——这就是"效果系统统一"的落点。
//! 里面**没有 `Damage`**：伤害 = `Contest(攻 vs 防)` 成功后的
//! [`ModifyResource`](Effect::ModifyResource)。
//!
//! 两条纪律：
//!
//! 1. **先算完数值，再动世界**（求值走只读快照，写世界走 [`Commands`] 延迟应用）；
//! 2. 写入口只有 `Commands` + 随机源 + 威胁登记 + 日志——属性最终值**不在这里改**，
//!    它由 `apply_status_modifiers` 从状态派生。
//!
//! 为了让调用方（结算系统、状态系统、怪物系统）不必各自手搓一份上下文，
//! 这里用 [`Blob`] 抽象"只读查询集合"，并在 [`params`] 里给出**共享的**
//! `#[derive(SystemParam)]` 打包（一个参数顶十几个，见该模块文档）。

mod apply;
mod blob;
mod params;

pub use blob::{Blob, Reads};

use apply::{apply_status, find_status, modify_resource, remove_status};
pub use params::EffectParams;

use bevy_ecs::prelude::*;

use crate::behaviors::action::{Action, CastsSkill, InitiatedBy, ResolveNow};
use crate::behaviors::content::{ResourceId, SkillCatalog, SkillId, StatusCatalog, StatusId};
use crate::behaviors::contest::{CombatRng, Contest, outcome_effects, resolve_contest};
use crate::behaviors::phase::{CombatLog, PendingThreat};
use crate::behaviors::requirement::Condition;

use crate::behaviors::value::{EvalContext, Value, Who, eval};

/// 效果：世界突变的**受控闭集**。
///
/// **不派生 `Deserialize`**：RON 里的引用是名字，由
/// [`EffectRon`](crate::behaviors::content::EffectRon) + 加载器翻译成 ID。
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// 改资源池：正数=回复，负数=伤害 / 消耗。
    ModifyResource {
        /// 哪个池。
        pool: ResourceId,
        /// 增减量。
        delta: Value,
        /// 哪一方。
        who: Who,
    },
    /// 施加状态。
    ApplyStatus {
        /// 哪种状态。
        status: StatusId,
        /// 持续时长（秒）。
        duration: Value,
        /// 给谁。
        who: Who,
    },
    /// 移除状态（跑 `on_remove`）。
    RemoveStatus {
        /// 哪种状态。
        status: StatusId,
        /// 从谁身上移除。
        who: Who,
    },
    /// 取消目标**当前正在进行的行动**（反制核心）。
    ///
    /// `Who::Target` 优先解析为"挂起威胁的来源"，没有威胁时退回目标实体。
    DispelAction {
        /// 取消谁的行动。
        who: Who,
    },
    /// 打断目标：清空目标的行动槽（不取消威胁登记）。
    Interrupt {
        /// 打断谁。
        who: Who,
    },
    /// 让某一方释放一个技能（生成行动实例；瞬发同帧结算）。
    SpawnAction {
        /// 释放哪个技能。
        skill: SkillId,
        /// 谁释放。
        who: Who,
    },
    /// 写一条战斗日志（L2 只读）。
    Log {
        /// 文案。
        text: String,
    },
    /// 对抗：按结果分支执行效果，并把强度写进后续效果的 `SkillPower`。
    Contest(Box<Contest>),
    /// 顺序执行。
    Sequence(Vec<Effect>),
    /// 条件分支。
    Conditional {
        /// 判据。
        cond: Condition,
        /// 成立时。
        then: Box<Effect>,
        /// 不成立时。
        else_: Box<Effect>,
    },
}

/// 执行上下文：只读数据 + 唯一写入口。
pub struct EffectContext<'a, 'w, 's, B: Blob> {
    /// 增删实体 / 组件（延迟应用）。
    pub commands: &'a mut Commands<'w, 's>,
    /// 技能目录（`SkillId` → 定义实体）。
    pub skill_catalog: &'a SkillCatalog,
    /// 状态目录（`StatusId` → 定义实体）。
    pub status_catalog: &'a StatusCatalog,
    /// 只读查询集合。
    pub reads: B,
    /// 随机源（只有 `RollUnder` 会用到）。
    pub rng: &'a mut CombatRng,
    /// 挂起威胁。
    pub pending_threat: &'a mut PendingThreat,
    /// 战斗日志（追加式）。
    pub log: &'a mut CombatLog,
    /// 本次结算的强度（对抗差值 / 比值）；顶层入口为 0。
    pub skill_power: f32,
}

/// 执行一个效果（入口）。
pub fn execute_effect<B: Blob>(
    effect: &Effect,
    caster: Entity,
    target: Option<Entity>,
    ctx: &mut EffectContext<'_, '_, '_, B>,
) {
    ctx.skill_power = 0.0;
    execute_with_power(effect, caster, target, 0.0, ctx);
}

/// 带强度地执行效果（`Contest` 分支会把差值 / 比值传下去）。
pub(super) fn execute_with_power<B: Blob>(
    effect: &Effect,
    caster: Entity,
    target: Option<Entity>,
    skill_power: f32,
    ctx: &mut EffectContext<'_, '_, '_, B>,
) {
    match effect {
        Effect::ModifyResource { pool, delta, who } => {
            let Some(actor) = pick(*who, caster, target) else {
                return;
            };
            let amount = value_of(delta, caster, target, skill_power, ctx);
            modify_resource(ctx, actor, *pool, amount);
        }

        Effect::ApplyStatus {
            status,
            duration,
            who,
        } => {
            let Some(actor) = pick(*who, caster, target) else {
                return;
            };
            let Some(&def_entity) = ctx.status_catalog.get(*status) else {
                ctx.log
                    .push(format!("未知状态 {status:?}（加载期应已拦截）"));
                return;
            };
            let seconds = value_of(duration, caster, target, skill_power, ctx);
            apply_status(ctx, def_entity, actor, caster, seconds);
        }

        Effect::RemoveStatus { status, who } => {
            let Some(actor) = pick(*who, caster, target) else {
                return;
            };
            let Some(&def_entity) = ctx.status_catalog.get(*status) else {
                return;
            };
            remove_status(ctx, actor, def_entity);
        }

        Effect::DispelAction { who } => {
            let victim = match who {
                Who::Target => ctx.pending_threat.source.or(target),
                Who::Caster => Some(caster),
            };
            if let Some(victim) = victim {
                cancel_actions(ctx, victim);
            }
            ctx.pending_threat.action = None;
            ctx.pending_threat.source = None;
            ctx.pending_threat.target = None;
        }

        Effect::Interrupt { who } => {
            if let Some(victim) = pick(*who, caster, target) {
                cancel_actions(ctx, victim);
            }
        }

        Effect::SpawnAction { skill, who } => {
            let Some(owner) = pick(*who, caster, target) else {
                return;
            };
            spawn_action(ctx, owner, *skill, target);
        }

        Effect::Log { text } => ctx.log.push(text.clone()),

        Effect::Contest(contest) => {
            let verdict =
                contest_verdict(contest, caster, target, skill_power, &ctx.reads, ctx.rng);
            for effect in outcome_effects(contest, verdict.outcome) {
                execute_with_power(effect, caster, target, verdict.power, ctx);
            }
        }

        Effect::Sequence(effects) => {
            for effect in effects {
                execute_with_power(effect, caster, target, skill_power, ctx);
            }
        }

        Effect::Conditional { cond, then, else_ } => {
            let branch = if eval_condition(cond, caster, target, ctx) {
                then
            } else {
                else_
            };
            execute_with_power(branch, caster, target, skill_power, ctx);
        }
    }
}

/// 求一个值（带当前强度）。
fn value_of<B: Blob>(
    value: &Value,
    caster: Entity,
    target: Option<Entity>,
    skill_power: f32,
    ctx: &EffectContext<'_, '_, '_, B>,
) -> f32 {
    let eval_ctx = eval_context(caster, target, skill_power, ctx);
    eval(value, &eval_ctx)
}

/// 判定一次对抗（随机源单独传入：这样"只读快照"与"可变随机源"不会互相借住）。
pub fn contest_verdict<B: Blob>(
    contest: &Contest,
    caster: Entity,
    target: Option<Entity>,
    skill_power: f32,
    reads: &B,
    rng: &mut CombatRng,
) -> crate::behaviors::contest::Verdict {
    let eval_ctx = EvalContext {
        caster_resources: reads.pools(caster),
        caster_stats: reads.stats(caster),
        target_resources: target.and_then(|target| reads.pools(target)),
        target_stats: target.and_then(|target| reads.stats(target)),
        skill_power,
    };
    resolve_contest(contest, &eval_ctx, rng)
}

/// 搭一份求值快照（双方池 + 双方属性 + 强度）。
fn eval_context<'a, B: Blob>(
    caster: Entity,
    target: Option<Entity>,
    skill_power: f32,
    ctx: &'a EffectContext<'_, '_, '_, B>,
) -> EvalContext<'a> {
    EvalContext {
        caster_resources: ctx.reads.pools(caster),
        caster_stats: ctx.reads.stats(caster),
        target_resources: target.and_then(|target| ctx.reads.pools(target)),
        target_stats: target.and_then(|target| ctx.reads.stats(target)),
        skill_power,
    }
}

/// 条件求值（宿主 = `Target`，兜底 `Caster`）。
fn eval_condition<B: Blob>(
    cond: &Condition,
    caster: Entity,
    target: Option<Entity>,
    ctx: &EffectContext<'_, '_, '_, B>,
) -> bool {
    match cond {
        Condition::Always => true,
        Condition::ResourceBelow { pool, ratio } => {
            let Some(entity) = pick(Who::Target, caster, target) else {
                return false;
            };
            ctx.reads
                .pools(entity)
                .is_some_and(|pools| pools.pool(*pool).is_some_and(|p| p.ratio() < *ratio))
        }
        Condition::HasStatus(status) => {
            let Some(entity) = pick(Who::Target, caster, target) else {
                return false;
            };
            let Some(&def_entity) = ctx.status_catalog.get(*status) else {
                return false;
            };
            find_status(ctx, entity, def_entity).is_some()
        }
        Condition::HasThreat => ctx.pending_threat.action.is_some(),
    }
}

/// `Who` → 具体实体（`Target` 缺失时退回 `Caster`）。
fn pick(who: Who, caster: Entity, target: Option<Entity>) -> Option<Entity> {
    match who {
        Who::Caster => Some(caster),
        Who::Target => target.or(Some(caster)),
    }
}

/// 取消一个实体当前的所有行动（`DispelAction` / `Interrupt` 共用）。
///
/// 行动实体由关系的反向集给出（`ActiveActions` 是 `InitiatedBy` 的反向集，Bevy 自动维护）。
fn cancel_actions<B: Blob>(ctx: &mut EffectContext<'_, '_, '_, B>, victim: Entity) {
    let mut targets: Vec<Entity> = {
        let Some(slots) = ctx.reads.active_actions() else {
            return;
        };
        slots
            .get(victim)
            .map(|slot| slot.actions().to_vec())
            .unwrap_or_default()
    };
    targets.dedup();
    for action in targets {
        ctx.commands.entity(action).despawn();
    }
}

/// 生成一个行动实例（`SpawnAction` 用）；瞬发技能打 `ResolveNow`，同帧结算。
pub fn spawn_action<B: Blob>(
    ctx: &mut EffectContext<'_, '_, '_, B>,
    owner: Entity,
    skill_id: SkillId,
    target: Option<Entity>,
) {
    let Some(&skill_entity) = ctx.skill_catalog.get(skill_id) else {
        return;
    };
    let duration = {
        let Some(skills) = ctx.reads.skills() else {
            return;
        };
        let Ok(skill) = skills.get(skill_entity) else {
            return;
        };
        skill.duration
    };
    let action = ctx
        .commands
        .spawn((
            Action {
                elapsed: 0.0,
                duration,
                target,
            },
            CastsSkill(skill_entity),
            InitiatedBy(owner),
        ))
        .id();
    if duration <= 0.0 {
        ctx.commands.entity(action).insert(ResolveNow);
    }
}
