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
use core::time::Duration;

use bevy_ecs::prelude::*;
use exn::{ErrorExt, Result};

use crate::atoms::modifiers::{
    Modifier, ModifierCaps, ModifierSet, ModifierSource, Rounding, evaluate,
};

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StatBlock {
    /// 力量。
    pub strength: u32,
    /// 敏捷。
    pub dexterity: u32,
    /// 体质。
    pub constitution: u32,
    /// 魔力。
    pub magic: u32,
    /// 意志。
    pub willpower: u32,
    /// 机敏。
    pub cunning: u32,
}

impl StatBlock {
    /// 所有属性取同一个默认值（初始属性用）。
    pub const fn new(default_value: u32) -> Self {
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
    pub fn get(&self, id: StatId) -> u32 {
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
    pub(crate) fn set(&mut self, id: StatId, value: u32) {
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
    pub(crate) fn add(&mut self, id: StatId, value: u32) {
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
    pub fn total(&self) -> u32 {
        self.strength
            + self.dexterity
            + self.constitution
            + self.magic
            + self.willpower
            + self.cunning
    }

    /// 全部清零，返回清零前的总和（crate 内部：洗点退款）。
    pub(crate) fn reset(&mut self) -> u32 {
        let total = self.total();
        *self = Self::default();
        total
    }
}

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, derive_more::Display, derive_more::Error)]
pub enum StatError {
    /// 未分配点数不足。
    #[display("not enough points: required {required}, have {available}")]
    NotEnoughPoints {
        /// 本次需要的点数。
        required: u32,
        /// 当前可用点数。
        available: u32,
    },
}

/// 默认初始属性（内容层可用 `Stat::from_base` 覆盖）。
pub const DEFAULT_STAT: u32 = 10;

/// 角色属性（组件，自成一体）。
#[derive(Component, Debug, Clone)]
pub struct Stat {
    base: StatBlock,
    allocated: StatBlock,
    modifiers: StatModifiers,
    cached: StatBlock,
    unspent_points: u32,
}

impl Stat {
    /// 用同一个初始值构造（`Stat::new(10)` = 六项全 10）。
    pub fn new(default_value: u32) -> Self {
        Self::from_base(StatBlock::new(default_value))
    }

    /// 用一份初始值构造；`cached` 先等于 `base`（此时 `allocated` 与修饰符都为空）。
    pub fn from_base(base: StatBlock) -> Self {
        Self {
            base,
            allocated: StatBlock::default(),
            modifiers: StatModifiers::default(),
            cached: base,
            unspent_points: 0,
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
    pub fn unspent_points(&self) -> u32 {
        self.unspent_points
    }

    /// 发放可分配点数（升级 / 任务奖励）。
    pub(crate) fn add_points(&mut self, amount: u32) {
        self.unspent_points += amount;
    }

    /// 加点：从 `unspent_points` 扣并记进 `allocated`；不足则**整条失败**（不部分生效）。
    pub(crate) fn allocate(&mut self, id: StatId, amount: u32) -> Result<(), StatError> {
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
    pub(crate) fn respec(&mut self) -> u32 {
        let refunded = self.allocated.reset();
        self.unspent_points += refunded;
        refunded
    }

    /// 加一条修饰符。
    pub(crate) fn add_modifier(&mut self, id: StatId, modifier: Modifier) {
        self.modifiers.slot_mut(id).add(modifier);
    }

    /// 按来源移除修饰符；返回是否真的移除了。
    pub(crate) fn remove_modifiers_by_source(&mut self, source: ModifierSource) -> bool {
        let mut removed = false;
        for slot in self.modifiers.slots_mut() {
            removed |= slot.remove_by_source(source);
        }
        removed
    }

    /// 推进临时修饰符寿命；返回是否有修饰符到期。
    pub(crate) fn tick_modifiers(&mut self, delta: Duration) -> bool {
        let mut expired = false;
        for slot in self.modifiers.slots_mut() {
            expired |= slot.tick(delta);
        }
        expired
    }

    /// **刷新最终值视图**：`(base + allocated) 经修饰符聚合 → 取整`。
    ///
    /// 由各条变更消息的处理系统在改完账本 / 修饰符后调用，所以 `cached` 永远是新的，
    /// 读方（伤害管线、UI）只需 `Deref`。
    pub(crate) fn refresh(&mut self, rounding: Rounding, caps: &ModifierCaps) {
        let mut raw = self.base;
        raw.append(&self.allocated);
        self.cached = StatBlock {
            strength: rounded(&raw, &self.modifiers, StatId::Strength, rounding, caps),
            dexterity: rounded(&raw, &self.modifiers, StatId::Dexterity, rounding, caps),
            constitution: rounded(&raw, &self.modifiers, StatId::Constitution, rounding, caps),
            magic: rounded(&raw, &self.modifiers, StatId::Magic, rounding, caps),
            willpower: rounded(&raw, &self.modifiers, StatId::Willpower, rounding, caps),
            cunning: rounded(&raw, &self.modifiers, StatId::Cunning, rounding, caps),
        };
    }
}

/// 单个属性的公式：聚合修饰符后按配置取整。
fn rounded(
    raw: &StatBlock,
    modifiers: &StatModifiers,
    id: StatId,
    rounding: Rounding,
    caps: &ModifierCaps,
) -> u32 {
    rounding.apply(evaluate(raw.get(id) as f32, modifiers.slot(id), caps))
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

    /// 真实生成来源实体，避免依赖 `Entity` 的内部构造 API。
    fn source(world: &mut World) -> ModifierSource {
        ModifierSource::new(world.spawn_empty().id())
    }

    #[test]
    fn fresh_stat_reads_its_base_through_deref() {
        let stat = Stat::new(DEFAULT_STAT);
        assert_eq!(stat.get(StatId::Constitution), DEFAULT_STAT);
        assert_eq!(*stat, *stat.base());
    }

    #[test]
    fn allocate_moves_points_from_unspent_into_allocated() {
        let mut stat = Stat::new(0);
        assert!(stat.allocate(StatId::Strength, 10).is_err());

        stat.add_points(20);
        assert!(stat.allocate(StatId::Strength, 10).is_ok());
        assert_eq!(stat.unspent_points(), 10);
        assert_eq!(stat.allocated().get(StatId::Strength), 10);

        assert!(stat.allocate(StatId::Cunning, 10).is_ok());
        assert_eq!(stat.unspent_points(), 0);
    }

    #[test]
    fn respec_refunds_allocated_and_keeps_base() {
        let mut stat = Stat::from_base(StatBlock::new(10));
        stat.add_points(5);
        stat.allocate(StatId::Strength, 3).unwrap();
        assert_eq!(stat.respec(), 3);
        assert_eq!(stat.unspent_points(), 5);
        assert_eq!(stat.base().get(StatId::Strength), 10, "初始值不动");
        assert_eq!(stat.allocated().total(), 0);
    }

    #[test]
    fn refresh_applies_allocated_then_modifiers() {
        let mut world = World::new();
        let bonus = source(&mut world);

        let mut stat = Stat::new(10);
        stat.add_points(5);
        stat.allocate(StatId::Strength, 5).unwrap();
        stat.add_modifier(StatId::Strength, Modifier::flat(10.0, bonus));
        stat.refresh(Rounding::Floor, &caps());

        // (10 + 5 + 10) = 25
        assert_eq!(stat.strength, 25);
        assert_eq!(stat.cunning, 10, "未加点的属性只有初始值");
    }

    #[test]
    fn refresh_rounds_once_at_the_end() {
        let mut world = World::new();
        let buff = source(&mut world);

        let mut stat = Stat::new(10);
        stat.add_modifier(StatId::Strength, Modifier::percent_add(0.05, buff));
        stat.refresh(Rounding::Floor, &caps());
        assert_eq!(stat.strength, 10, "10 * 1.05 = 10.5 → 向下取整 10");

        stat.refresh(Rounding::Nearest, &caps());
        assert_eq!(stat.strength, 11, "同一份数据换口径 → 11");
    }

    #[test]
    fn removing_a_source_drops_it_from_every_slot() {
        let mut world = World::new();
        let source = source(&mut world);

        let mut stat = Stat::new(10);
        stat.add_modifier(StatId::Strength, Modifier::flat(4.0, source));
        stat.add_modifier(StatId::Magic, Modifier::flat(4.0, source));
        assert_eq!(stat.modifiers().len(), 2);

        assert!(stat.remove_modifiers_by_source(source));
        assert!(stat.modifiers().is_empty());
        assert!(
            !stat.remove_modifiers_by_source(source),
            "重复移除应为 false"
        );
    }
}
