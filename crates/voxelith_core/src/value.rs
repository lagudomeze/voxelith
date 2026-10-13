//! 值：**可修正的量**这一层的基础代码。
//!
//! 一个量 = 一个自己存着的数 + 一串修正。读出规则只有一条：
//!
//! ```text
//! 有效值 = raw + Σ每条修正的贡献(raw)      // 比例按 raw 算
//! ```
//!
//! 这里装三件事：
//!
//! | 东西 | 是什么 |
//! | --- | --- |
//! | [`Modifier<Mark>`] | 一条修正（平加 + 比例），以及它贡献多少（[`Modifier::contribution`]） |
//! | [`Value`] | "这个量自己存着一个数"——属性存基础值，资源当前值组件存当前值；[`Max<Q>`] 存基础上限 |
//! | [`Total<Mark>`] | **有效值缓存**：上面那条公式算出来的结果放这里，读的人只看它 |
//!
//! ## 名字怎么说
//!
//! - [`Value::raw`] 是**取数**：中性词，因为同一个方法要管四种角色（属性的基础值、上限的基础值、
//!   回复速率的基础值、资源当前值的**当前值**）。
//! - 文档与设计里说「**基础值**」时指的是语义（属性/上限/回复速率的那一半）；说「**有效值**」
//!   时指的是 [`Total`] 里的结果。
//! - **当前值不走修正**（它是状态，由事件改）：所以它的组件**不挂 `Total`、也不注册重算**——
//!   这是一条约定，`Value` 这个 trait 本身管不住它。
//!
//! ## 公式的求和部分在规则层
//!
//! 这一层只给"一条修正贡献多少"（比例按 raw 那半）与"raw 是多少"。**求和与写入缓存**
//! 在 `voxelith_rules` 的重算系统里——因为它要按具体项目的挂法去找修正。
//!
//! ## 修正怎么挂到主人身上，这一层不管
//!
//! “哪些修正算数”是**具体项目**的事：装备、增益、只在某个条件下生效的东西，各自的生命周期
//! 与生效规则都不同，在这里定死一种挂法反而挡路。所以：谁重算，谁决定喂什么
//! （`voxelith_rules::recompute`）；读的人只读 [`Total`]，不用认识任何挂法——`SY-58`
//! （面板上的数就是进公式的数）从"约定"变成了"结构"。
//!
//! ## 资源 = 两个组件
//!
//! 资源的当前值是一个 [`Value`]（`Health`），上限是另一个 [`Value`]（`Max<Health>`），
//! 靠 `#[require(Max<Health>)]` 绑在一起。这样上限直接复用 `Value` 的修正能力与缓存，
//! 不需要第二个 trait、也不需要第二条重算路径。

use bevy_ecs::component::Mutable;
use bevy_ecs::prelude::*;
use core::marker::PhantomData;

// ==================== 修正 ====================

/// 一条修正：平加 + 比例。**这就是修正实体上挂的组件。**
///
/// - **平加** `flat`：绝对值（`+5`；负的就是减）。
/// - **比例** `ratio`：**raw** 的比例（`0.5` = `+50%`；负的就是按比例减）。
///   注意是按 raw，不是按“已经加过别的修正之后的值”——这样叠加才可交换、可并行。
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
    pub fn contribution(&self, raw: f32) -> f32 {
        self.flat + raw * self.ratio
    }
}

// ==================== 量 ====================

/// 一个"自己存着一个数"的量：属性（基础值）、资源的当前值、上限（[`Max`]）、回复速率。
///
/// 有效值（raw + Σ修正）在 [`Total<Self>`] 里——但**只有需要它的量才挂 `Total`**；
/// 资源的当前值是状态，不挂。
pub trait Value: Component<Mutability = Mutable> + Sized {
    /// 取这个量自己存着的那个数。
    fn raw(&self) -> f32;

    /// 那个数的可变引用。
    ///
    /// **只给"改值的唯一通路"用**（[`crate::change::Adjust`] + [`crate::change::apply_adjustments`]）。
    /// 别的地方要改它，请发一条消息——治疗、扣血、加点都走同一条路（`SY-17`）。
    fn raw_mut(&mut self) -> &mut f32;
}

/// 上限：资源上限那个量，**本身也是一个 [`Value`]**。
///
/// 资源被拆成两个组件——当前值（如 `Health`）与上限（`Max<Health>`），当前值 `#[require]` 了它。
/// 于是上限天然拥有修正能力与 [`Total`] 缓存，重算也只认 [`Value`] 一种输入。
///
/// 第二个字段是"指向哪个量"的类型标记；`bsn!` 里用别名写 `HealthMax(60.0)` 就行（未指定的字段默认）。
#[derive(Component, Clone, Copy, Debug)]
#[require(Total<Max<Q>>)]
pub struct Max<Q: Value>(pub f32, PhantomData<Q>);

impl<Q: Value> Max<Q> {
    /// 造一个基础上限。Rust 代码里用它；`bsn!` 里用别名写 `HealthMax(60.0)`。
    pub fn new(value: f32) -> Self {
        Self(value, PhantomData)
    }
}

// 手写 `Default`：派生版本会给 `Q` 加约束，而 `#[require(Max<资源>)]` 需要它在泛型上就有默认。
impl<Q: Value> Default for Max<Q> {
    fn default() -> Self {
        Self(0.0, PhantomData)
    }
}

impl<Q: Value> Value for Max<Q> {
    fn raw(&self) -> f32 {
        self.0
    }

    fn raw_mut(&mut self) -> &mut f32 {
        &mut self.0
    }
}

/// 某个量的**有效值**：raw + Σ修正。
///
/// **这是缓存**：由规则层的重算系统写（`voxelith_rules::recompute`），别处**只读**。
/// `#[require(Total<量>)]` 让需要它的量一出生就带一份，所以 `bsn!` 里不用写它。
///
/// 读的人只看它，于是"面板上的数"与"进公式的数"必然是同一个（`SY-58`）。
#[derive(Component, Clone, Copy, Debug)]
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

// 手写 `Default`：派生版本会给 `Mark` 加约束，而 `#[require(Total<量>)]` 需要它在泛型上就有默认。
impl<Mark> Default for Total<Mark> {
    fn default() -> Self {
        Self::new(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试用的量：一个 newtype（与 `attribute` 里的属性同构）。
    #[derive(Component, Clone, Copy, Debug, Default)]
    struct TestValue(pub f32);

    impl Value for TestValue {
        fn raw(&self) -> f32 {
            self.0
        }

        fn raw_mut(&mut self) -> &mut f32 {
            &mut self.0
        }
    }

    /// 求和。**真实项目里这段在 `voxelith_rules::recompute` 里**（它要按挂法去找修正）；
    /// 这里只是把公式的语义钉在测试里。
    fn sum(raw: f32, modifiers: impl Iterator<Item = Modifier<TestValue>>) -> f32 {
        raw + modifiers.map(|m| m.contribution(raw)).sum::<f32>()
    }

    #[test]
    fn no_modifiers_means_raw() {
        assert_eq!(sum(10.0, std::iter::empty()), 10.0);
    }

    #[test]
    fn flat_contributes_its_amount() {
        assert_eq!(sum(10.0, [Modifier::flat(5.0)].into_iter()), 15.0);
    }

    #[test]
    fn ratio_contributes_a_share_of_raw() {
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

    /// 平加与比例都基于 raw，所以登记顺序不影响结果。
    #[test]
    fn modifiers_commute() {
        let a = [Modifier::flat(5.0), Modifier::ratio(0.5)];
        let b = [Modifier::ratio(0.5), Modifier::flat(5.0)]; // 顺序反过来

        assert_eq!(sum(10.0, a.iter().cloned()), 20.0);
        assert_eq!(sum(10.0, a.iter().cloned()), sum(10.0, b.iter().cloned()));
    }

    /// 上限本身是个量：能取数、能改。
    #[test]
    fn max_is_a_value() {
        let mut max = Max::<TestValue>(60.0, PhantomData);

        assert_eq!(max.raw(), 60.0);
        *max.raw_mut() += 20.0;
        assert_eq!(max.raw(), 80.0);
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
