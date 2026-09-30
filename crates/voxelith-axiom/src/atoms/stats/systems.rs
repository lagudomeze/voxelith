//! L0 属性的变更入口：消息 + **唯一写路径**。
//!
//! 每条系统都只查询 [`Stat`]（**R8**），改完账本 / 修饰符后调用 [`Stat::refresh`]：
//! 最终值在同一帧内就是新的，所以**没有**"基础值变了""最终值算好了"这类来回消息。
//!
//! 消息分两类：
//!
//! - **变更指令**（外部发进来）：加点、发点数、洗点、加 / 删修饰符；
//! - **结果广播**（本模块发出去）：加点成功 / 失败，供 UI、日志、表现监听。

use bevy_ecs::prelude::*;

use crate::utils::Key;

use super::StatConfig;
use super::stat::{Modifier, ModifierCaps, Stat, StatError, StatId};

/// UI / 内容层请求加点。
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub struct AllocateStatRequest {
    /// 目标实体。
    pub entity: Entity,
    /// 想加点的属性。
    pub stat: StatId,
    /// 想加的点数。
    pub amount: f32,
}

/// 发放可分配点数（升级 / 任务奖励）。
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub struct GrantStatPointsMessage {
    /// 目标实体。
    pub entity: Entity,
    /// 发放的点数。
    pub amount: f32,
}

/// 洗点：退回全部已分配点数。
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct RespecStatsMessage {
    /// 目标实体。
    pub entity: Entity,
}

/// 添加一条修饰符（装备 / 被动 / 状态派生）。
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub struct AddStatModifierMessage {
    /// 目标实体。
    pub entity: Entity,
    /// 修饰哪个属性。
    pub stat: StatId,
    /// 修饰符本体（来源写在 `modifier.source` 里）。
    pub modifier: Modifier,
}

/// 按来源移除该实体上**所有属性**的修饰符（装备卸下 / 被动失效 / 状态到期）。
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoveStatModifiersMessage {
    /// 目标实体。
    pub entity: Entity,
    /// 修饰哪个属性。
    pub stat: StatId,
    /// 要移除的key。
    pub key: Key,
}

/// 加点成功（UI 可以用来做数字滚动 / 音效）。
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub struct StatAllocatedMessage {
    /// 目标实体。
    pub entity: Entity,
    /// 被加点的属性。
    pub stat: StatId,
    /// 本次加的点数。
    pub amount: f32,
}

/// 加点失败（UI 监听这条决定弹什么提示）。
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub struct StatAllocationFailedMessage {
    /// 目标实体。
    pub entity: Entity,
    /// 想加点的属性。
    pub stat: StatId,
    /// 想加的点数。
    pub amount: f32,
    /// 结构化原因。
    pub reason: StatError,
}

/// 发放可分配点数。
pub fn apply_stat_point_grant(
    mut messages: MessageReader<GrantStatPointsMessage>,
    mut stats: Query<&mut Stat>,
    _config: Res<StatConfig>,
    caps: Res<ModifierCaps>,
) {
    for message in messages.read() {
        let Ok(mut stat) = stats.get_mut(message.entity) else {
            continue;
        };
        stat.add_points(message.amount);
        stat.refresh(&caps);
    }
}

/// 加点：成功发 [`StatAllocatedMessage`]，失败发 [`StatAllocationFailedMessage`]。
pub fn apply_stat_allocation(
    mut requests: MessageReader<AllocateStatRequest>,
    mut stats: Query<&mut Stat>,
    _config: Res<StatConfig>,
    caps: Res<ModifierCaps>,
    mut allocated: MessageWriter<StatAllocatedMessage>,
    mut failed: MessageWriter<StatAllocationFailedMessage>,
) {
    for request in requests.read() {
        let Ok(mut stat) = stats.get_mut(request.entity) else {
            continue;
        };

        match stat.allocate(request.stat, request.amount) {
            Ok(()) => {
                stat.refresh(&caps);
                allocated.write(StatAllocatedMessage {
                    entity: request.entity,
                    stat: request.stat,
                    amount: request.amount,
                });
            }
            Err(error) => {
                failed.write(StatAllocationFailedMessage {
                    entity: request.entity,
                    stat: request.stat,
                    amount: request.amount,
                    reason: *error,
                });
            }
        }
    }
}

/// 洗点：退 `allocated`（初始值不动）。
pub fn apply_stat_respec(
    mut messages: MessageReader<RespecStatsMessage>,
    mut stats: Query<&mut Stat>,
    _config: Res<StatConfig>,
    caps: Res<ModifierCaps>,
) {
    for message in messages.read() {
        let Ok(mut stat) = stats.get_mut(message.entity) else {
            continue;
        };
        stat.respec();
        stat.refresh(&caps);
    }
}

/// 添加修饰符。
pub fn apply_stat_modifier_add(
    mut messages: MessageReader<AddStatModifierMessage>,
    mut stats: Query<&mut Stat>,
    _config: Res<StatConfig>,
    caps: Res<ModifierCaps>,
) {
    for message in messages.read() {
        let Ok(mut stat) = stats.get_mut(message.entity) else {
            continue;
        };
        stat.add_modifier(message.stat, message.modifier);
        stat.refresh(&caps);
    }
}

/// 按来源移除修饰符（没移除到任何东西就不重算）。
pub fn apply_stat_modifier_remove(
    mut messages: MessageReader<RemoveStatModifiersMessage>,
    mut stats: Query<&mut Stat>,
    _config: Res<StatConfig>,
    caps: Res<ModifierCaps>,
) {
    for message in messages.read() {
        let Ok(mut stat) = stats.get_mut(message.entity) else {
            continue;
        };
        if stat.remove_modifier(message.stat, message.key).is_some() {
            stat.refresh(&caps);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atoms::stats::{DEFAULT_STAT, StatBlock, StatPlugin};
    use bevy_app::prelude::*;

    /// 收集结果消息（observer 不利于断言，用普通系统 + Resource）。
    #[derive(Resource, Default)]
    struct Capture {
        allocated: Vec<StatAllocatedMessage>,
        failed: Vec<StatAllocationFailedMessage>,
    }

    fn capture_messages(
        mut allocated: MessageReader<StatAllocatedMessage>,
        mut failed: MessageReader<StatAllocationFailedMessage>,
        mut capture: ResMut<Capture>,
    ) {
        for message in allocated.read() {
            capture.allocated.push(*message);
        }
        for message in failed.read() {
            capture.failed.push(*message);
        }
    }

    fn test_app() -> App {
        let mut app = App::new();
        app.add_plugins(StatPlugin);
        app.init_resource::<Capture>();
        app.add_systems(Update, capture_messages.after(apply_stat_allocation));
        app
    }

    fn spawn_actor(app: &mut App, strength: f32, unspent: f32) -> Entity {
        let base = StatBlock {
            strength,
            ..StatBlock::default()
        };
        let entity = app.world_mut().spawn(Stat::from_base(base)).id();
        if unspent > 0.0 {
            app.world_mut()
                .resource_mut::<Messages<GrantStatPointsMessage>>()
                .write(GrantStatPointsMessage {
                    entity,
                    amount: unspent,
                });
            app.update();
        }
        entity
    }

    /// 加点在同一帧内就刷新了最终值（没有"回写最终值"的消息）。
    #[test]
    fn allocation_refreshes_cached_in_the_same_frame() {
        let mut app = test_app();
        let entity = spawn_actor(&mut app, 10.0, 5.0);

        app.world_mut()
            .resource_mut::<Messages<AllocateStatRequest>>()
            .write(AllocateStatRequest {
                entity,
                stat: StatId::Strength,
                amount: 3.0,
            });
        app.update();

        let stat = app.world().get::<Stat>(entity).unwrap();
        assert_eq!(stat.get(StatId::Strength), 13.0);
        assert_eq!(stat.allocated().get(StatId::Strength), 3.0);
        assert_eq!(stat.unspent_points(), 2.0);
    }

    /// 发点数只动"未分配"，不改最终值。
    #[test]
    fn granting_points_does_not_change_the_final_value() {
        let mut app = test_app();
        let entity = spawn_actor(&mut app, 10.0, 0.0);

        app.world_mut()
            .resource_mut::<Messages<GrantStatPointsMessage>>()
            .write(GrantStatPointsMessage {
                entity,
                amount: 5.0,
            });
        app.update();

        let stat = app.world().get::<Stat>(entity).unwrap();
        assert_eq!(stat.unspent_points(), 5.0);
        assert_eq!(stat.get(StatId::Strength), 10.0);
    }

    /// 加点失败要发失败消息，而不是静默丢弃。
    #[test]
    fn failed_allocation_emits_a_failure_message() {
        let mut app = test_app();
        let entity = spawn_actor(&mut app, 10.0, 0.0);

        app.world_mut()
            .resource_mut::<Messages<AllocateStatRequest>>()
            .write(AllocateStatRequest {
                entity,
                stat: StatId::Strength,
                amount: 3.0,
            });
        app.update();

        let capture = app.world().resource::<Capture>();
        assert_eq!(capture.failed.len(), 1);
        assert!(capture.allocated.is_empty());
        assert_eq!(
            capture.failed[0].reason,
            StatError::NotEnoughPoints {
                required: 3.0,
                available: 0.0
            }
        );
    }

    /// 加点成功要发成功消息。
    #[test]
    fn successful_allocation_emits_a_success_message() {
        let mut app = test_app();
        let entity = spawn_actor(&mut app, DEFAULT_STAT, 2.0);

        app.world_mut()
            .resource_mut::<Messages<AllocateStatRequest>>()
            .write(AllocateStatRequest {
                entity,
                stat: StatId::Dexterity,
                amount: 2.0,
            });
        app.update();

        let capture = app.world().resource::<Capture>();
        assert_eq!(capture.allocated.len(), 1);
        assert_eq!(
            app.world()
                .get::<Stat>(entity)
                .unwrap()
                .allocated()
                .get(StatId::Dexterity),
            2.0
        );
    }

    /// 洗点退回全部分配点数，初始值不动。
    #[test]
    fn respec_refunds_allocated_only() {
        let mut app = test_app();
        let entity = spawn_actor(&mut app, 12.0, 4.0);

        app.world_mut()
            .resource_mut::<Messages<AllocateStatRequest>>()
            .write(AllocateStatRequest {
                entity,
                stat: StatId::Strength,
                amount: 4.0,
            });
        app.update();
        assert_eq!(app.world().get::<Stat>(entity).unwrap().strength, 16.0);

        app.world_mut()
            .resource_mut::<Messages<RespecStatsMessage>>()
            .write(RespecStatsMessage { entity });
        app.update();

        let stat = app.world().get::<Stat>(entity).unwrap();
        assert_eq!(stat.strength, 12.0);
        assert_eq!(stat.unspent_points(), 4.0);
    }

    /// 未知实体（没有 `Stat`）的消息被安全忽略。
    #[test]
    fn messages_for_unknown_entities_are_ignored() {
        let mut app = test_app();
        let ghost = app.world_mut().spawn_empty().id();

        app.world_mut()
            .resource_mut::<Messages<AllocateStatRequest>>()
            .write(AllocateStatRequest {
                entity: ghost,
                stat: StatId::Strength,
                amount: 1.0,
            });
        app.update();

        assert!(app.world().get::<Stat>(ghost).is_none());
    }
}
