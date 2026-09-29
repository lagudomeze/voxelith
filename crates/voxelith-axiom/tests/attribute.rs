//! 属性机制的端到端行为测试：**L0 只存值 + L1 算公式 + 消息驱动缓存**。
//!
//! 这些测试固化三条设计不变量（见 `docs/combat-mechanics.md`）：
//!
//! - **I2（L1 算、L0 存）**：公式在 L1，最终值仍然由 L0 的唯一写入口落到组件上。
//! - **I5（一个组件一个写入口）**：加点/成长/洗点全部走消息，测试无法绕过去直接写字段。
//! - 缓存由**脏消息**驱动：没有消息就没有重算（`revision` 不变）。

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use voxelith_axiom::atoms::attribute::{
    AttributeAllocationMessage, AttributeGrowthMessage, AttributeId, AttributePlugin,
    AttributeRespecMessage, AttributeValues, Attributes,
};
use voxelith_axiom::behaviors::attributes::{
    AddAttributeModifierMessage, AttributeBehaviorPlugin, AttributeModifiers,
    RemoveAttributeModifiersMessage,
};
use voxelith_axiom::behaviors::modifiers::{Modifier, ModifierSource};

fn test_app() -> App {
    let mut app = App::new();
    app.add_plugins(AttributePlugin);
    app.add_plugins(AttributeBehaviorPlugin);
    app
}

fn spawn_actor(app: &mut App, strength: f32, unspent: f32) -> Entity {
    let mut base = AttributeValues::default();
    base.set(AttributeId::Strength, strength);
    app.world_mut()
        .spawn((
            Attributes::new(base, unspent),
            AttributeModifiers::default(),
        ))
        .id()
}

fn cached(app: &App, entity: Entity, id: AttributeId) -> f32 {
    app.world()
        .get::<Attributes>(entity)
        .expect("实体应带 Attributes")
        .cached()
        .get(id)
}

fn revision(app: &App, entity: Entity) -> u64 {
    app.world().get::<Attributes>(entity).unwrap().revision()
}

/// 造一个"装备 / 被动"来源实体（来源只用到 `Entity`，裸实体即可）。
fn spawn_source(app: &mut App) -> ModifierSource {
    ModifierSource::new(app.world_mut().spawn_empty().id())
}

// ---------- 加点 / 洗点 / 成长 ----------

#[test]
fn spending_points_updates_final_in_the_same_frame() {
    let mut app = test_app();
    let entity = spawn_actor(&mut app, 10.0, 5.0);
    app.update();
    assert_eq!(cached(&app, entity, AttributeId::Strength), 10.0);

    app.world_mut()
        .resource_mut::<Messages<AttributeAllocationMessage>>()
        .write(AttributeAllocationMessage {
            entity,
            attribute: AttributeId::Strength,
            amount: 3.0,
        });
    app.update();

    let attributes = app.world().get::<Attributes>(entity).unwrap();
    assert_eq!(attributes.cached().get(AttributeId::Strength), 13.0);
    assert_eq!(attributes.allocated().get(AttributeId::Strength), 3.0);
    assert_eq!(attributes.unspent(), 2.0);
}

#[test]
fn spending_points_fails_when_not_enough() {
    let mut app = test_app();
    let entity = spawn_actor(&mut app, 10.0, 1.0);
    app.update();

    app.world_mut()
        .resource_mut::<Messages<AttributeAllocationMessage>>()
        .write(AttributeAllocationMessage {
            entity,
            attribute: AttributeId::Strength,
            amount: 5.0,
        });
    app.update();

    let attributes = app.world().get::<Attributes>(entity).unwrap();
    assert_eq!(attributes.cached().get(AttributeId::Strength), 10.0);
    assert_eq!(attributes.unspent(), 1.0, "点数不足时不应部分扣费");
}

#[test]
fn growth_survives_respec() {
    let mut app = test_app();
    let entity = spawn_actor(&mut app, 10.0, 5.0);
    app.update();

    app.world_mut()
        .resource_mut::<Messages<AttributeGrowthMessage>>()
        .write(AttributeGrowthMessage {
            entity,
            attribute: AttributeId::Strength,
            amount: 2.0,
        });
    app.update();
    assert_eq!(cached(&app, entity, AttributeId::Strength), 12.0);

    app.world_mut()
        .resource_mut::<Messages<AttributeAllocationMessage>>()
        .write(AttributeAllocationMessage {
            entity,
            attribute: AttributeId::Strength,
            amount: 3.0,
        });
    app.update();
    assert_eq!(cached(&app, entity, AttributeId::Strength), 15.0);

    app.world_mut()
        .resource_mut::<Messages<AttributeRespecMessage>>()
        .write(AttributeRespecMessage { entity });
    app.update();

    let attributes = app.world().get::<Attributes>(entity).unwrap();
    assert_eq!(attributes.cached().get(AttributeId::Strength), 12.0);
    assert_eq!(attributes.allocated().get(AttributeId::Strength), 0.0);
    assert_eq!(attributes.unspent(), 5.0, "洗点应退回全部分配点");
}

#[test]
fn respec_refunds_across_multiple_attributes() {
    let mut app = test_app();
    let entity = spawn_actor(&mut app, 10.0, 10.0);
    app.update();

    for (attribute, amount) in [
        (AttributeId::Strength, 3.0),
        (AttributeId::Dexterity, 4.0),
        (AttributeId::Magic, 3.0),
    ] {
        app.world_mut()
            .resource_mut::<Messages<AttributeAllocationMessage>>()
            .write(AttributeAllocationMessage {
                entity,
                attribute,
                amount,
            });
    }
    app.update();

    app.world_mut()
        .resource_mut::<Messages<AttributeRespecMessage>>()
        .write(AttributeRespecMessage { entity });
    app.update();

    let attributes = app.world().get::<Attributes>(entity).unwrap();
    assert_eq!(attributes.allocated().total(), 0.0);
    assert_eq!(attributes.unspent(), 10.0);
    assert_eq!(attributes.cached().get(AttributeId::Strength), 10.0);
    assert_eq!(attributes.cached().get(AttributeId::Dexterity), 0.0);
}

// ---------- 修饰符（L1 公式）----------

#[test]
fn flat_modifier_recomputes_final() {
    let mut app = test_app();
    let entity = spawn_actor(&mut app, 10.0, 0.0);
    let belt = spawn_source(&mut app);
    app.update();

    app.world_mut()
        .resource_mut::<Messages<AddAttributeModifierMessage>>()
        .write(AddAttributeModifierMessage {
            entity,
            attribute: AttributeId::Strength,
            modifier: Modifier::flat(5.0, belt),
        });
    app.update();

    assert_eq!(cached(&app, entity, AttributeId::Strength), 15.0);
}

#[test]
fn modifiers_only_touch_their_own_attribute() {
    let mut app = test_app();
    let entity = spawn_actor(&mut app, 10.0, 0.0);
    let source = spawn_source(&mut app);
    app.update();

    app.world_mut()
        .resource_mut::<Messages<AddAttributeModifierMessage>>()
        .write(AddAttributeModifierMessage {
            entity,
            attribute: AttributeId::Magic,
            modifier: Modifier::flat(5.0, source),
        });
    app.update();

    let modifiers = app.world().get::<AttributeModifiers>(entity).unwrap();
    assert_eq!(modifiers.slot_len(AttributeId::Magic), 1);
    assert_eq!(modifiers.slot_len(AttributeId::Strength), 0);
    assert_eq!(cached(&app, entity, AttributeId::Strength), 10.0);
    assert_eq!(cached(&app, entity, AttributeId::Magic), 5.0);
}

#[test]
fn multiple_sources_stack_by_op_order() {
    let mut app = test_app();
    let entity = spawn_actor(&mut app, 10.0, 0.0);
    let ring = spawn_source(&mut app);
    let potion = spawn_source(&mut app);
    app.update();

    // flat +10 → (10 + 10) = 20；percent_add +50% → 30；percent_mul +50% → 45
    for modifier in [
        Modifier::flat(10.0, ring),
        Modifier::percent_add(0.5, potion),
        Modifier::percent_mul(0.5, ring),
    ] {
        app.world_mut()
            .resource_mut::<Messages<AddAttributeModifierMessage>>()
            .write(AddAttributeModifierMessage {
                entity,
                attribute: AttributeId::Strength,
                modifier,
            });
    }
    app.update();

    assert_eq!(cached(&app, entity, AttributeId::Strength), 45.0);
}

#[test]
fn removing_a_source_drops_all_its_modifiers_across_attributes() {
    let mut app = test_app();
    let entity = spawn_actor(&mut app, 10.0, 0.0);
    let source = spawn_source(&mut app);
    app.update();

    for attribute in [AttributeId::Strength, AttributeId::Magic] {
        app.world_mut()
            .resource_mut::<Messages<AddAttributeModifierMessage>>()
            .write(AddAttributeModifierMessage {
                entity,
                attribute,
                modifier: Modifier::flat(4.0, source),
            });
    }
    app.update();
    assert_eq!(cached(&app, entity, AttributeId::Strength), 14.0);
    assert_eq!(cached(&app, entity, AttributeId::Magic), 4.0);

    app.world_mut()
        .resource_mut::<Messages<RemoveAttributeModifiersMessage>>()
        .write(RemoveAttributeModifiersMessage { entity, source });
    app.update();

    let modifiers = app.world().get::<AttributeModifiers>(entity).unwrap();
    assert!(modifiers.is_empty(), "按来源移除应清掉该来源的所有属性槽位");
    assert_eq!(cached(&app, entity, AttributeId::Strength), 10.0);
    assert_eq!(cached(&app, entity, AttributeId::Magic), 0.0);
}

#[test]
fn respec_does_not_touch_modifiers() {
    let mut app = test_app();
    let entity = spawn_actor(&mut app, 10.0, 5.0);
    let belt = spawn_source(&mut app);
    app.update();

    app.world_mut()
        .resource_mut::<Messages<AddAttributeModifierMessage>>()
        .write(AddAttributeModifierMessage {
            entity,
            attribute: AttributeId::Strength,
            modifier: Modifier::flat(5.0, belt),
        });
    app.world_mut()
        .resource_mut::<Messages<AttributeAllocationMessage>>()
        .write(AttributeAllocationMessage {
            entity,
            attribute: AttributeId::Strength,
            amount: 3.0,
        });
    app.update();
    assert_eq!(cached(&app, entity, AttributeId::Strength), 18.0);

    app.world_mut()
        .resource_mut::<Messages<AttributeRespecMessage>>()
        .write(AttributeRespecMessage { entity });
    app.update();

    // 退掉分配的 3 点，修饰符 +5 保留
    assert_eq!(cached(&app, entity, AttributeId::Strength), 15.0);
}

// ---------- 缓存 ----------

#[test]
fn cache_is_not_recomputed_without_messages() {
    let mut app = test_app();
    let entity = spawn_actor(&mut app, 10.0, 0.0);
    app.update();
    let revision_before = revision(&app, entity);

    for _ in 0..3 {
        app.update();
    }

    assert_eq!(
        revision(&app, entity),
        revision_before,
        "没有脏消息时不应重算（也就不会写缓存）"
    );
    assert_eq!(cached(&app, entity, AttributeId::Strength), 10.0);
}

#[test]
fn modifier_messages_for_unknown_entities_are_ignored() {
    let mut app = test_app();
    let ghost = app.world_mut().spawn_empty().id();
    app.update();

    app.world_mut()
        .resource_mut::<Messages<AddAttributeModifierMessage>>()
        .write(AddAttributeModifierMessage {
            entity: ghost,
            attribute: AttributeId::Strength,
            modifier: Modifier::flat(5.0, ModifierSource::new(ghost)),
        });
    app.update();

    assert!(app.world().get::<AttributeModifiers>(ghost).is_none());
}
