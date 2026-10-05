//! **行动类效果**：取消行动 / 打断 / 生成行动。
//!
//! 反制的核心就在这一格：`DispelAction` 取消的是"即将生效的 action"，
//! 不是怪物这个实体（见 [docs/combat-design.md](../../../../../docs/combat-design.md) D5）。
//!
//! 行动实体从哪里找？**从关系的反向集**：`ActiveActions` 是 `InitiatedBy` 的反向集，
//! Bevy 自动维护——所以"取消某人的行动"不需要自建一张表。

use bevy_ecs::prelude::*;

use crate::atoms::action::{Action, CastsSkill, InitiatedBy, ResolveNow};
use crate::behaviors::content::SkillId;
use crate::behaviors::value::Who;

use super::{Blob, EffectContext, pick};

/// `Effect::DispelAction`：取消"即将生效"的那条行动，并清掉威胁登记。
///
/// `Who::Target` **优先解析为怪物当前的行动实体**（`pending_threat.action`），
/// 无威胁时退回目标本身——反制要取消的是行动，不是怪物。
pub(super) fn dispel<B: Blob>(
    who: Who,
    caster: Entity,
    target: Option<Entity>,
    ctx: &mut EffectContext<'_, '_, '_, B>,
) {
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

/// `Effect::Interrupt`：清空目标的行动槽（**不**取消威胁来源）。
pub(super) fn interrupt<B: Blob>(
    who: Who,
    caster: Entity,
    target: Option<Entity>,
    ctx: &mut EffectContext<'_, '_, '_, B>,
) {
    if let Some(victim) = pick(who, caster, target) {
        cancel_actions(ctx, victim);
    }
}

/// `Effect::SpawnAction`：给某人排一个行动（瞬发的同帧结算）。
pub(super) fn spawn<B: Blob>(
    skill: SkillId,
    who: Who,
    caster: Entity,
    target: Option<Entity>,
    ctx: &mut EffectContext<'_, '_, '_, B>,
) {
    let Some(owner) = pick(who, caster, target) else {
        return;
    };
    spawn_action(ctx, owner, skill, target);
}

/// 取消一个实体当前的所有行动（`DispelAction` / `Interrupt` 共用）。
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
