//! L1 修饰符：只有**数据与聚合公式**。
//!
//! 它不认识"被修饰的是什么"——每个域自带修饰符槽位组件与脏消息
//! （例：[`crate::behaviors::attributes::AttributeModifiers`]），
//! 因此不存在中心 `ModifierTarget` 枚举那种"改一处、动全身"的枢纽耦合。
//!
//! 聚合顺序**固定、确定性、无随机**：
//!
//! ```text
//! final = (base + Σflat) * (1 + clamp(Σpercent_add, min, max)) * Π(1 + percent_mul)
//! ```
//!
//! 顺序即语义：`flat` 先结算，`percent_add` 合并后只乘一次（多个 +10% = +20%），
//! `percent_mul` 各自独立相乘（复利）。要调平衡就调 [`ModifierCaps`]，不要改这里。

use bevy_ecs::prelude::*;

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

/// 修饰符来源：施加它的实体（装备 / 被动 / 状态实例）。
///
/// 按来源整体移除是"装备卸下、被动失效、状态到期"的统一清理方式（作用域化的落点）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ModifierSource(pub Entity);

impl ModifierSource {
    /// 用来源实体构造。
    pub fn new(entity: Entity) -> Self {
        Self(entity)
    }

    /// 来源实体。
    pub fn entity(self) -> Entity {
        self.0
    }
}

/// 一条修饰符。**不含"修饰谁"**：目标由它所在的槽位决定。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Modifier {
    /// 运算种类。
    pub op: ModifierOp,
    /// 数值（`percent_*` 用小数：`0.1` = +10%）。
    pub value: f32,
    /// 来源（用于按来源批量移除）。
    pub source: ModifierSource,
}

impl Modifier {
    /// 加法修饰符。
    pub fn flat(value: f32, source: ModifierSource) -> Self {
        Self {
            op: ModifierOp::Flat,
            value,
            source,
        }
    }

    /// 百分比加法修饰符。
    pub fn percent_add(value: f32, source: ModifierSource) -> Self {
        Self {
            op: ModifierOp::PercentAdd,
            value,
            source,
        }
    }

    /// 百分比乘法修饰符。
    pub fn percent_mul(value: f32, source: ModifierSource) -> Self {
        Self {
            op: ModifierOp::PercentMul,
            value,
            source,
        }
    }
}

/// 聚合上限：防止 `percent_add` 堆到免疫（或负到无意义）。
///
/// 这是"抵抗支持上限、防止减伤堆到免疫"的通用手段，各域只需要自己的 [`ModifierCaps`] 资源。
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct ModifierCaps {
    /// `Σpercent_add` 的下限。
    pub percent_add_min: f32,
    /// `Σpercent_add` 的上限。
    pub percent_add_max: f32,
}

impl Default for ModifierCaps {
    fn default() -> Self {
        Self {
            percent_add_min: -0.9,
            percent_add_max: 10.0,
        }
    }
}

/// 一个槽位上的修饰符列表（某个属性的、某种抵抗的……）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModifierSet(Vec<Modifier>);

impl ModifierSet {
    /// 添加一条修饰符。
    pub fn add(&mut self, modifier: Modifier) {
        self.0.push(modifier);
    }

    /// 按来源移除（装备卸下 / 被动失效 / 状态到期）；返回是否真的移除了。
    pub fn remove_by_source(&mut self, source: ModifierSource) -> bool {
        let before = self.0.len();
        self.0.retain(|modifier| modifier.source != source);
        self.0.len() != before
    }

    /// 遍历修饰符（聚合公式用）。
    pub fn iter(&self) -> impl Iterator<Item = &Modifier> {
        self.0.iter()
    }

    /// 修饰符条数。
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// 聚合一个槽位：顺序见[模块文档](self)。
pub fn evaluate(base: f32, modifiers: &ModifierSet, caps: &ModifierCaps) -> f32 {
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

    let percent_add = percent_add.clamp(caps.percent_add_min, caps.percent_add_max);
    (base + flat) * (1.0 + percent_add) * percent_mul
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 真实生成来源实体，避免依赖 `Entity` 的内部构造 API。
    fn source(world: &mut World) -> ModifierSource {
        ModifierSource::new(world.spawn_empty().id())
    }

    fn set(modifiers: &[Modifier]) -> ModifierSet {
        let mut set = ModifierSet::default();
        for modifier in modifiers {
            set.add(*modifier);
        }
        set
    }

    #[test]
    fn empty_set_returns_base() {
        assert_eq!(
            evaluate(10.0, &ModifierSet::default(), &ModifierCaps::default()),
            10.0
        );
    }

    #[test]
    fn flat_adds_before_percentages() {
        let mut world = World::new();
        let modifiers = set(&[
            Modifier::flat(5.0, source(&mut world)),
            Modifier::flat(-2.0, source(&mut world)),
        ]);
        assert_eq!(evaluate(10.0, &modifiers, &ModifierCaps::default()), 13.0);
    }

    #[test]
    fn percent_add_merges_into_one_multiplier() {
        // 两个 +50% 是 +100%（合并成 *2），不是复利 *2.25
        let mut world = World::new();
        let modifiers = set(&[
            Modifier::percent_add(0.5, source(&mut world)),
            Modifier::percent_add(0.5, source(&mut world)),
        ]);
        assert_eq!(evaluate(10.0, &modifiers, &ModifierCaps::default()), 20.0);
    }

    #[test]
    fn percent_mul_compounds_last() {
        // (10 + 10) * (1 + 0.5) * (1 + 0.5) = 45
        let mut world = World::new();
        let modifiers = set(&[
            Modifier::flat(10.0, source(&mut world)),
            Modifier::percent_add(0.5, source(&mut world)),
            Modifier::percent_mul(0.5, source(&mut world)),
        ]);
        assert_eq!(evaluate(10.0, &modifiers, &ModifierCaps::default()), 45.0);
    }

    #[test]
    fn percent_add_is_clamped_by_caps() {
        let caps = ModifierCaps {
            percent_add_min: -0.5,
            percent_add_max: 0.5,
        };
        let mut world = World::new();
        let modifiers = set(&[
            Modifier::percent_add(1.0, source(&mut world)),
            Modifier::percent_add(1.0, source(&mut world)),
        ]);
        // 合计 +200% 被夹到 +50%
        assert_eq!(evaluate(10.0, &modifiers, &caps), 15.0);
    }

    #[test]
    fn remove_by_source_only_drops_that_source() {
        let mut world = World::new();
        let kept = source(&mut world);
        let loaded = source(&mut world);
        let mut modifiers = set(&[Modifier::flat(5.0, loaded), Modifier::flat(3.0, kept)]);

        assert!(modifiers.remove_by_source(loaded));
        assert_eq!(modifiers.len(), 1);
        assert_eq!(evaluate(0.0, &modifiers, &ModifierCaps::default()), 3.0);
        assert!(!modifiers.remove_by_source(loaded), "重复移除应返回 false");
    }
}
