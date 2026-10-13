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
//! | [`Modifier<Mark>`] | 一条修正（平加 + 比例），以及它贡献多少 |
//! | [`Affects`] / [`Modifiers`] | **归属**：这条修正算在谁头上、主人身上有哪些修正 |
//! | [`Value`] / [`Max<Q>`] / [`Total<Mark>`] | 量接口、上限、有效值缓存 |
//!
//! ## 归属内置且唯一，来源自由
//!
//! “这条修正算在谁头上”每个修正都有、且只有一个答案，所以它**内置在库里**：
//!
//! - [`Affects`]：修正实体上写着它算在谁头上（可以指向自己——派生就是"给自己挂一条"）；
//! - [`Modifiers`]：主人身上自动维护的名单，由 Bevy 的关系机制增删，`linked_spawn` 负责清理。
//!
//! **只有这一条归属关系**，所以不会出现"两个插件各算一半、互相覆盖"那种静默错。
//! 项目真正自由的是**来源怎么组织**——装备、增益、光环、地形效果是修正实体上的**标签组件**
//! （额外挂什么随项目），不是第二条归属关系。
//!
//! 为什么不把归属直接放进 [`Modifier`]（`who` 字段）：那样 `Modifier` 自己就是关系组件，
//! 而场景机制给每个 child 会再插一次关系组件 ✗ → `Bundle has duplicate components` ✗
//! （而且它也要求组件派生 `FromTemplate`，会与名单写法二选一）。实测记在
//! `docs/design/bevy/02-derived-rules.md`。
//!
//! ## 名字怎么说
//!
//! - [`Value::raw`] 是**取数**：中性词，因为同一个方法要管四种角色（属性的基础值、上限的基础值、
//!   回复速率的基础值、资源当前值的**当前值**）。
//! - 文档与设计里说「**基础值**」时指的是语义；说「**有效值**」时指的是 [`Total`] 里的结果。
//! - **当前值不走修正**（它是状态，由事件改）：所以它的组件**不挂 `Total`、也不注册重算**——
//!   这是一条约定，`Value` 这个 trait 本身管不住它。
//!
//! ## 公式的求和部分在规则层
//!
//! 这一层只给"一条修正贡献多少"（比例按 raw 那半）与"raw 是多少"。**求和与写入缓存**
//! 在 `voxelith_rules` 的重算系统里。
//!
//! ## 资源 = 两个组件
//!
//! 资源的当前值是一个 [`Value`]（`Health`），上限是另一个 [`Value`]（[`Max<Health>`]），
//! 靠 `#[require(Max<Health>)]` 绑在一起。这样上限直接复用 `Value` 的修正能力与缓存，
//! 不需要第二个 trait、也不需要第二条重算路径。

use bevy_ecs::component::Mutable;
use bevy_ecs::prelude::*;
use core::marker::PhantomData;

// ==================== 归属 ====================

/// 这条修正**算在谁头上**。
///
/// 放在修正实体上；主人身上的名单 [`Modifiers`] 由 Bevy 自动维护。可以指向自己——
/// 派生规则就是"给自己挂一条修正"（`allow_self_referential`）。
///
/// **归属只有这一条**：装备、增益、天赋……是修正实体上的标签，不是第二条归属关系。
#[derive(Component, Clone, Copy, Debug, FromTemplate)]
#[relationship(relationship_target = Modifiers, allow_self_referential)]
pub struct Affects(pub Entity);

/// 主人身上挂着哪些修正。Bevy 自动维护：
///
/// - 插一条带 [`Affects(自己)`](Affects) 的修正实体，名单就多一项；
/// - 销毁修正实体，名单自动摘掉；**最后一个也没了，这个组件自己也会消失** ✗
///   （所以读它的人要用 `Option<&Modifiers>`）；
/// - 主人被销毁，名单上的修正实体一起销毁（`linked_spawn`）。
#[derive(Component, Default)]
#[relationship_target(relationship = Affects, linked_spawn)]
pub struct Modifiers(Vec<Entity>);

impl Modifiers {
    /// 名单里的修正实体。
    pub fn iter(&self) -> impl Iterator<Item = Entity> + '_ {
        self.0.iter().copied()
    }

    /// 名单里有多少条。
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// 空不空。
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

// ==================== 修正 ====================

/// 一条修正：平加 + 比例。**这就是修正实体上挂的组件。**
///
/// - **平加** `flat`：绝对值（`+5`；负的就是减）。
/// - **比例** `ratio`：**raw** 的比例（`0.5` = `+50%`；负的就是按比例减）。
///   注意是按 raw，不是按“已经加过别的修正之后的值”——这样叠加才可交换、可并行。
///
/// 两个值可以同时有，也可以只写一个（另一个留 0）。
/// 同一个量可以同时有多条修正——有加也有减，它们一起归约。
///
/// 它挂在哪个实体上、算在谁头上（[`Affects`]）是两件事：内容里用 `Modifiers [ … ]` 列表写，
/// 归属由宏自动补。
#[derive(Component, Clone, Debug)]
pub struct Modifier<Mark> {
    /// 平加。
    pub flat: f32,
    /// 比例。
    pub ratio: f32,
    /// 修正指向哪个量，靠类型表达。
    _marker: PhantomData<Mark>,
}

// 手写 `Default`：派生版本会给 `Mark` 加约束，而 `bsn!` 里 `ModStrength { flat: 5.0 }`
// 这种"只写几个字段"的写法需要它在泛型上就有默认。
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

    fn mods(items: impl IntoIterator<Item = (f32, f32)>) -> Vec<Modifier<TestValue>> {
        items
            .into_iter()
            .map(|(flat, ratio)| Modifier::new(flat, ratio))
            .collect()
    }

    /// 求和。**真实项目里这段在 `voxelith_rules::recompute` 里**；这里只把公式语义钉住。
    fn sum(raw: f32, modifiers: &[Modifier<TestValue>]) -> f32 {
        raw + modifiers.iter().map(|m| m.contribution(raw)).sum::<f32>()
    }

    #[test]
    fn no_modifiers_means_raw() {
        assert_eq!(sum(10.0, &[]), 10.0);
    }

    #[test]
    fn flat_contributes_its_amount() {
        assert_eq!(sum(10.0, &mods([(5.0, 0.0)])), 15.0);
    }

    #[test]
    fn ratio_contributes_a_share_of_raw() {
        assert_eq!(sum(10.0, &mods([(0.0, 0.5)])), 15.0);
    }

    /// **一条修正可以同时带平加和比例**，两者分开算再相加。
    #[test]
    fn one_modifier_can_carry_both() {
        // 10 + 5 + 10 × 0.5 = 20
        assert_eq!(sum(10.0, &mods([(5.0, 0.5)])), 20.0);
    }

    /// 加与减可以同时挂，一起归约。
    #[test]
    fn plus_and_minus_coexist() {
        let list = mods([(5.0, 0.0), (-3.0, 0.0), (0.0, 0.5), (0.0, -0.2)]);
        // 10 + 5 - 3 + 10 × 0.5 - 10 × 0.2 = 15
        assert_eq!(sum(10.0, &list), 15.0);
    }

    /// 平加与比例都基于 raw，所以登记顺序不影响结果。
    #[test]
    fn modifiers_commute() {
        let a = mods([(5.0, 0.0), (0.0, 0.5)]);
        let b = mods([(0.0, 0.5), (5.0, 0.0)]); // 顺序反过来

        assert_eq!(sum(10.0, &a), 20.0);
        assert_eq!(sum(10.0, &a), sum(10.0, &b));
    }

    /// 归属：名单由 Bevy 维护，含自指（派生就是"给自己挂一条"）。
    #[test]
    fn attribution_is_maintained_for_us() {
        let mut world = World::new();
        let hero = world.spawn(TestValue(10.0)).id();

        // 自己给自己挂一条
        let own = world
            .spawn((Affects(hero), Modifier::<TestValue>::flat(2.0)))
            .id();
        // 别人给它挂一条
        let other = world
            .spawn((Affects(hero), Modifier::<TestValue>::flat(5.0)))
            .id();

        let list = world.get::<Modifiers>(hero).unwrap();
        assert_eq!(list.len(), 2);
        assert!(list.iter().any(|e| e == own), "自指那条在该在名单里");
        assert!(list.iter().any(|e| e == other));

        // 销毁一条，名单自动摘掉
        world.despawn(other);
        assert_eq!(world.get::<Modifiers>(hero).unwrap().len(), 1);

        // 最后一条也没了，名单组件自己消失
        world.despawn(own);
        assert!(
            world.get::<Modifiers>(hero).is_none(),
            "空名单会连组件一起消失（读的人要用 Option）"
        );
    }

    /// 主人没了，修正跟着没（`linked_spawn`）。
    #[test]
    fn despawn_takes_modifiers_along() {
        let mut world = World::new();
        let hero = world.spawn(TestValue(10.0)).id();
        let modifier = world
            .spawn((Affects(hero), Modifier::<TestValue>::flat(5.0)))
            .id();

        world.despawn(hero);
        assert!(world.get_entity(modifier).is_err());
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
