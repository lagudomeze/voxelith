//! L1 抵抗：**自成一体**的目标防御数据 + 确定性减伤公式。
//!
//! [`Resistance`] 一个组件装三样：`base`（内容层设定的基础值）、`modifiers`（修饰符槽位）、
//! `cached`（最终值视图，`Deref` 暴露给伤害管线只读）。收到修饰符变更消息后**自己刷新视图**，
//! 所以没有"脏了/算好了"的来回消息。
//!
//! - **两类减伤分家**：百分比减伤 / 护甲走本模块的确定性公式（进管线）；
//!   `evasion`（概率规避）只被 [`crate::behaviors::rolls`] 读，**不进管线**。
//! - **上限**：`percent` 被 [`ResistanceCaps::max_percent`] 夹住，防止减伤堆到免疫。
//! - **不参与状态判定**：状态走 [`crate::behaviors::status`] 的独立豁免框架。
//! - 数据按 [`DamageType`] 列索引（`DamageType` 定义在 L1，所以抵抗也在 L1）。

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_time::Time;

use crate::atoms::{Modifier, ModifierCaps, ModifierSet};
use crate::behaviors::damage::DamageType;
use crate::utils::Key;

/// 单类型抵抗值：先扣 `flat`，再按 `percent` 打折。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ResistEntry {
    /// 固定减伤。
    pub flat: f32,
    /// 百分比减伤（0.25 = 25%）。
    pub percent: f32,
}

/// 抵抗数值（基础值 / 最终值视图共用这个形状）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResistValues {
    /// 每种伤害类型一项（列宽跟随 `DamageType::COUNT`）。
    pub per_type: [ResistEntry; DamageType::COUNT],
    /// 护甲：等价于所有类型的额外 flat 减伤。
    pub armor: f32,
    /// 闪避：概率规避，**不进管线**（判定层用）。
    pub evasion: f32,
}

impl Default for ResistValues {
    fn default() -> Self {
        Self {
            per_type: [ResistEntry::default(); DamageType::COUNT],
            armor: 0.0,
            evasion: 0.0,
        }
    }
}

/// 修饰符作用的位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResistSlot {
    /// 某种伤害类型的固定减伤。
    TypeFlat(DamageType),
    /// 某种伤害类型的百分比减伤。
    TypePercent(DamageType),
    /// 护甲。
    Armor,
    /// 闪避。
    Evasion,
}

/// 抵抗的修饰符槽位（放在 [`Resistance`] 内部）。
///
/// 语义：`percent` 这类"百分比数值"用 `Modifier::flat(0.1)` 表示 +10 个百分点，
/// `percent_add` / `percent_mul` 则按乘法缩放当前数值（与 [`evaluate`] 的语义一致）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ResistanceModifiers {
    /// 每种伤害类型的固定减伤槽位。
    pub flat_per_type: [ModifierSet; DamageType::COUNT],
    /// 每种伤害类型的百分比减伤槽位。
    pub percent_per_type: [ModifierSet; DamageType::COUNT],
    /// 护甲槽位。
    pub armor: ModifierSet,
    /// 闪避槽位。
    pub evasion: ModifierSet,
}

impl ResistanceModifiers {
    /// 固定减伤槽位。
    pub fn flat_slot(&self, damage_type: DamageType) -> &ModifierSet {
        &self.flat_per_type[damage_type.index()]
    }

    /// 百分比减伤槽位。
    pub fn percent_slot(&self, damage_type: DamageType) -> &ModifierSet {
        &self.percent_per_type[damage_type.index()]
    }

    fn slot_mut(&mut self, slot: ResistSlot) -> &mut ModifierSet {
        match slot {
            ResistSlot::TypeFlat(damage_type) => &mut self.flat_per_type[damage_type.index()],
            ResistSlot::TypePercent(damage_type) => &mut self.percent_per_type[damage_type.index()],
            ResistSlot::Armor => &mut self.armor,
            ResistSlot::Evasion => &mut self.evasion,
        }
    }

    /// 所有槽位的可变引用（清来源 / 计时用）。
    fn slots_mut(&mut self) -> impl Iterator<Item = &mut ModifierSet> {
        self.flat_per_type
            .iter_mut()
            .chain(self.percent_per_type.iter_mut())
            .chain([&mut self.armor, &mut self.evasion])
    }
}

/// 抵抗（组件，自成一体）。
#[derive(Component, Debug, Clone)]
pub struct Resistance {
    base: ResistValues,
    modifiers: ResistanceModifiers,
    cached: ResistValues,
}

impl Resistance {
    /// 空抵抗。
    pub fn new() -> Self {
        Self::from_base(ResistValues::default())
    }

    /// 用内容层给的基础值构造（最终值视图先等于基础值）。
    pub fn from_base(base: ResistValues) -> Self {
        Self {
            base,
            modifiers: ResistanceModifiers::default(),
            cached: base,
        }
    }

    /// 基础值（内容层设定）。
    pub fn base(&self) -> &ResistValues {
        &self.base
    }

    /// 修饰符槽位（只读）。
    pub fn modifiers(&self) -> &ResistanceModifiers {
        &self.modifiers
    }

    /// 加一条修饰符。
    pub(crate) fn insert_modifier(&mut self, slot: ResistSlot, modifier: Modifier) -> Key {
        self.modifiers.slot_mut(slot).insert(modifier)
    }

    /// 按来源移除修饰符；返回是否真的移除了。
    pub(crate) fn remove_modifier(&mut self, slot: ResistSlot, key: Key) -> bool {
        self.modifiers.slot_mut(slot).remove(key).is_some()
    }

    /// 推进临时修饰符寿命；返回是否有修饰符到期。
    pub(crate) fn tick_modifiers(&mut self, delta: core::time::Duration) -> bool {
        let mut expired = false;
        for slot in self.modifiers.slots_mut() {
            expired |= slot.tick(delta);
        }
        expired
    }

    /// **刷新最终值视图**：`base + 各槽位修饰符 → 最终值`（确定性、无随机）。
    pub(crate) fn refresh(&mut self, modifier_caps: &ModifierCaps, caps: &ResistanceCaps) {
        let base = self.base;
        let mut values = base;
        for damage_type in DamageType::ALL {
            let index = damage_type.index();
            values.per_type[index] = ResistEntry {
                flat: evaluate(
                    base.per_type[index].flat.max(0.0),
                    self.modifiers.flat_slot(damage_type),
                    modifier_caps,
                ),
                percent: evaluate(
                    base.per_type[index].percent.max(0.0),
                    self.modifiers.percent_slot(damage_type),
                    modifier_caps,
                )
                .min(caps.max_percent),
            };
        }
        values.armor = evaluate(base.armor, &self.modifiers.armor, modifier_caps);
        values.evasion = evaluate(base.evasion, &self.modifiers.evasion, modifier_caps);
        self.cached = values;
    }
}

impl Default for Resistance {
    fn default() -> Self {
        Self::new()
    }
}

impl core::ops::Deref for Resistance {
    type Target = ResistValues;

    /// 暴露**只读的最终值视图**（伤害管线只读它）。
    fn deref(&self) -> &Self::Target {
        &self.cached
    }
}

/// 抵抗上限与保底（**Resource**：内容层 / 文件注入）。
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct ResistanceCaps {
    /// 百分比减伤上限（默认 85%，防止免疫）。
    pub max_percent: f32,
    /// 保底伤害比例：无论怎么堆减伤，至少造成基础伤害的这个比例（默认 10%）。
    pub min_damage_ratio: f32,
}

impl Default for ResistanceCaps {
    fn default() -> Self {
        Self {
            max_percent: 0.85,
            min_damage_ratio: 0.1,
        }
    }
}

/// 添加一条抵抗修饰符。
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub struct AddResistanceModifierMessage {
    /// 目标实体。
    pub entity: Entity,
    /// 作用位置。
    pub slot: ResistSlot,
    /// 修饰符本体。
    pub modifier: Modifier,
}

/// 按来源移除该实体上所有抵抗修饰符（装备卸下 / 被动失效 / 状态到期）。
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoveResistanceModifiersMessage {
    /// 目标实体。
    pub entity: Entity,
    /// 要移除的来源。
    pub source: ModifierSource,
}

/// 添加修饰符：加完自己刷新视图。
pub fn apply_resistance_modifier_add(
    mut messages: MessageReader<AddResistanceModifierMessage>,
    mut actors: Query<&mut Resistance>,
    modifier_caps: Res<ModifierCaps>,
    caps: Res<ResistanceCaps>,
) {
    for message in messages.read() {
        let Ok(mut resistance) = actors.get_mut(message.entity) else {
            continue;
        };
        resistance.add_modifier(message.slot, message.modifier);
        resistance.refresh(&modifier_caps, &caps);
    }
}

/// 按来源移除修饰符（没移除到任何东西就不重算）。
pub fn apply_resistance_modifier_remove(
    mut messages: MessageReader<RemoveResistanceModifiersMessage>,
    mut actors: Query<&mut Resistance>,
    modifier_caps: Res<ModifierCaps>,
    caps: Res<ResistanceCaps>,
) {
    for message in messages.read() {
        let Ok(mut resistance) = actors.get_mut(message.entity) else {
            continue;
        };
        if resistance.remove_modifiers_by_source(message.source) {
            resistance.refresh(&modifier_caps, &caps);
        }
    }
}

/// 推进临时抵抗修饰符的寿命（药水 / 临时护盾到点即失效）。
pub fn tick_resistance_modifier_lifetimes(
    time: Res<Time>,
    mut actors: Query<&mut Resistance>,
    modifier_caps: Res<ModifierCaps>,
    caps: Res<ResistanceCaps>,
) {
    let delta = time.delta();
    if delta.is_zero() {
        return;
    }

    for mut resistance in &mut actors {
        if resistance.tick_modifiers(delta) {
            resistance.refresh(&modifier_caps, &caps);
        }
    }
}

/// 确定性减伤（**无随机**，管线阶段三调用）。
///
/// 顺序：`flat + armor` → 百分比（夹上限）→ 保底比例（至少造成基础伤害的 `min_damage_ratio`）。
pub fn mitigate(amount: f32, entry: ResistEntry, armor: f32, caps: &ResistanceCaps) -> f32 {
    if amount <= 0.0 {
        return 0.0;
    }
    let after_flat = (amount - entry.flat - armor).max(0.0);
    let percent = entry.percent.clamp(0.0, caps.max_percent);
    let after_percent = after_flat * (1.0 - percent);
    after_percent.max(amount * caps.min_damage_ratio)
}

/// 注册抵抗域的消息与系统。
pub struct ResistancePlugin;

impl Plugin for ResistancePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ResistanceCaps>()
            .init_resource::<ModifierCaps>()
            .add_message::<AddResistanceModifierMessage>()
            .add_message::<RemoveResistanceModifiersMessage>()
            .add_systems(
                Update,
                (
                    tick_resistance_modifier_lifetimes,
                    apply_resistance_modifier_add,
                    apply_resistance_modifier_remove,
                )
                    .chain(),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_reduction_applies_before_percent() {
        let caps = ResistanceCaps::default();
        let entry = ResistEntry {
            flat: 10.0,
            percent: 0.5,
        };
        // (100 - 10 - 0) * 0.5 = 45
        assert_eq!(mitigate(100.0, entry, 0.0, &caps), 45.0);
    }

    #[test]
    fn percent_is_capped_and_damage_has_a_floor() {
        let caps = ResistanceCaps {
            max_percent: 0.5,
            min_damage_ratio: 0.2,
        };
        let entry = ResistEntry {
            flat: 0.0,
            percent: 0.99,
        };
        // 99% 被夹到 50%：50；保底是 100*0.2 = 20 → 取 50
        assert_eq!(mitigate(100.0, entry, 0.0, &caps), 50.0);
        // 极端 flat 也不能打到免疫：保底 20
        let entry = ResistEntry {
            flat: 999.0,
            percent: 0.0,
        };
        assert_eq!(mitigate(100.0, entry, 0.0, &caps), 20.0);
    }

    #[test]
    fn armor_is_an_extra_flat_reduction() {
        let caps = ResistanceCaps::default();
        let entry = ResistEntry::default();
        assert_eq!(mitigate(100.0, entry, 25.0, &caps), 75.0);
    }

    fn resistance_app() -> (App, Entity) {
        let mut app = App::new();
        // 临时修饰符计时读 `Time`；这里不推进时间，只为让系统参数存在。
        app.init_resource::<Time>();
        app.add_plugins(ResistancePlugin);
        let entity = app.world_mut().spawn(Resistance::new()).id();
        (app, entity)
    }

    fn spawn_source(app: &mut App) -> ModifierSource {
        ModifierSource::new(app.world_mut().spawn_empty().id())
    }

    /// 修饰符 → 最终值视图 → 减伤：整条链在同一帧内跑通。
    #[test]
    fn modifiers_flow_into_the_resistance_view() {
        let (mut app, entity) = resistance_app();
        let source = spawn_source(&mut app);

        app.world_mut()
            .resource_mut::<Messages<AddResistanceModifierMessage>>()
            .write(AddResistanceModifierMessage {
                entity,
                slot: ResistSlot::TypeFlat(DamageType::Fire),
                modifier: Modifier::flat(12.0, source),
            });
        app.update();

        let resistance = app.world().get::<Resistance>(entity).unwrap();
        let entry = resistance.per_type[DamageType::Fire.index()];
        assert_eq!(entry.flat, 12.0);
        assert_eq!(
            mitigate(100.0, entry, 0.0, &ResistanceCaps::default()),
            88.0
        );
        assert_eq!(
            resistance.per_type[DamageType::Frost.index()].flat,
            0.0,
            "修饰符只影响它自己的槽位"
        );
    }

    /// 百分比减伤修饰符必须被 `ResistanceCaps::max_percent` 夹住（防免疫）。
    #[test]
    fn percent_modifiers_are_capped_by_resistance_caps() {
        let (mut app, entity) = resistance_app();
        let source = spawn_source(&mut app);

        app.world_mut()
            .resource_mut::<Messages<AddResistanceModifierMessage>>()
            .write(AddResistanceModifierMessage {
                entity,
                slot: ResistSlot::TypePercent(DamageType::Physical),
                modifier: Modifier::flat(0.95, source),
            });
        app.update();

        let entry =
            app.world().get::<Resistance>(entity).unwrap().per_type[DamageType::Physical.index()];
        assert_eq!(entry.percent, 0.85, "95% 应被夹到上限 85%");
        // f32 精度：100 * (1 - 0.85) 可能是 14.999998；伤害到管线末段才取整，这里用容差比较
        let mitigated = mitigate(100.0, entry, 0.0, &ResistanceCaps::default());
        assert!(
            (mitigated - 15.0).abs() < 0.001,
            "减伤后应约为 15，实际 {mitigated}"
        );
    }

    /// 按来源移除：装备卸下 / 临时药水到期时，所有槽位一次清干净。
    #[test]
    fn removing_a_source_clears_every_resistance_slot() {
        let (mut app, entity) = resistance_app();
        let source = spawn_source(&mut app);

        for slot in [
            ResistSlot::TypeFlat(DamageType::Fire),
            ResistSlot::Armor,
            ResistSlot::Evasion,
        ] {
            app.world_mut()
                .resource_mut::<Messages<AddResistanceModifierMessage>>()
                .write(AddResistanceModifierMessage {
                    entity,
                    slot,
                    modifier: Modifier::flat(0.2, source),
                });
        }
        app.update();

        let resistance = app.world().get::<Resistance>(entity).unwrap();
        assert_eq!(resistance.per_type[DamageType::Fire.index()].flat, 0.2);
        assert_eq!(resistance.armor, 0.2);
        assert_eq!(resistance.evasion, 0.2);

        app.world_mut()
            .resource_mut::<Messages<RemoveResistanceModifiersMessage>>()
            .write(RemoveResistanceModifiersMessage { entity, source });
        app.update();

        let resistance = app.world().get::<Resistance>(entity).unwrap();
        assert_eq!(resistance.per_type[DamageType::Fire.index()].flat, 0.0);
        assert_eq!(resistance.armor, 0.0);
        assert_eq!(resistance.evasion, 0.0);
    }

    /// 临时修饰符到期后视图自己回落。
    #[test]
    fn temporary_resistance_modifier_expires() {
        let (mut app, entity) = resistance_app();
        let source = spawn_source(&mut app);

        app.world_mut()
            .resource_mut::<Messages<AddResistanceModifierMessage>>()
            .write(AddResistanceModifierMessage {
                entity,
                slot: ResistSlot::Armor,
                modifier: Modifier::flat(20.0, source).lasting(core::time::Duration::from_secs(2)),
            });
        app.update();
        assert_eq!(app.world().get::<Resistance>(entity).unwrap().armor, 20.0);

        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(core::time::Duration::from_secs(2));
        app.update();

        assert_eq!(app.world().get::<Resistance>(entity).unwrap().armor, 0.0);
        assert!(
            app.world()
                .get::<Resistance>(entity)
                .unwrap()
                .modifiers()
                .armor
                .is_empty()
        );
    }
}
