//! 属性标识与值容器：**唯一定义点**。
//!
//! 新增一个属性 = 在 [`AttributeId`] 的列表里加一行；`COUNT` / `ALL` / `index` / `name`
//! 自动跟随，所有按 `AttributeId::COUNT` 定宽的数组（属性值、每个属性的修饰符槽位）自动扩列。

use core::ops::{Index, IndexMut};

use crate::voxelith_defs;

voxelith_defs! {
    /// 一级属性。
    ///
    /// 字面量是**稳定标识串**（存档 / 配置 / 日志用），改名等于改对外契约。
    pub enum AttributeId {
        Strength = "strength",
        Dexterity = "dexterity",
        Constitution = "constitution",
        Magic = "magic",
        Willpower = "willpower",
        Cunning = "cunning",
    }
}

/// 按 [`AttributeId`] 索引的一组属性值。
///
/// 用定宽数组而不是结构体字段：新增属性不必再补 `get` / `set` / `iter` 这些样板代码。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AttributeValues([f32; AttributeId::COUNT]);

impl AttributeValues {
    /// 读一个属性。
    pub fn get(&self, id: AttributeId) -> f32 {
        self.0[id.index()]
    }

    /// 写一个属性。
    pub fn set(&mut self, id: AttributeId, value: f32) {
        self.0[id.index()] = value;
    }

    /// 给一个属性加减。
    pub fn add(&mut self, id: AttributeId, delta: f32) {
        self.set(id, self.get(id) + delta);
    }

    /// 按定义顺序遍历。
    pub fn iter(&self) -> impl Iterator<Item = (AttributeId, f32)> + '_ {
        AttributeId::ALL.iter().map(|&id| (id, self.get(id)))
    }

    /// 全部属性之和（洗点退款等汇总用）。
    pub fn total(&self) -> f32 {
        AttributeId::ALL.iter().map(|&id| self.get(id)).sum()
    }
}

impl Default for AttributeValues {
    fn default() -> Self {
        Self([0.0; AttributeId::COUNT])
    }
}

impl Index<AttributeId> for AttributeValues {
    type Output = f32;

    fn index(&self, id: AttributeId) -> &Self::Output {
        &self.0[id.index()]
    }
}

impl IndexMut<AttributeId> for AttributeValues {
    fn index_mut(&mut self, id: AttributeId) -> &mut Self::Output {
        &mut self.0[id.index()]
    }
}

#[cfg(test)]
mod tests {
    use super::{AttributeId, AttributeValues};

    #[test]
    fn default_is_all_zero() {
        let values = AttributeValues::default();
        assert!(values.iter().all(|(_, value)| value == 0.0));
    }

    #[test]
    fn set_and_get_round_trip_through_index() {
        let mut values = AttributeValues::default();
        values.set(AttributeId::Willpower, 7.5);
        assert_eq!(values[AttributeId::Willpower], 7.5);
        assert_eq!(values.get(AttributeId::Willpower), 7.5);
    }

    #[test]
    fn total_sums_every_attribute() {
        let mut values = AttributeValues::default();
        values.set(AttributeId::Strength, 3.0);
        values.set(AttributeId::Magic, 4.0);
        assert_eq!(values.total(), 7.0);
    }
}
