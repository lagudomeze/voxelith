//! 属性：标识、值容器与**自成一体**的组件。
//!
//! [`Stat`] 把四样东西收在一个组件里：
//!
//! | 字段 | 含义 | 谁改 |
//! |---|---|---|
//! | `base` | **初始值**（内容层生成时设定，运行期不变） | `Stat::from_base` |
//! | `allocated` | 已分配的点数（升级成长发的点数也在里面） | 加点 / 洗点消息 |
//! | `modifiers` | 每个属性一组修饰符槽位（装备 / 被动 / 状态派生） | 修饰符消息 |
//! | `cached` | **最终值视图**：`(base + allocated) 经修饰符聚合 → 取整` | 上面三类变更后由 [`Stat::refresh`] 刷新 |
//!
//! 因为自成一体，它**只接受"有修改"的消息**，改完自己刷 `cached`：
//! 不需要 L1 回写最终值，也没有"基础值变了 / 脏了"这类来回消息。
//!
//! 读法：`Deref` 暴露 `cached`（`stat.strength` / `stat.get(StatId::Strength)`），
//! 要看账本用 `base()` / `allocated()` / `unspent_points()` / `modifiers()`。

use core::ops::Deref;

use crate::utils::{Key, SlotVec};
use bevy_ecs::prelude::*;
use exn::{ErrorExt, Result};

/// 属性标识：需要参数化访问时用它（UI、配置、日志、修饰符目标）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StatId {
    /// 力量。
    Strength,
    /// 敏捷。
    Dexterity,
    /// 体质。
    Constitution,
    /// 魔力。
    Magic,
    /// 意志。
    Willpower,
    /// 机敏。
    Cunning,
}

impl StatId {
    /// 稳定标识串（存档 / 配置 / 日志用）；改名等于改对外契约。
    pub const fn name(self) -> &'static str {
        match self {
            StatId::Strength => "strength",
            StatId::Dexterity => "dexterity",
            StatId::Constitution => "constitution",
            StatId::Magic => "magic",
            StatId::Willpower => "willpower",
            StatId::Cunning => "cunning",
        }
    }
}

/// 一组属性值。
///
/// 字段公开只为**方便读**（`block.strength`）；`set` / `add` / `append` / `reset` 都是
/// `pub(crate)`，改值集中在 [`Stat`]，也没有 `IndexMut`。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct StatBlock {
    /// 力量。
    pub strength: f32,
    /// 敏捷。
    pub dexterity: f32,
    /// 体质。
    pub constitution: f32,
    /// 魔力。
    pub magic: f32,
    /// 意志。
    pub willpower: f32,
    /// 机敏。
    pub cunning: f32,
}

impl StatBlock {
    /// 所有属性取同一个默认值（初始属性用）。
    pub const fn new(default_value: f32) -> Self {
        Self {
            strength: default_value,
            dexterity: default_value,
            constitution: default_value,
            magic: default_value,
            willpower: default_value,
            cunning: default_value,
        }
    }

    /// 按标识取值（参数化访问的入口）。
    pub fn get(&self, id: StatId) -> f32 {
        match id {
            StatId::Strength => self.strength,
            StatId::Dexterity => self.dexterity,
            StatId::Constitution => self.constitution,
            StatId::Magic => self.magic,
            StatId::Willpower => self.willpower,
            StatId::Cunning => self.cunning,
        }
    }

    /// 按标识写值（crate 内部）。
    pub(crate) fn set(&mut self, id: StatId, value: f32) {
        match id {
            StatId::Strength => self.strength = value,
            StatId::Dexterity => self.dexterity = value,
            StatId::Constitution => self.constitution = value,
            StatId::Magic => self.magic = value,
            StatId::Willpower => self.willpower = value,
            StatId::Cunning => self.cunning = value,
        }
    }

    /// 按标识加减（crate 内部）。
    pub(crate) fn add(&mut self, id: StatId, value: f32) {
        self.set(id, self.get(id) + value);
    }

    /// 逐项相加（crate 内部：把 `base + allocated` 合成一份）。
    pub(crate) fn append(&mut self, other: &Self) {
        self.strength += other.strength;
        self.dexterity += other.dexterity;
        self.constitution += other.constitution;
        self.magic += other.magic;
        self.willpower += other.willpower;
        self.cunning += other.cunning;
    }

    /// 全部属性之和（洗点退款等汇总）。
    pub fn total(&self) -> f32 {
        self.strength
            + self.dexterity
            + self.constitution
            + self.magic
            + self.willpower
            + self.cunning
    }

    /// 全部清零，返回清零前的总和（crate 内部：洗点退款）。
    pub(crate) fn reset(&mut self) -> f32 {
        let total = self.total();
        *self = Self::default();
        total
    }
}

/// 修饰符的运算种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModifierOp {
    /// 加法：最先结算。
    Flat,
    /// 百分比加法：合并后统一乘一次（不是复利）。
    PercentAdd,
    /// 百分比乘法：各自独立相乘，最后结算。
    PercentMul,
}

/// 一条修饰符。**不含"修饰谁"**：目标由它所在的槽位决定。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Modifier {
    /// 运算种类。
    pub op: ModifierOp,
    /// 数值（`percent_*` 用小数：`0.1` = +10%）。
    pub value: f32,
}

impl Modifier {
    /// 加法修饰符。
    pub fn flat(value: f32) -> Self {
        Self {
            op: ModifierOp::Flat,
            value,
        }
    }

    /// 百分比加法修饰符。
    pub fn percent_add(value: f32) -> Self {
        Self {
            op: ModifierOp::PercentAdd,
            value,
        }
    }

    /// 百分比乘法修饰符。
    pub fn percent_mul(value: f32) -> Self {
        Self {
            op: ModifierOp::PercentMul,
            value,
        }
    }
}

pub type ModifierSet = SlotVec<Modifier>;

/// 属性修饰符槽位：每个属性一组。
///
/// 放在 [`Stat`] 内部（组件自成一体），所以修饰符本身不需要携带"修饰谁"。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StatModifiers {
    /// 力量槽位。
    pub strength: ModifierSet,
    /// 敏捷槽位。
    pub dexterity: ModifierSet,
    /// 体质槽位。
    pub constitution: ModifierSet,
    /// 魔力槽位。
    pub magic: ModifierSet,
    /// 意志槽位。
    pub willpower: ModifierSet,
    /// 机敏槽位。
    pub cunning: ModifierSet,
}

#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct ModifierCaps {
    /// `Σpercent_add` 的下限。
    pub percent_add_min: f32,
    /// `Σpercent_add` 的上限。
    pub percent_add_max: f32,
    /// `Σpercent_mul` 的下限。
    pub percent_mul_min: f32,
    /// `Σpercent_mul` 的上限。
    pub percent_mul_max: f32,
}

impl Default for ModifierCaps {
    fn default() -> Self {
        Self {
            percent_add_min: 0.01,
            percent_add_max: 0.98,
            percent_mul_min: 0.01,
            percent_mul_max: 0.98,
        }
    }
}

impl ModifierCaps {
    pub fn evaluate(&self, base: f32, modifiers: &ModifierSet) -> f32 {
        let mut flat = 0.0;
        let mut percent_add = 0.0;
        let mut percent_mul = 1.0;

        for modifier in modifiers.iter() {
            match modifier.op {
                ModifierOp::Flat => flat += modifier.value,
                ModifierOp::PercentAdd => percent_add += modifier.value,
                ModifierOp::PercentMul => percent_mul *= 1.0 + modifier.value,
            }
        }

        let percent_add = percent_add.clamp(self.percent_add_min, self.percent_add_max);
        let percent_mul = percent_mul.clamp(self.percent_mul_min, self.percent_mul_max);

        (base + flat) * (1.0 + percent_add) * percent_mul
    }
}

impl StatModifiers {
    /// 按标识取槽位（参数化访问）。
    pub fn slot(&self, id: StatId) -> &ModifierSet {
        match id {
            StatId::Strength => &self.strength,
            StatId::Dexterity => &self.dexterity,
            StatId::Constitution => &self.constitution,
            StatId::Magic => &self.magic,
            StatId::Willpower => &self.willpower,
            StatId::Cunning => &self.cunning,
        }
    }

    /// 按标识取可变槽位（crate 内部）。
    pub(crate) fn slot_mut(&mut self, id: StatId) -> &mut ModifierSet {
        match id {
            StatId::Strength => &mut self.strength,
            StatId::Dexterity => &mut self.dexterity,
            StatId::Constitution => &mut self.constitution,
            StatId::Magic => &mut self.magic,
            StatId::Willpower => &mut self.willpower,
            StatId::Cunning => &mut self.cunning,
        }
    }

    /// 所有槽位的可变引用（清来源 / 计时用）。
    pub(crate) fn slots_mut(&mut self) -> [&mut ModifierSet; 6] {
        [
            &mut self.strength,
            &mut self.dexterity,
            &mut self.constitution,
            &mut self.magic,
            &mut self.willpower,
            &mut self.cunning,
        ]
    }

    /// 所有槽位（统计用）。
    fn slots(&self) -> [&ModifierSet; 6] {
        [
            &self.strength,
            &self.dexterity,
            &self.constitution,
            &self.magic,
            &self.willpower,
            &self.cunning,
        ]
    }

    /// 某个属性的修饰符条数。
    pub fn slot_len(&self, id: StatId) -> usize {
        self.slot(id).len()
    }

    /// 所有槽位上的修饰符总数。
    pub fn len(&self) -> usize {
        self.slots().iter().map(|slot| slot.len()).sum()
    }

    /// 是否没有任何修饰符。
    pub fn is_empty(&self) -> bool {
        self.slots().iter().all(|slot| slot.is_empty())
    }
}

/// 属性操作的结构化错误：UI 直接照它决定提示文案。
#[derive(Debug, Clone, Copy, PartialEq, derive_more::Display, derive_more::Error)]
pub enum StatError {
    /// 未分配点数不足。
    #[display("not enough points: required {required}, have {available}")]
    NotEnoughPoints {
        /// 本次需要的点数。
        required: f32,
        /// 当前可用点数。
        available: f32,
    },
}

/// 默认初始属性（内容层可用 `Stat::from_base` 覆盖）。
pub const DEFAULT_STAT: f32 = 10.0;

/// 角色属性（组件，自成一体）。
#[derive(Component, Debug, Clone)]
pub struct Stat {
    base: StatBlock,
    allocated: StatBlock,
    modifiers: StatModifiers,
    cached: StatBlock,
    unspent_points: f32,
}

impl Stat {
    /// 用同一个初始值构造（`Stat::new(10)` = 六项全 10）。
    pub fn new(default_value: f32) -> Self {
        Self::from_base(StatBlock::new(default_value))
    }

    /// 用一份初始值构造；`cached` 先等于 `base`（此时 `allocated` 与修饰符都为空）。
    pub fn from_base(base: StatBlock) -> Self {
        Self {
            base,
            allocated: StatBlock::default(),
            modifiers: StatModifiers::default(),
            cached: base,
            unspent_points: 0.0,
        }
    }

    /// 初始值（运行期不变）。
    pub fn base(&self) -> &StatBlock {
        &self.base
    }

    /// 已分配的点数（升级成长发的点数也算在里面）。
    pub fn allocated(&self) -> &StatBlock {
        &self.allocated
    }

    /// 修饰符槽位（只读）。
    pub fn modifiers(&self) -> &StatModifiers {
        &self.modifiers
    }

    /// 未分配点数。
    pub fn unspent_points(&self) -> f32 {
        self.unspent_points
    }

    /// 发放可分配点数（升级 / 任务奖励）。
    pub(crate) fn add_points(&mut self, amount: f32) {
        self.unspent_points += amount;
    }

    /// 加点：从 `unspent_points` 扣并记进 `allocated`；不足则**整条失败**（不部分生效）。
    pub(crate) fn allocate(&mut self, id: StatId, amount: f32) -> Result<(), StatError> {
        if self.unspent_points < amount {
            return Err(StatError::NotEnoughPoints {
                required: amount,
                available: self.unspent_points,
            }
            .raise());
        }
        self.allocated.add(id, amount);
        self.unspent_points -= amount;
        Ok(())
    }

    /// 洗点：退掉 `allocated`（含成长的分配），返回退回的点数。
    pub(crate) fn respec(&mut self) -> f32 {
        let refunded = self.allocated.reset();
        self.unspent_points += refunded;
        refunded
    }

    /// 加一条修饰符。
    pub(crate) fn add_modifier(&mut self, id: StatId, modifier: Modifier) -> Key {
        self.modifiers.slot_mut(id).insert(modifier)
    }

    /// 按来源移除修饰符；返回是否真的移除了。
    pub(crate) fn remove_modifier(&mut self, id: StatId, key: Key) -> Option<Modifier> {
        self.modifiers.slot_mut(id).remove(key)
    }

    /// **刷新最终值视图**：`(base + allocated) 经修饰符聚合 → 取整`。
    ///
    /// 由各条变更消息的处理系统在改完账本 / 修饰符后调用，所以 `cached` 永远是新的，
    /// 读方（伤害管线、UI）只需 `Deref`。
    pub(crate) fn refresh(&mut self, caps: &ModifierCaps) {
        self.cached.reset();
        self.cached.append(&self.base);
        self.cached.append(&self.allocated);

        self.cached.strength = caps.evaluate(self.base.strength, &self.modifiers.strength);
        self.cached.dexterity = caps.evaluate(self.base.dexterity, &self.modifiers.dexterity);
        self.cached.constitution =
            caps.evaluate(self.base.constitution, &self.modifiers.constitution);
        self.cached.magic = caps.evaluate(self.base.magic, &self.modifiers.magic);
        self.cached.willpower = caps.evaluate(self.base.willpower, &self.modifiers.willpower);
        self.cached.cunning = caps.evaluate(self.base.cunning, &self.modifiers.cunning);
    }
}

impl Default for Stat {
    fn default() -> Self {
        Self::new(DEFAULT_STAT)
    }
}

impl Deref for Stat {
    type Target = StatBlock;

    /// 暴露**只读的最终值视图**（`stat.strength`）。
    fn deref(&self) -> &Self::Target {
        &self.cached
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps() -> ModifierCaps {
        ModifierCaps::default()
    }

    #[test]
    fn fresh_stat_reads_its_base_through_deref() {
        let stat = Stat::new(DEFAULT_STAT);
        assert_eq!(stat.get(StatId::Constitution), DEFAULT_STAT);
        assert_eq!(*stat, *stat.base());
    }

    #[test]
    fn allocate_moves_points_from_unspent_into_allocated() {
        let mut stat = Stat::new(0.0);
        assert!(stat.allocate(StatId::Strength, 10.0).is_err());

        stat.add_points(20.0);
        assert!(stat.allocate(StatId::Strength, 10.0).is_ok());
        assert_eq!(stat.unspent_points(), 10.0);
        assert_eq!(stat.allocated().get(StatId::Strength), 10.0);

        assert!(stat.allocate(StatId::Cunning, 10.0).is_ok());
        assert_eq!(stat.unspent_points(), 0.0);
    }

    #[test]
    fn respec_refunds_allocated_and_keeps_base() {
        let mut stat = Stat::from_base(StatBlock::new(10.0));
        stat.add_points(5.0);
        stat.allocate(StatId::Strength, 3.0).unwrap();
        assert_eq!(stat.respec(), 3.0);
        assert_eq!(stat.unspent_points(), 5.0);
        assert_eq!(stat.base().get(StatId::Strength), 10.0, "初始值不动");
        assert_eq!(stat.allocated().total(), 0.0);
    }

    #[test]
    fn refresh_applies_allocated_then_modifiers() {
        let mut stat = Stat::new(10.0);
        stat.add_points(5.0);
        assert!(stat.allocate(StatId::Strength, 5.0).is_ok());
        let _key = stat.add_modifier(StatId::Strength, Modifier::flat(10.0));
        stat.refresh(&caps());

        // (10 + 5 + 10) = 25
        assert_eq!(stat.strength, 25.0);
        assert_eq!(stat.cunning, 10.0, "未加点的属性只有初始值");
    }

    #[test]
    fn refresh_rounds_once_at_the_end() {
        let mut stat = Stat::new(10.0);
        stat.add_modifier(StatId::Strength, Modifier::percent_add(1.00));
        stat.refresh(&caps());
        assert_eq!(stat.strength, 10.0, "10 * 1.05 = 10.5 → 向下取整 10");

        stat.refresh(&caps());
        assert_eq!(stat.strength, 11.0, "同一份数据换口径 → 11");
    }

    #[test]
    fn removing_a_source_drops_it_from_every_slot() {
        let mut stat = Stat::new(10.0);
        let k1 = stat.add_modifier(StatId::Strength, Modifier::flat(4.0));
        let k2 = stat.add_modifier(StatId::Magic, Modifier::flat(4.0));
        assert_eq!(stat.modifiers().len(), 2);

        assert!(stat.remove_modifier(StatId::Strength, k1).is_some());
        assert_eq!(stat.modifiers().len(), 1);
        assert!(stat.remove_modifier(StatId::Strength, k1).is_none());
        assert!(stat.remove_modifier(StatId::Strength, k2).is_some());
        assert!(stat.modifiers().is_empty());
        assert!(stat.remove_modifier(StatId::Strength, k2).is_none());
    }
}
