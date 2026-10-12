//! 修正：别的东西怎么改一个量。
//!
//! 一条修正 = **一个实体**，上面挂一个 [`Modifier<Mark>`]，通过 [`ModifiesOf`] 指回主人；
//! 主人身上的 [`Modifiers`] 由关系自动维护（`linked_spawn`：主人销毁时一起清理）。
//!
//! 公式不在这里，在 [`crate::value::Resolvable::resolve`]：它遍历修正，
//! 每条问一句 [`Modifier::contribution`]。

use bevy_ecs::prelude::*;
use bevy_ecs::VariantDefaults;
use core::marker::PhantomData;

/// 一条修正：平加或比例。**这就是挂在修正实体上的组件。**
///
/// 值是**有符号**的：负的平加就是减，负的比例就是按比例减。
/// 同一个量可以同时挂多条修正——有加也有减，它们一起归约。
///
/// 两个变体各带一个 `PhantomData<Mark>`：**修正指向哪个量是靠类型表达的**，
/// 而值本身用不到 `Mark`，不用 phantom 会报 `E0392`（泛型参数没被使用）。
///
/// 泛型参数叫 `Mark` 而不是 `M`：`Component` 派生在泛型枚举上会自己引入一个 `M`，
/// 撞名报 `E0403`（实测）。
#[derive(Component, Clone, Debug, VariantDefaults)]
pub enum Modifier<Mark> {
    /// 平加：加一个绝对值（负 = 减）。
    Flat(f32, PhantomData<Mark>),
    /// 比例：加“**基础值**”的一个比例（`0.5` = `+50% 基础值`；负 = 按比例减）。
    ///
    /// 注意是按基础值，不是按“已经加过别的修正之后的值”——这样叠加才可交换、可并行。
    Ratio(f32, PhantomData<Mark>),
}

// 手写 `Default`：`bsn!` 需要它（`Clone + Default` 才有那套自动 Template）。
// 泛型枚举的 `#[derive(Default)]` 只认无数据的变体，所以这里直接给一个“空平加”——贡献 0。
impl<Mark> Default for Modifier<Mark> {
    fn default() -> Self {
        Self::flat(0.0)
    }
}

impl<Mark> Modifier<Mark> {
    /// 平加。
    pub fn flat(amount: f32) -> Self {
        Self::Flat(amount, PhantomData)
    }

    /// 比例。
    pub fn ratio(ratio: f32) -> Self {
        Self::Ratio(ratio, PhantomData)
    }

    /// 这条修正贡献多少（比例按**基础值**算）。
    pub fn contribution(&self, base: f32) -> f32 {
        match self {
            Modifier::Flat(amount, _) => *amount,
            Modifier::Ratio(ratio, _) => base * ratio,
        }
    }
}

/// 修正指回被修正的角色。
///
/// `Entity` 没有 `Default`，所以必须派生 `FromTemplate`，`bsn!` 才能写它。
#[derive(Component, FromTemplate)]
#[relationship(relationship_target = Modifiers)]
pub struct ModifiesOf(pub Entity);

/// 被修正者身上自动维护的修正列表。
#[derive(Component, Default)]
#[relationship_target(relationship = ModifiesOf, linked_spawn)]
pub struct Modifiers(Vec<Entity>);

/// 遍历主人身上的修正。
///
/// 走主人自己的 [`Modifiers`] 组件 + 那几个修正实体，**不做全库扫描**。
pub fn modifiers<'w, Mark: Send + Sync + 'static>(
    world: &'w World,
    owner: Entity,
) -> impl Iterator<Item = &'w Modifier<Mark>> {
    world
        .get::<Modifiers>(owner)
        .into_iter()
        .flat_map(move |mods| mods.iter().filter_map(move |m| world.get::<Modifier<Mark>>(m)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试用的量：不依赖具体属性或资源。
    #[derive(Clone, Copy, Debug, Default)]
    struct TestStat;

    fn total(world: &World, owner: Entity, base: f32) -> f32 {
        modifiers::<TestStat>(world, owner)
            .map(|m| m.contribution(base))
            .sum()
    }

    #[test]
    fn flat_contributes_its_amount() {
        let mut world = World::new();
        let hero = world.spawn_empty().id();
        world.spawn((ModifiesOf(hero), Modifier::<TestStat>::flat(5.0)));

        assert_eq!(total(&world, hero, 10.0), 5.0);
    }

    #[test]
    fn ratio_contributes_a_share_of_base() {
        let mut world = World::new();
        let hero = world.spawn_empty().id();
        world.spawn((ModifiesOf(hero), Modifier::<TestStat>::ratio(0.5)));

        assert_eq!(total(&world, hero, 10.0), 5.0);
    }

    /// 加与减可以同时挂，一起归约。
    #[test]
    fn plus_and_minus_coexist() {
        let mut world = World::new();
        let hero = world.spawn_empty().id();
        world.spawn((ModifiesOf(hero), Modifier::<TestStat>::flat(5.0)));
        world.spawn((ModifiesOf(hero), Modifier::<TestStat>::flat(-3.0)));
        world.spawn((ModifiesOf(hero), Modifier::<TestStat>::ratio(0.5)));
        world.spawn((ModifiesOf(hero), Modifier::<TestStat>::ratio(-0.2)));

        // 5 - 3 + 10 × 0.5 - 10 × 0.2 = 5
        assert_eq!(total(&world, hero, 10.0), 5.0);
    }

    /// 主人销毁时，修正实体随 `linked_spawn` 一起消失。
    #[test]
    fn modifiers_die_with_their_owner() {
        let mut world = World::new();

        let owner = world.spawn_empty().id();
        let m1 = world
            .spawn((ModifiesOf(owner), Modifier::<TestStat>::flat(5.0)))
            .id();
        let m2 = world
            .spawn((ModifiesOf(owner), Modifier::<TestStat>::flat(3.0)))
            .id();

        assert_eq!(modifiers::<TestStat>(&world, owner).count(), 2);

        world.despawn(owner);
        assert!(world.get_entity(owner).is_err());
        assert!(world.get_entity(m1).is_err());
        assert!(world.get_entity(m2).is_err());
    }
}
