//! L0 属性的**唯一写入口**集合。
//!
//! - **R8**：本模块每个系统只查询 [`Attributes`]，不跨组件查询。
//! - **R13**：L1 不能直接写 `Attributes`，只能发消息；公式在 L1（`behaviors::attributes`）。
//! - **R56 的精神**：字段私有，写入只发生在下面这几个 `apply_*` 系统里；
//!   每一次数值变化都对应一条消息，因此可追踪、可回放。

use bevy_ecs::prelude::*;

use super::defs::{AttributeId, AttributeValues};

/// 角色的属性数值。
///
/// 字段私有：`base`（永久值）、`allocated`（玩家分配，洗点依据）、`cached`（最终值缓存）、
/// `unspent`（未分配点）、`revision`（缓存版本号）。
#[derive(Component, Debug, Clone, Default)]
pub struct Attributes {
    base: AttributeValues,
    allocated: AttributeValues,
    cached: AttributeValues,
    unspent: f32,
    revision: u64,
}

impl Attributes {
    /// 用初始基础值与未分配点数构造；最终值缓存初始化为基础值。
    pub fn new(base: AttributeValues, unspent: f32) -> Self {
        Self {
            cached: base,
            base,
            allocated: AttributeValues::default(),
            unspent,
            revision: 1,
        }
    }

    /// 永久基础值（初始 + 成长 + 玩家分配）。
    pub fn base(&self) -> &AttributeValues {
        &self.base
    }

    /// 玩家主动分配的点数（洗点依据）。
    pub fn allocated(&self) -> &AttributeValues {
        &self.allocated
    }

    /// 最终值缓存：L1 公式算完回写，**不必每帧重算**。
    pub fn cached(&self) -> &AttributeValues {
        &self.cached
    }

    /// 未分配点数。
    pub fn unspent(&self) -> f32 {
        self.unspent
    }

    /// 缓存版本号：每次回写 +1；相同即"没有新的最终值"。
    pub fn revision(&self) -> u64 {
        self.revision
    }
}

impl AsRef<AttributeValues> for Attributes {
    /// 默认读到的是**最终值**（读方不需要知道 base / allocated 的存在）。
    fn as_ref(&self) -> &AttributeValues {
        &self.cached
    }
}

/// 加点：消耗未分配点数，并记入"已分配"。
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub struct AttributeAllocationMessage {
    pub entity: Entity,
    pub attribute: AttributeId,
    pub amount: f32,
}

/// 成长：升级等永久提升，**不消耗**未分配点数（洗点不退回）。
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub struct AttributeGrowthMessage {
    pub entity: Entity,
    pub attribute: AttributeId,
    pub amount: f32,
}

/// 洗点：退回全部已分配点数，成长保留。
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttributeRespecMessage {
    pub entity: Entity,
}

/// 基础值已变：发给 L1 让它重算最终值（L0 自己不算公式）。
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttributeBaseChangedMessage {
    pub entity: Entity,
}

/// L1 算好的最终值：L0 只负责写进缓存（唯一写缓存入口）。
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub struct AttributeFinalMessage {
    pub entity: Entity,
    pub values: AttributeValues,
    pub revision: u64,
}

/// 加点：点数不足或数值非法时整条消息被忽略（不部分生效）。
pub fn apply_attribute_allocation(
    mut messages: MessageReader<AttributeAllocationMessage>,
    mut actors: Query<&mut Attributes>,
    mut changed: MessageWriter<AttributeBaseChangedMessage>,
) {
    for message in messages.read() {
        let Ok(mut attributes) = actors.get_mut(message.entity) else {
            continue;
        };
        if message.amount <= 0.0
            || !message.amount.is_finite()
            || attributes.unspent < message.amount
        {
            continue;
        }
        attributes.base.add(message.attribute, message.amount);
        attributes.allocated.add(message.attribute, message.amount);
        attributes.unspent -= message.amount;
        changed.write(AttributeBaseChangedMessage {
            entity: message.entity,
        });
    }
}

/// 成长：升级等来源的永久提升，不碰 `unspent` / `allocated`。
pub fn apply_attribute_growth(
    mut messages: MessageReader<AttributeGrowthMessage>,
    mut actors: Query<&mut Attributes>,
    mut changed: MessageWriter<AttributeBaseChangedMessage>,
) {
    for message in messages.read() {
        let Ok(mut attributes) = actors.get_mut(message.entity) else {
            continue;
        };
        if message.amount <= 0.0 || !message.amount.is_finite() {
            continue;
        }
        attributes.base.add(message.attribute, message.amount);
        changed.write(AttributeBaseChangedMessage {
            entity: message.entity,
        });
    }
}

/// 洗点：只退 `allocated`，成长（`base - allocated`）保留。
pub fn apply_attribute_respec(
    mut messages: MessageReader<AttributeRespecMessage>,
    mut actors: Query<&mut Attributes>,
    mut changed: MessageWriter<AttributeBaseChangedMessage>,
) {
    for message in messages.read() {
        let Ok(mut attributes) = actors.get_mut(message.entity) else {
            continue;
        };
        let mut refunded = 0.0;
        for id in AttributeId::ALL {
            let allocated = attributes.allocated.get(id);
            if allocated != 0.0 {
                attributes.base.add(id, -allocated);
                refunded += allocated;
            }
        }
        attributes.allocated = AttributeValues::default();
        attributes.unspent += refunded;
        changed.write(AttributeBaseChangedMessage {
            entity: message.entity,
        });
    }
}

/// 存最终值：只接受比当前更新的 `revision`（防止乱序消息回退缓存）。
pub fn apply_attribute_final(
    mut messages: MessageReader<AttributeFinalMessage>,
    mut actors: Query<&mut Attributes>,
) {
    for message in messages.read() {
        let Ok(mut attributes) = actors.get_mut(message.entity) else {
            continue;
        };
        if message.revision <= attributes.revision {
            continue;
        }
        attributes.cached = message.values;
        attributes.revision = message.revision;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atoms::attribute::{AttributeFinalMessage, AttributePlugin};
    use crate::behaviors::attributes::AttributeBehaviorPlugin;
    use bevy_app::prelude::*;

    fn spawn_actor(app: &mut App, strength: f32) -> Entity {
        let mut base = AttributeValues::default();
        base.set(AttributeId::Strength, strength);
        app.world_mut().spawn(Attributes::new(base, 0.0)).id()
    }

    /// 没有消息 → 不重算：直接改 base（只有本模块能这么干）也不会被公式覆盖。
    #[test]
    fn final_cache_is_only_written_by_message() {
        let mut app = App::new();
        app.add_plugins(AttributePlugin);
        app.add_plugins(AttributeBehaviorPlugin);
        let entity = spawn_actor(&mut app, 10.0);

        app.update();
        let revision_after_setup = app.world().get::<Attributes>(entity).unwrap().revision();

        {
            let mut attributes = app.world_mut().get_mut::<Attributes>(entity).unwrap();
            attributes.base.add(AttributeId::Strength, 5.0);
        }
        app.update();

        let attributes = app.world().get::<Attributes>(entity).unwrap();
        assert_eq!(
            attributes.cached().get(AttributeId::Strength),
            10.0,
            "没有脏消息就不应重算最终值"
        );
        assert_eq!(attributes.revision(), revision_after_setup);
    }

    /// 乱序（更旧）的最终值消息不得回退缓存。
    #[test]
    fn stale_final_message_is_ignored() {
        let mut app = App::new();
        app.add_plugins(AttributePlugin);
        let entity = spawn_actor(&mut app, 10.0);
        app.update();

        let mut values = AttributeValues::default();
        values.set(AttributeId::Strength, 999.0);
        app.world_mut()
            .resource_mut::<Messages<AttributeFinalMessage>>()
            .write(AttributeFinalMessage {
                entity,
                values,
                revision: 0,
            });
        app.update();

        assert_eq!(
            app.world()
                .get::<Attributes>(entity)
                .unwrap()
                .cached()
                .get(AttributeId::Strength),
            10.0,
            "revision 不大于当前值的最终值消息应被忽略"
        );
    }

    #[test]
    fn messages_for_unknown_entities_are_ignored() {
        let mut app = App::new();
        app.add_plugins(AttributePlugin);
        let ghost = app.world_mut().spawn_empty().id();

        app.world_mut()
            .resource_mut::<Messages<AttributeAllocationMessage>>()
            .write(AttributeAllocationMessage {
                entity: ghost,
                attribute: AttributeId::Strength,
                amount: 1.0,
            });
        app.update();

        assert!(app.world().get::<Attributes>(ghost).is_none());
    }
}
