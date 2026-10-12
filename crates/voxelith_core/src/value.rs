//! 值：**可修正的量**这一层的基础代码。
//!
//! 一个量 = 一个基础值 + 挂在它上面的一串修正。读出规则只有一条：
//!
//! ```text
//! 最终值 = 基础值 + Σ每条修正的贡献(基础值)      // 比例按基础值算
//! ```
//!
//! 本模块装的就是这件事的全部：
//!
//! | 东西 | 是什么 |
//! | --- | --- |
//! | [`Modifier<Mark>`] | 一条修正（平加 + 比例），挂在**修正实体**上 |
//! | [`ModifiesOf`] / [`Modifiers`] | 修正指回主人；主人身上那份列表由关系自动维护 |
//! | [`resolve_with`] | 上面那条公式，**全项目只此一处** |
//! | [`modifiers`] | 从世界里取修正（测试/装配用；system 里用查询） |
//! | [`Value`] / [`Resourceable`] | 读出接口：单值型 / 资源（**只到上限**） |
//! | [`Max<T>`] | 上限那个量的标记 |
//!
//! 两种形状：
//!
//! | 形状 | 哪个值可修正 | 标记 |
//! | --- | --- | --- |
//! | **单值型**（属性、护甲、移动速度……） | 它本身 | 类型自己 |
//! | **资源**（血量、精力……） | **只有上限** | [`Max<T>`] |
//!
//! **当前值不走修正**：它是状态，由事件（伤害、治疗、回复）改。
//!
//! **这里没有 `resolved(world, owner)` 这种东西**：写 system 时手里已经有组件和
//! 关联列表（`&Modifiers`），把它里面的实体喂给修正查询即可——见 `tests/system_read.rs`。

use bevy_ecs::prelude::*;
use core::marker::PhantomData;

// ==================== 修正 ====================

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
    _marker: PhantomData<Mark>,
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
/// 全项目**唯一的公式**——[`Value::resolve`] 与 [`Resourceable::resolve_max`] 都只是
/// 把它包一层。将来要加“减少型比例的下限”这类护栏，落点也是这里一处。
///
/// 它是纯函数：不碰世界、不遍历，给定迭代器就有结果——所以顺序无关、可并行。
pub fn resolve_with<'a, Mark: 'a>(
    base: f32,
    modifiers: impl Iterator<Item = &'a Modifier<Mark>>,
) -> f32 {
    base + modifiers.map(|m| m.contribution(base)).sum::<f32>()
}

// ==================== 关系：修正指回主人 ====================

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

// ==================== 读出 ====================

/// 单值型：只有一个基础值（属性、护甲、移动速度、挖掘力……）。
pub trait Value: Component + Sized {
    /// 基础值。
    fn base(&self) -> f32;

    /// 读出：基础值 + Σ每条修正的贡献。
    fn resolve<'a>(&'a self, modifiers: impl Iterator<Item = &'a Modifier<Self>>) -> f32 {
        resolve_with(self.base(), modifiers)
    }
}

/// 资源：当前值 + 上限。**只有上限可修正。**
pub trait Resourceable: Component + Sized {
    /// 当前值（状态，由事件改）。
    fn current(&self) -> f32;

    /// 上限的基础值。
    fn base_max(&self) -> f32;

    /// 读出**最终上限**。当前值不走这里。
    fn resolve_max<'a>(&'a self, modifiers: impl Iterator<Item = &'a Modifier<Max<Self>>>) -> f32 {
        resolve_with(self.base_max(), modifiers)
    }
}

/// 上限那个量。
///
/// 资源有两个值，所以上限需要一个名字；单值型只有一个值，标记就是它自己。
#[derive(Clone, Copy, Debug, Default)]
pub struct Max<T>(PhantomData<T>);

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
    fn no_modifiers_means_base_value() {
        let mut world = World::new();
        let hero = world.spawn_empty().id();

        assert_eq!(total(&world, hero, 10.0), 10.0);
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

    /// 平加与比例都基于基础值，所以登记顺序不影响结果。
    #[test]
    fn modifiers_commute() {
        let mut world = World::new();

        let a = world.spawn_empty().id();
        spawn_modifier(&mut world, a, Modifier::flat(5.0));
        spawn_modifier(&mut world, a, Modifier::ratio(0.5));

        let b = world.spawn_empty().id();
        spawn_modifier(&mut world, b, Modifier::ratio(0.5)); // 顺序反过来
        spawn_modifier(&mut world, b, Modifier::flat(5.0));

        assert_eq!(total(&world, a, 10.0), total(&world, b, 10.0));
        assert_eq!(total(&world, a, 10.0), 20.0);
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
