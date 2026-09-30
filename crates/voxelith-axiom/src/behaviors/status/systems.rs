//! 状态的生命周期与判定系统。
//!
//! 分工：
//!
//! | 关注点 | 落在哪 |
//! |---|---|
//! | 是否命中、持续多久、能不能叠 | [`resolve_status_applications`]（用 [`ContestKind`] 的统一框架） |
//! | 免疫 | [`StatusImmunity`] 组件（判定前先查） |
//! | 每回合结算 | [`tick_status_timers`]（`Time` 驱动，发 [`StatusTickMessage`]） |
//! | 净化 | [`purge_statuses`]（despawn 实例实体） |
//! | 到期 | [`StatusExpiredEvent`]（`EntityEvent`，内容层 / 表现层 observe） |
//!
//! 状态实例是**独立子实体**（`ChildOf(宿主)`）：宿主销毁会被层级语义自动带走，
//! 净化 = despawn，作用域化监听也可以直接绑到实例实体上。

use core::time::Duration;

use bevy_ecs::prelude::*;
use bevy_time::Time;

use super::contest::{ContestParams, contest_chance, contest_duration};
use super::{Stacking, StatusConfig, StatusId, StatusImmunity, StatusInstance, StatusRegistry};
use crate::atoms::stats::Stat;
use crate::behaviors::rolls::CombatRng;

/// 请求施加状态（**Message**）。
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub struct ApplyStatusRequest {
    /// 施加者。
    pub source: Entity,
    /// 目标。
    pub target: Entity,
    /// 想施加的状态。
    pub status: StatusId,
    /// 强度系数（内容层用来表达"这一击更强"）。
    pub power: f32,
}

/// 状态判定结果（**Message**）：表现层 / UI / 日志监听它。
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusResolvedMessage {
    /// 目标。
    pub target: Entity,
    /// 状态。
    pub status: StatusId,
    /// 是否施加成功。
    pub applied: bool,
    /// 施加后的层数（失败为 0）。
    pub stacks: u8,
    /// 判定出的持续时间（失败为零）。
    pub duration: Duration,
    /// 失败原因。
    pub reason: Option<StatusRejectReason>,
}

/// 判定拒绝原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusRejectReason {
    /// 内容层没定义这个状态（加载期漏配）。
    Undefined,
    /// 目标免疫。
    Immune,
    /// 判定失败（攻防强度差）。
    ContestFailed,
    /// 已达最大层数 / 唯一状态已存在。
    MaxStacks,
    /// 被净化。
    Purged,
}

/// 净化请求（**Message**）：`filter` 为 `None` 表示清掉目标身上所有状态。
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct PurgeStatusMessage {
    /// 目标。
    pub target: Entity,
    /// 只净化这一种状态（`None` = 全部）。
    pub filter: Option<StatusId>,
}

/// 每回合结算通知（**Message**）：内容层按 `on_tick` 行为响应（例如 DOT 伤害）。
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusTickMessage {
    /// 宿主。
    pub host: Entity,
    /// 状态。
    pub status: StatusId,
    /// 当前层数。
    pub stacks: u8,
}

/// 状态结束（**EntityEvent**，target = 状态实例实体）：到期或被净化。
#[derive(EntityEvent, Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusExpiredEvent {
    /// 结束的状态实例实体。
    #[event_target]
    pub entity: Entity,
    /// 宿主。
    pub host: Entity,
    /// 状态。
    pub status: StatusId,
}

fn rejected(request: &ApplyStatusRequest, reason: StatusRejectReason) -> StatusResolvedMessage {
    StatusResolvedMessage {
        target: request.target,
        status: request.status,
        applied: false,
        stacks: 0,
        duration: Duration::ZERO,
        reason: Some(reason),
    }
}

/// 判定并施加状态：**伤害之后独立发起**，与伤害管线互不干涉。
///
/// 顺序：查定义 → 查免疫 → 攻防强度判定（[`contest_chance`]）→ 时长（[`contest_duration`]）
/// → 叠加 / 刷新 → 建实例（独立子实体 + `ChildOf(宿主)`）。
#[allow(clippy::too_many_arguments)]
pub fn resolve_status_applications(
    mut requests: MessageReader<ApplyStatusRequest>,
    registry: Res<StatusRegistry>,
    params: Res<ContestParams>,
    config: Res<StatusConfig>,
    mut rng: ResMut<CombatRng>,
    stats: Query<&Stat>,
    immunities: Query<&StatusImmunity>,
    instances: Query<(Entity, &StatusInstance)>,
    mut commands: Commands,
    mut resolved: MessageWriter<StatusResolvedMessage>,
) {
    for request in requests.read() {
        // 1) 定义
        let Some(def) = registry.get(request.status).copied() else {
            resolved.write(rejected(request, StatusRejectReason::Undefined));
            continue;
        };

        // 2) 免疫
        if immunities
            .get(request.target)
            .is_ok_and(|immunity| immunity.is_immune(request.status))
        {
            resolved.write(rejected(request, StatusRejectReason::Immune));
            continue;
        }

        // 3) 攻防强度判定（物理 / 法术 / 精神共用同一套公式）
        let (offense, defense) = match (stats.get(request.source), stats.get(request.target)) {
            (Ok(attacker), Ok(defender)) => {
                let (offense, defense) = def.contest.scores(attacker, defender);
                (offense * request.power.max(0.0), defense)
            }
            _ => (0.0, 0.0),
        };
        if !rng.chance(contest_chance(offense, defense, &params)) {
            resolved.write(rejected(request, StatusRejectReason::ContestFailed));
            continue;
        }

        // 4) 时长（夹在配置上限内，避免无限控）
        let duration =
            contest_duration(def.base_duration, offense, defense, &params).min(config.max_duration);

        // 5) 叠加 / 刷新
        let existing = instances.iter().find(|(_, instance)| {
            instance.host == request.target && instance.status == request.status
        });

        let stacks = match (def.stacking, existing) {
            (_, None) => {
                commands.spawn((
                    StatusInstance::new(request.target, request.source, request.status, duration),
                    ChildOf(request.target),
                ));
                1
            }
            (Stacking::Unique, Some(_)) => {
                resolved.write(rejected(request, StatusRejectReason::MaxStacks));
                continue;
            }
            (Stacking::Stack, Some((entity, instance))) => {
                if instance.stacks >= def.max_stacks {
                    resolved.write(rejected(request, StatusRejectReason::MaxStacks));
                    continue;
                }
                let stacks = instance.stacks + 1;
                commands.entity(entity).insert(StatusInstance {
                    stacks,
                    remaining: duration,
                    tick_accumulator: Duration::ZERO,
                    ..*instance
                });
                stacks
            }
            (Stacking::Refresh, Some((entity, instance))) => {
                commands.entity(entity).insert(StatusInstance {
                    remaining: duration,
                    tick_accumulator: Duration::ZERO,
                    ..*instance
                });
                instance.stacks.max(1)
            }
        };

        resolved.write(StatusResolvedMessage {
            target: request.target,
            status: request.status,
            applied: true,
            stacks,
            duration,
            reason: None,
        });
    }
}

/// 净化：despawn 状态实例实体，并广播 [`StatusResolvedMessage`]（`Purged`）。
pub fn purge_statuses(
    mut requests: MessageReader<PurgeStatusMessage>,
    instances: Query<(Entity, &StatusInstance)>,
    mut commands: Commands,
    mut resolved: MessageWriter<StatusResolvedMessage>,
) {
    for request in requests.read() {
        for (entity, instance) in instances.iter() {
            if instance.host != request.target {
                continue;
            }
            if request
                .filter
                .is_some_and(|status| status != instance.status)
            {
                continue;
            }
            commands.entity(entity).despawn();
            resolved.write(StatusResolvedMessage {
                target: request.target,
                status: instance.status,
                applied: false,
                stacks: 0,
                duration: Duration::ZERO,
                reason: Some(StatusRejectReason::Purged),
            });
        }
    }
}

/// 推进状态计时：每 [`StatusConfig::tick_interval`] 发一次结算通知，归零则到期并销毁实例。
///
/// 到期用 [`StatusExpiredEvent`] 通知（内容层 observe 跑 `on_expire` 行为，表现层放特效）。
pub fn tick_status_timers(
    time: Res<Time>,
    config: Res<StatusConfig>,
    mut commands: Commands,
    mut instances: Query<(Entity, &mut StatusInstance)>,
    mut ticks: MessageWriter<StatusTickMessage>,
) {
    let delta = time.delta();

    for (entity, mut instance) in &mut instances {
        if instance.remaining <= delta {
            commands.entity(entity).despawn();
            commands.trigger(StatusExpiredEvent {
                entity,
                host: instance.host,
                status: instance.status,
            });
            continue;
        }

        instance.remaining -= delta;
        instance.tick_accumulator += delta;
        if instance.tick_accumulator >= config.tick_interval {
            instance.tick_accumulator -= config.tick_interval;
            ticks.write(StatusTickMessage {
                host: instance.host,
                status: instance.status,
                stacks: instance.stacks,
            });
        }
    }
}
