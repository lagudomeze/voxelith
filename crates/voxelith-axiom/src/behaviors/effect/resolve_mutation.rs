//! **改动类效果**：改池 / 加状态 / 摘状态。
//!
//! 这三条是"世界真的变了"的落点，具体怎么写进世界由 [`super::apply`] 负责
//! （那是唯一碰 `Resources` / `ActiveStatus` 的地方）。这里只做**取值 + 挑人**：
//! 把 `Value` 求成数、把 `Who` 解成实体，然后交给落点。
//!
//! 拆出来的理由：它们和"取消行动 / 生成行动"（[`super::resolve_action`]）、
//! "递归元效果"（[`super::resolve_meta`]）是**三类完全不同的世界操作**，
//! 挤在同一个 `match` 里时，"改池"这件事的口子没有名字。

use bevy_ecs::prelude::*;

use crate::behaviors::content::{ResourceId, StatusId};
use crate::behaviors::value::{Value, Who};

use super::apply::{apply_status, modify_resource, remove_status};
use super::{Blob, EffectContext, pick, value_of};

/// `Effect::ModifyResource`：正数回复、负数伤害 / 消耗。
pub(super) fn modify_pool<B: Blob>(
    pool: ResourceId,
    delta: &Value,
    who: Who,
    caster: Entity,
    target: Option<Entity>,
    skill_power: f32,
    ctx: &mut EffectContext<'_, '_, '_, B>,
) {
    let Some(actor) = pick(who, caster, target) else {
        return;
    };
    let amount = value_of(delta, caster, target, skill_power, ctx);
    modify_resource(ctx, actor, pool, amount);
}

/// `Effect::ApplyStatus`：按叠加规则施加（未知状态写日志，不静默）。
pub(super) fn attach<B: Blob>(
    status: StatusId,
    duration: &Value,
    who: Who,
    caster: Entity,
    target: Option<Entity>,
    skill_power: f32,
    ctx: &mut EffectContext<'_, '_, '_, B>,
) {
    let Some(actor) = pick(who, caster, target) else {
        return;
    };
    let Some(&def_entity) = ctx.status_catalog.get(status) else {
        ctx.log
            .push(format!("未知状态 {status:?}（加载期应已拦截）"));
        return;
    };
    let seconds = value_of(duration, caster, target, skill_power, ctx);
    apply_status(ctx, def_entity, actor, caster, seconds);
}

/// `Effect::RemoveStatus`：跑 `on_remove` 再销毁实例。
pub(super) fn detach<B: Blob>(
    status: StatusId,
    who: Who,
    caster: Entity,
    target: Option<Entity>,
    ctx: &mut EffectContext<'_, '_, '_, B>,
) {
    let Some(actor) = pick(who, caster, target) else {
        return;
    };
    let Some(&def_entity) = ctx.status_catalog.get(status) else {
        return;
    };
    remove_status(ctx, actor, def_entity);
}
