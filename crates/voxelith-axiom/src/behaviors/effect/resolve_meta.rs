//! **元效果**：自己不改世界，只决定"接下来执行什么"。
//!
//! | 效果 | 干什么 |
//! |---|---|
//! | `Contest` | 判定一次对抗，按结果跑对应的那组效果（**递归**） |
//! | `Sequence` | 顺序跑一串（**递归**） |
//! | `Conditional` | 条件成立走 `then`，否则走 `else_`（**递归**） |
//!
//! 递归的入口是 [`super::execute_with_power`]。它带着**强度**（`skill_power`）往下传：
//! `Contest` 是强度的来源，所以它之后的分支能拿到"强多少"当数值用（见 combat-design §5）。
//!
//! 这也是"效果原语递归组合"那句话的落点：技能不是一段代码，是一棵可以套的树。

use bevy_ecs::prelude::*;

use crate::behaviors::contest::{CombatRng, Contest, outcome_effects, resolve_contest};
use crate::behaviors::requirement::Condition;
use crate::behaviors::value::{EvalContext, Who};

use super::apply::find_status;
use super::{Blob, EffectContext, execute_with_power, pick};

/// `Effect::Contest`：判定 → 跑匹配的那组效果，并把本次强度传下去。
pub(super) fn contest<B: Blob>(
    contest: &Contest,
    caster: Entity,
    target: Option<Entity>,
    skill_power: f32,
    ctx: &mut EffectContext<'_, '_, '_, B>,
) {
    let verdict = contest_verdict(contest, caster, target, skill_power, &ctx.reads, ctx.rng);
    for effect in outcome_effects(contest, verdict.outcome) {
        execute_with_power(effect, caster, target, verdict.power, ctx);
    }
}

/// `Effect::Sequence`：按顺序全部跑一遍（强度不变）。
pub(super) fn sequence<B: Blob>(
    effects: &[crate::behaviors::effect::Effect],
    caster: Entity,
    target: Option<Entity>,
    skill_power: f32,
    ctx: &mut EffectContext<'_, '_, '_, B>,
) {
    for effect in effects {
        execute_with_power(effect, caster, target, skill_power, ctx);
    }
}

/// `Effect::Conditional`：按条件选一支跑（两边都写全，避免"忘了写 else"）。
pub(super) fn conditional<B: Blob>(
    cond: &Condition,
    then: &crate::behaviors::effect::Effect,
    else_: &crate::behaviors::effect::Effect,
    caster: Entity,
    target: Option<Entity>,
    skill_power: f32,
    ctx: &mut EffectContext<'_, '_, '_, B>,
) {
    let branch = if eval_condition(cond, caster, target, ctx) {
        then
    } else {
        else_
    };
    execute_with_power(branch, caster, target, skill_power, ctx);
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
        Condition::HasThreat => ctx.window.is_open(),
    }
}
