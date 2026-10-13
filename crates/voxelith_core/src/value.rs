//! 值：**可修正的量**这一层的基础代码。
//!
//! 一个量 = 一个基础值 + 一串修正。读出规则只有一条：
//!
//! ```text
//! 有效值 = 基础值 + Σ每条修正的贡献(基础值)      // 比例按基础值算
//! ```
//!
//! 这里装三件事：
//!
//! | 东西 | 是什么 |
//! | --- | --- |
//! | [`Modifier<Mark>`] | 一条修正（平加 + 比例），以及它贡献多少（[`Modifier::contribution`]） |
//! | [`Value`] / [`Resourceable`] / [`Max<T>`] | 基础值接口与上限标记 |
//! | [`Total<Mark>`] | **有效值缓存**：上面那条公式算出来的结果放这里，读的人只看它 |
//!
//! ## 公式的求和部分在规则层
//!
//! 这一层只给"一条修正贡献多少"（比例按基础值那半）与"基础值是多少"。**求和与写入缓存**
//! 在 `voxelith_rules` 的重算系统里——因为它要按具体项目的挂法去找修正，而那种知识
//! 不属于这一层（见下）。换个说法：**公式的实质部分在 core，搬运部分在 rules**。
//!
//! ## 修正怎么挂到主人身上，这一层不管
//!
//! “哪些修正算数”是**具体项目**的事：装备、增益、只在某个条件下生效的东西，各自的生命周期
//! 与生效规则都不同，在这里定死一种挂法反而挡路。所以：
//!
//! - 谁重算，谁决定喂什么（`voxelith_rules::recompute`）；
//! - 读的人只读 [`Total<Mark>`]，不用认识任何挂法——`SY-58`（面板上的数就是进公式的数）
//!   从"约定"变成了"结构"。
//!
//! 一份实测过的挂法（自定义 relationship + `linked_spawn` 自动清理 + 在 `bsn!` 里写成列表）
//! 记在 `docs/design/bevy/01-attributes-and-resources.md`。
//!
//! ## 两种形状
//!
//! | 形状 | 哪个值可修正 | 标记 |
//! | --- | --- | --- |
//! | **单值型**（属性、护甲、移动速度……） | 它本身 | 类型自己 |
//! | **资源**（血量、精力……） | **只有上限** | [`Max<T>`] |
//!
//! **当前值不走修正**：它是状态，由事件（伤害、治疗、回复）改。

use bevy_ecs::component::Mutable;
use bevy_ecs::prelude::*;
use core::marker::PhantomData;

// ==================== 修正 ====================

/// 一条修正：平加 + 比例。**这就是修正实体上挂的组件。**
///
/// - **平加** `flat`：绝对值（`+5`；负的就是减）。
/// - **比例** `ratio`：**基础值**的比例（`0.5` = `+50% 基础值`；负的就是按比例减）。
///   注意是按基础值，不是按“已经加过别的修正之后的值”——这样叠加才可交换、可并行。
///
/// 两个值可以同时有，也可以只写一个（另一个留 0）。
/// 同一个量可以同时有多条修正——有加也有减，它们一起归约。
#[derive(Component, Clone, Debug)]
pub struct Modifier<Mark> {
    /// 平加。
    pub flat: f32,
    /// 比例。
    pub ratio: f32,
    /// 修正指向哪个量，靠类型表达。
    _marker: PhantomData<Mark>,
}

// 手写 `Default`：派生版本会给 `Mark` 加 `Default` 约束，而
// `#[require(Modifier<进>)]` 这类地方需要它在**泛型上**就能默认。
impl<Mark> Default for Modifier<Mark> {
    fn default() -> Self {
        Self::new(0.0, 0.0)
    }
}

impl<Mark> Modifier<Mark> {
    /// 平加与比例一起给。
    pub fn new(flat: f32, ratio: f32) -> Self {
        Self {
            flat,
            ratio,
            _marker: PhantomData,
        }
    }

    /// 只给平加。
    pub fn flat(flat: f32) -> Self {
        Self::new(flat, 0.0)
    }

    /// 只给比例。
    pub fn ratio(ratio: f32) -> Self {
        Self::new(0.0, ratio)
    }

    /// 这条修正贡献多少：**平加与比例分开算，再相加**。
    ///
    /// 这是公式的实质部分；求和见模块文档。
    pub fn contribution(&self, base: f32) -> f32 {
        self.flat + base * self.ratio
    }
}

// ==================== 形状 ====================

/// 单值型：只有一个基础值（属性、护甲、移动速度、挖掘力……）。
pub trait Value: Component + Sized {
    /// 基础值。有效值是 [`Total<Self>`]。
    fn base(&self) -> f32;
}

/// 资源：当前值 + 上限。**只有上限可修正。**
pub trait Resourceable: Component<Mutability = Mutable> + Sized {
    /// 当前值（状态，由事件改）。
    fn current(&self) -> f32;

    /// 上限的基础值。有效上限是 [`Total<Max<Self>>`]。
    fn base_max(&self) -> f32;

    /// 当前值的可变引用。
    ///
    /// **只给“改值的唯一通路”用**（[`crate::change::apply_adjustments`]）。别的地方要改当前值，
    /// 请发一条 [`crate::change::Adjust`] 消息——治疗与扣血必须是同一条通路（`SY-17`）。
    fn current_mut(&mut self) -> &mut f32;
}

/// 上限那个量。
///
/// 资源有两个值，所以上限需要一个名字；单值型只有一个值，标记就是它自己。
#[derive(Clone, Copy, Debug, Default)]
pub struct Max<T>(PhantomData<T>);

/// 某个量的**有效值**：基础值 + Σ修正。
///
/// **这是缓存**：由规则层的重算系统写（`voxelith_rules::recompute`），别处**只读**。
/// `#[require(Total<量>)]` 让每个量一出生就带一份，所以 `bsn!` 里不用写它。
///
/// 读的人只看它，于是"面板上的数"与"进公式的数"必然是同一个（`SY-58`）。
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Total<Mark> {
    /// 有效值。
    pub value: f32,
    /// 指向哪个量，靠类型表达。
    _marker: PhantomData<Mark>,
}

impl<Mark> Total<Mark> {
    /// 造一个有效值。
    pub fn new(value: f32) -> Self {
        Self {
            value,
            _marker: PhantomData,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试用的量：一个 newtype（与 `attribute` 里的属性同构）。
    #[derive(Component, Clone, Copy, Debug, Default)]
    struct TestValue(pub f32);

    impl Value for TestValue {
        fn base(&self) -> f32 {
            self.0
        }
    }

    /// 求和。**真实项目里这段在 `voxelith_rules::recompute` 里**（它要按挂法去找修正）；
    /// 这里只是把公式的语义钉在测试里。
    fn sum(base: f32, modifiers: impl Iterator<Item = Modifier<TestValue>>) -> f32 {
        base + modifiers.map(|m| m.contribution(base)).sum::<f32>()
    }

    #[test]
    fn no_modifiers_means_base_value() {
        assert_eq!(sum(10.0, std::iter::empty()), 10.0);
    }

    #[test]
    fn flat_contributes_its_amount() {
        assert_eq!(sum(10.0, [Modifier::flat(5.0)].into_iter()), 15.0);
    }

    #[test]
    fn ratio_contributes_a_share_of_base() {
        assert_eq!(sum(10.0, [Modifier::ratio(0.5)].into_iter()), 15.0);
    }

    /// **一条修正可以同时带平加和比例**，两者分开算再相加。
    #[test]
    fn one_modifier_can_carry_both() {
        // 10 + 5 + 10 × 0.5 = 20
        assert_eq!(sum(10.0, [Modifier::new(5.0, 0.5)].into_iter()), 20.0);
    }

    /// 加与减可以同时挂，一起归约。
    #[test]
    fn plus_and_minus_coexist() {
        let modifiers = [
            Modifier::flat(5.0),
            Modifier::flat(-3.0),
            Modifier::ratio(0.5),
            Modifier::ratio(-0.2),
        ];
        // 10 + 5 - 3 + 10 × 0.5 - 10 × 0.2 = 15
        assert_eq!(sum(10.0, modifiers.into_iter()), 15.0);
    }

    /// 平加与比例都基于基础值，所以登记顺序不影响结果。
    #[test]
    fn modifiers_commute() {
        let a = [Modifier::flat(5.0), Modifier::ratio(0.5)];
        let b = [Modifier::ratio(0.5), Modifier::flat(5.0)]; // 顺序反过来

        assert_eq!(sum(10.0, a.iter().cloned()), 20.0);
        assert_eq!(sum(10.0, a.iter().cloned()), sum(10.0, b.iter().cloned()));
    }

    /// 缓存组件默认 0，且能被写——它只是个格子。
    #[test]
    fn total_is_a_slot() {
        let mut world = World::new();
        let e = world.spawn(Total::<TestValue>::default()).id();

        assert_eq!(world.get::<Total<TestValue>>(e).unwrap().value, 0.0);
        world.get_mut::<Total<TestValue>>(e).unwrap().value = 15.0;
        assert_eq!(world.get::<Total<TestValue>>(e).unwrap().value, 15.0);
    }
}
