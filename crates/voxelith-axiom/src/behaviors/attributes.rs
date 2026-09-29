//! L1 属性公式：**基础值 + 修饰符 → 最终值**。
//!
//! 为什么在 L1（**R11**）：它必须同时读到 L0 的 [`Attributes`]（基础值）与自己的
//! [`AttributeModifiers`]（修饰符），单个组件自己跑不通。
//!
//! - **R13**：不写 `Attributes`，只发 [`AttributeFinalMessage`] 让 L0 回写缓存。
//! - **缓存**：由脏消息驱动，不是每帧重算；`revision` 由 L0 维护，管线里没有"每帧重算一遍"。
//! - **定义一处**：修饰符按属性分槽（`[ModifierSet; AttributeId::COUNT]`），
//!   新增属性自动多一个槽位，不必改本模块。

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;

use crate::atoms::attribute::{
    AttributeBaseChangedMessage, AttributeFinalMessage, AttributeId, AttributeStage, Attributes,
};
use crate::behaviors::modifiers::{Modifier, ModifierCaps, ModifierSet, ModifierSource, evaluate};

/// 角色的属性修饰符槽位（本模块唯一写入口）。
#[derive(Component, Debug, Clone, PartialEq)]
pub struct AttributeModifiers([ModifierSet; AttributeId::COUNT]);

impl Default for AttributeModifiers {
    fn default() -> Self {
        Self(core::array::from_fn(|_| ModifierSet::default()))
    }
}

impl AttributeModifiers {
    /// 某个属性的修饰符槽位。
    pub fn slot(&self, attribute: AttributeId) -> &ModifierSet {
        &self.0[attribute.index()]
    }

    /// 某个属性的修饰符条数（调试 / 测试用）。
    pub fn slot_len(&self, attribute: AttributeId) -> usize {
        self.slot(attribute).len()
    }

    /// 所有属性槽位上的修饰符总数（调试 / 测试用）。
    pub fn len(&self) -> usize {
        self.0.iter().map(ModifierSet::len).sum()
    }

    /// 是否没有任何修饰符。
    pub fn is_empty(&self) -> bool {
        self.0.iter().all(ModifierSet::is_empty)
    }
}

/// 添加一条修饰符（来源写在 [`Modifier::source`] 里，便于之后按来源整体移除）。
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub struct AddAttributeModifierMessage {
    pub entity: Entity,
    pub attribute: AttributeId,
    pub modifier: Modifier,
}

/// 按来源移除该实体上**所有属性**的修饰符（装备卸下 / 被动失效 / 状态到期）。
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoveAttributeModifiersMessage {
    pub entity: Entity,
    pub source: ModifierSource,
}

/// 修饰符已变（L1 内部的脏信号，重算系统唯一消费）。
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttributeModifiersChangedMessage {
    pub entity: Entity,
}

/// 唯一写入口：改 [`AttributeModifiers`]，并广播脏信号。
///
/// 没有 `AttributeModifiers` 组件的实体（非角色实体）会被安全跳过。
pub fn apply_attribute_modifier_change(
    mut added: MessageReader<AddAttributeModifierMessage>,
    mut removed: MessageReader<RemoveAttributeModifiersMessage>,
    mut actors: Query<&mut AttributeModifiers>,
    mut changed: MessageWriter<AttributeModifiersChangedMessage>,
) {
    for message in added.read() {
        let Ok(mut modifiers) = actors.get_mut(message.entity) else {
            continue;
        };
        modifiers.0[message.attribute.index()].add(message.modifier);
        changed.write(AttributeModifiersChangedMessage {
            entity: message.entity,
        });
    }

    for message in removed.read() {
        let Ok(mut modifiers) = actors.get_mut(message.entity) else {
            continue;
        };
        let mut dirty = false;
        for slot in &mut modifiers.0 {
            dirty |= slot.remove_by_source(message.source);
        }
        if dirty {
            changed.write(AttributeModifiersChangedMessage {
                entity: message.entity,
            });
        }
    }
}

/// 公式：对每个属性做 `base + modifiers`（顺序固定、无随机）。
///
/// 只对脏实体重算；最终值原样通过消息交给 L0（**R13**）。
pub fn recompute_attribute_final(
    mut base_changed: MessageReader<AttributeBaseChangedMessage>,
    mut modifiers_changed: MessageReader<AttributeModifiersChangedMessage>,
    actors: Query<(&Attributes, Option<&AttributeModifiers>)>,
    caps: Res<ModifierCaps>,
    mut out: MessageWriter<AttributeFinalMessage>,
) {
    let mut dirty: Vec<Entity> = Vec::new();
    for message in base_changed.read() {
        dirty.push(message.entity);
    }
    for message in modifiers_changed.read() {
        dirty.push(message.entity);
    }
    dirty.sort_unstable();
    dirty.dedup();

    for entity in dirty {
        let Ok((attributes, modifiers)) = actors.get(entity) else {
            continue;
        };

        let mut values = *attributes.base();
        if let Some(modifiers) = modifiers {
            for id in AttributeId::ALL {
                values.set(
                    id,
                    evaluate(attributes.base().get(id), modifiers.slot(id), &caps),
                );
            }
        }

        out.write(AttributeFinalMessage {
            entity,
            values,
            revision: attributes.revision() + 1,
        });
    }
}

/// 注册 L1 属性公式，并把自己插进 L0 的固定阶段之间（`ChangeBase → RecomputeFinal → StoreFinal`）。
pub struct AttributeBehaviorPlugin;

impl Plugin for AttributeBehaviorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ModifierCaps>()
            .add_message::<AddAttributeModifierMessage>()
            .add_message::<RemoveAttributeModifiersMessage>()
            .add_message::<AttributeModifiersChangedMessage>()
            .add_systems(
                Update,
                (apply_attribute_modifier_change, recompute_attribute_final)
                    .chain()
                    .in_set(AttributeStage::RecomputeFinal),
            );
    }
}
