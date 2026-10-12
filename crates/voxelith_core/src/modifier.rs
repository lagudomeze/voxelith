//! 修正：别的东西怎么改一个量。
//!
//! 一条修正 = **一个实体**，上面挂一个 [`Modifier<Mark>`]，通过 [`ModifiesOf`] 指回主人；
//! 主人身上的 [`Modifiers`] 由关系自动维护（`linked_spawn`：主人销毁时一起清理）。
//!
//! 一条修正可以**同时**带平加和比例（`+5 力量 且 +50% 基础力量`）——两者分开算、再相加。

use bevy_ecs::prelude::*;
use core::marker::PhantomData;

/// 一条修正：平加 + 比例。**这就是挂在修正实体上的组件。**
///
/// - **平加** `flat`：绝对值（`+5`；负的就是减）。
/// - **比例** `ratio`：**基础值**的比例（`0.5` = `+50% 基础值`；负的就是按比例减）。
///   注意是按基础值，不是按“已经加过别的修正之后的值”——这样叠加才可交换、可并行。
///
/// 两个值可以同时有，也可以只写一个（另一个留 0）。
/// 同一个量可以同时挂多条修正——有加也有减，它们一起归约。
#[derive(Component, Clone, Debug, Default)]
pub struct Modifier<Mark> {
    /// 平加。
    pub flat: f32,
    /// 比例。
    pub ratio: f32,
    /// 修正指向哪个量，靠类型表达。
    pub _marker: PhantomData<Mark>,
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
    pub fn contribution(&self, base: f32) -> f32 {
        self.flat + base * self.ratio
    }
}

/// 读出公式：**基础值 + Σ每条修正的贡献**（比例按基础值算）。
///
/// 全项目**唯一的公式**——单值型（[`crate::value::Resolvable::resolve`]）与资源上限
/// （[`crate::value::Resourceable::resolve_max`]）都只是把它包一层。
///
/// 它是纯函数：不碰世界、不遍历，给定迭代器就有结果——所以顺序无关、可并行。
pub fn resolve_with<'a, Mark: 'a>(
    base: f32,
    modifiers: impl Iterator<Item = &'a Modifier<Mark>>,
) -> f32 {
    base + modifiers.map(|m| m.contribution(base)).sum::<f32>()
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
/// system 里不需要它——那里已经有 `&Modifiers` 和修正查询了；这是给测试与装配用的。
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

    fn spawn_modifier(world: &mut World, owner: Entity, m: Modifier<TestStat>) {
        world.spawn((ModifiesOf(owner), m));
    }

    fn total(world: &World, owner: Entity, base: f32) -> f32 {
        resolve_with(base, modifiers::<TestStat>(world, owner))
    }

    #[test]
    fn flat_contributes_its_amount() {
        let mut world = World::new();
        let hero = world.spawn_empty().id();
        spawn_modifier(&mut world, hero, Modifier::flat(5.0));

        assert_eq!(total(&world, hero, 10.0), 15.0);
    }

    #[test]
    fn ratio_contributes_a_share_of_base() {
        let mut world = World::new();
        let hero = world.spawn_empty().id();
        spawn_modifier(&mut world, hero, Modifier::ratio(0.5));

        assert_eq!(total(&world, hero, 10.0), 15.0);
    }

    /// **一条修正可以同时带平加和比例**，两者分开算再相加。
    #[test]
    fn one_modifier_can_carry_both() {
        let mut world = World::new();
        let hero = world.spawn_empty().id();
        spawn_modifier(&mut world, hero, Modifier::new(5.0, 0.5));

        // 10 + 5 + 10 × 0.5 = 20
        assert_eq!(total(&world, hero, 10.0), 20.0);
    }

    /// 加与减可以同时挂，一起归约。
    #[test]
    fn plus_and_minus_coexist() {
        let mut world = World::new();
        let hero = world.spawn_empty().id();
        spawn_modifier(&mut world, hero, Modifier::flat(5.0));
        spawn_modifier(&mut world, hero, Modifier::flat(-3.0));
        spawn_modifier(&mut world, hero, Modifier::ratio(0.5));
        spawn_modifier(&mut world, hero, Modifier::ratio(-0.2));

        // 10 + 5 - 3 + 10 × 0.5 - 10 × 0.2 = 15
        assert_eq!(total(&world, hero, 10.0), 15.0);
    }

    /// 主人销毁时，修正实体随 `linked_spawn` 一起消失。
    #[test]
    fn modifiers_die_with_their_owner() {
        let mut world = World::new();

        let owner = world.spawn_empty().id();
        let m1 = world.spawn((ModifiesOf(owner), Modifier::<TestStat>::flat(5.0))).id();
        let m2 = world.spawn((ModifiesOf(owner), Modifier::<TestStat>::flat(3.0))).id();

        assert_eq!(modifiers::<TestStat>(&world, owner).count(), 2);

        world.despawn(owner);
        assert!(world.get_entity(owner).is_err());
        assert!(world.get_entity(m1).is_err());
        assert!(world.get_entity(m2).is_err());
    }
}
