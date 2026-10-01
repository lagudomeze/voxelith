//! 三类"世界突变"的具体落点：改池、加 / 摘状态、维护状态槽。
//!
//! 它们都由效果执行器调用；共同纪律是**先算完数值、再动世界**
//! （写世界一律走 `Commands` 延迟应用）。

use bevy_ecs::prelude::*;

use crate::behaviors::content::ResourceId;
use crate::behaviors::status::{ActiveStatus, AttachedTo, Stacking};

use super::{Blob, EffectContext, execute_with_power};
/// 改资源池：`Resources::modify` 是**唯一写入口**，改完把整份组件写回实体。
///
/// 先克隆出来（借用在局部作用域内结束），再改、再写回，避免同时借 `ctx.reads` 与 `ctx.commands`。
pub(super) fn modify_resource<B: Blob>(
    ctx: &mut EffectContext<'_, '_, '_, B>,
    actor: Entity,
    pool: ResourceId,
    delta: f32,
) {
    let Some(current) = ctx.reads.pools(actor).cloned() else {
        return;
    };
    let mut updated = current;
    updated.modify(pool, delta);
    ctx.commands.entity(actor).insert(updated);
}

/// 施加状态：按 [`Stacking`] 刷新 / 叠层 / 忽略 / 替换，新建时跑 `on_apply`。
pub(super) fn apply_status<B: Blob>(
    ctx: &mut EffectContext<'_, '_, '_, B>,
    def_entity: Entity,
    host: Entity,
    source: Entity,
    duration: f32,
) {
    let duration = duration.max(0.0);
    let existing = find_status(ctx, host, def_entity);

    // 读定义（借用只在这一段里），拿够信息就放掉。
    let definition = {
        let defs = ctx.reads.status_defs();
        let Ok(definition) = defs.get(def_entity) else {
            return;
        };
        definition.clone()
    };

    match (definition.stacking, existing) {
        (Stacking::Ignore, Some(_)) => return,
        (Stacking::Refresh, Some(entity)) => {
            let current = {
                let statuses = ctx.reads.statuses();
                statuses.get(entity).ok().copied()
            };
            if let Some(mut updated) = current {
                updated.remaining = duration;
                ctx.commands.entity(entity).insert(updated);
            }
            sync_status_slot(ctx, host, entity);
            return;
        }
        (Stacking::Stack { max }, Some(entity)) => {
            let current = {
                let statuses = ctx.reads.statuses();
                statuses.get(entity).ok().copied()
            };
            if let Some(mut updated) = current {
                updated.stacks = updated.stacks.saturating_add(1).min(max);
                updated.remaining = duration;
                ctx.commands.entity(entity).insert(updated);
            }
            sync_status_slot(ctx, host, entity);
            return;
        }
        (Stacking::Replace, Some(entity)) => {
            ctx.commands.entity(entity).despawn();
        }
        _ => {}
    }

    let new_status = ctx
        .commands
        .spawn((
            ActiveStatus {
                def: def_entity,
                id: Some(definition.id),
                remaining: duration,
                stacks: 1,
                source,
                tick_accumulator: 0.0,
            },
            AttachedTo(host),
        ))
        .id();
    sync_status_slot(ctx, host, new_status);

    // `on_apply` 用同一套原语执行。
    for effect in &definition.on_apply {
        execute_with_power(effect, source, Some(host), 0.0, ctx);
    }
}

/// 移除状态：先跑 `on_remove`，再销毁实例。
pub(super) fn remove_status<B: Blob>(
    ctx: &mut EffectContext<'_, '_, '_, B>,
    host: Entity,
    def_entity: Entity,
) {
    let Some(entity) = find_status(ctx, host, def_entity) else {
        return;
    };
    let (definition, source) = {
        let defs = ctx.reads.status_defs();
        let statuses = ctx.reads.statuses();
        let Ok(definition) = defs.get(def_entity) else {
            return;
        };
        let source = statuses.get(entity).map_or(host, |status| status.source);
        (definition.clone(), source)
    };
    for effect in &definition.on_remove {
        execute_with_power(effect, source, Some(host), 0.0, ctx);
    }
    ctx.commands.entity(entity).despawn();
    unsync_status_slot(ctx, host, entity);
}

/// 目标身上是否已经有这种状态（返回实例实体）。
pub(super) fn find_status<B: Blob>(
    ctx: &EffectContext<'_, '_, '_, B>,
    host: Entity,
    def_entity: Entity,
) -> Option<Entity> {
    let state = {
        let states = ctx.reads.actor_states();
        states.get(host).ok()?.clone()
    };
    let statuses = ctx.reads.statuses();
    state.statuses.iter().copied().find(|&candidate| {
        statuses
            .get(candidate)
            .is_ok_and(|status| status.def == def_entity)
    })
}

/// 把实例登记进宿主的槽（关系组件 `AttachedTo` 也会做，这里保证同帧可见）。
fn sync_status_slot<B: Blob>(ctx: &mut EffectContext<'_, '_, '_, B>, host: Entity, status: Entity) {
    let current = {
        let states = ctx.reads.actor_states();
        states.get(host).ok().cloned()
    };
    let Some(mut updated) = current else {
        return;
    };
    if updated.statuses.contains(&status) {
        return;
    }
    updated.statuses.push(status);
    ctx.commands.entity(host).insert(updated);
}

/// 把实例从宿主的槽里摘掉。
fn unsync_status_slot<B: Blob>(
    ctx: &mut EffectContext<'_, '_, '_, B>,
    host: Entity,
    status: Entity,
) {
    let current = {
        let states = ctx.reads.actor_states();
        states.get(host).ok().cloned()
    };
    let Some(mut updated) = current else {
        return;
    };
    if !updated.statuses.contains(&status) {
        return;
    }
    updated.statuses.retain(|&candidate| candidate != status);
    ctx.commands.entity(host).insert(updated);
}
