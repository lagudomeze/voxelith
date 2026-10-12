//! 值：**可修正的量**这一层的基础代码。
//!
//! 一个量 = 一个基础值 + 一串修正。读出规则只有一条：
//!
//! ```text
//! 最终值 = 基础值 + Σ每条修正的贡献(基础值)      // 比例按基础值算
//! ```
//!
//! 所以这里只装两件事：
//!
//! | 东西 | 是什么 |
//! | --- | --- |
//! | [`Modifier<Mark>`] | 一条修正（平加 + 比例） |
//! | [`Value`] / [`Resourceable`] / [`Max<T>`] | 读出接口与上限标记 |
//!
//! 那条公式就写在两个读出方法里，**各一行**（`Value::resolve` 与
//! `Resourceable::resolve_max`）——不足以再抽一个名字出来。
//!
//! ## 修正怎么挂到主人身上，这一层不管
//!
//! “哪些修正算数”是**具体系统**的事：装备、增益、只在某个条件下生效的东西，各自的生命周期
//! 与生效规则都不同，在这里定死一种挂法反而挡路。所以读出接口只吃一个迭代器——
//! **谁调用，谁决定喂什么**。
//!
//! 一份实测过的挂法（自定义 relationship + `linked_spawn` 自动清理 + 在 `bsn!` 里写成列表）
//! 记在 `docs/design/bevy/01-attributes-and-resources.md`；应用侧的用例见
//! `crates/voxelith_app/src/main.rs` 与 `tests/bsn.rs`。
//!
//! ## 两种形状
//!
//! | 形状 | 哪个值可修正 | 标记 |
//! | --- | --- | --- |
//! | **单值型**（属性、护甲、移动速度……） | 它本身 | 类型自己 |
//! | **资源**（血量、精力……） | **只有上限** | [`Max<T>`] |
//!
//! **当前值不走修正**：它是状态，由事件（伤害、治疗、回复）改。

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

// ==================== 读出 ====================

/// 单值型：只有一个基础值（属性、护甲、移动速度、挖掘力……）。
pub trait Value: Component + Sized {
    /// 基础值。
    fn base(&self) -> f32;

    /// 读出：基础值 + Σ每条修正的贡献（比例按基础值算）。
    ///
    /// 纯函数：不碰世界、不遍历，给定迭代器就有结果——所以顺序无关、可并行。
    fn resolve<'a>(&'a self, modifiers: impl Iterator<Item = &'a Modifier<Self>>) -> f32 {
        // 公式。（`Resourceable::resolve_max` 里有一份一样的，改的时候两处都改。）
        let base = self.base();
        base + modifiers.map(|m| m.contribution(base)).sum::<f32>()
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
        // 公式。（与 `Value::resolve` 同一行，只是标记类型不同。）
        let base = self.base_max();
        base + modifiers.map(|m| m.contribution(base)).sum::<f32>()
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

    /// 测试用的量：一个 newtype（与 `attribute` 里的属性同构）。
    #[derive(Component, Clone, Copy, Debug, Default)]
    struct TestValue(pub f32);

    impl Value for TestValue {
        fn base(&self) -> f32 {
            self.0
        }
    }

    /// **测试自己定怎么挂**：主人身上挂一串修正实体。
    ///
    /// 真实项目里这可能是装备槽、增益列表，或者任何“这个系统想算的东西”。
    #[derive(Component, Default)]
    struct Attached(Vec<Entity>);

    fn spawn_owner(
        world: &mut World,
        base: f32,
        modifiers: impl IntoIterator<Item = Modifier<TestValue>>,
    ) -> Entity {
        let attached: Vec<Entity> = modifiers
            .into_iter()
            .map(|m| world.spawn(m).id())
            .collect();
        world.spawn((TestValue(base), Attached(attached))).id()
    }

    fn total(world: &World, owner: Entity) -> f32 {
        let value = world.get::<TestValue>(owner).unwrap();
        let attached = world.get::<Attached>(owner).unwrap();
        value.resolve(
            attached
                .0
                .iter()
                .filter_map(|e| world.get::<Modifier<TestValue>>(*e)),
        )
    }

    #[test]
    fn no_modifiers_means_base_value() {
        let mut world = World::new();
        let hero = spawn_owner(&mut world, 10.0, []);

        assert_eq!(total(&world, hero), 10.0);
    }

    #[test]
    fn flat_contributes_its_amount() {
        let mut world = World::new();
        let hero = spawn_owner(&mut world, 10.0, [Modifier::flat(5.0)]);

        assert_eq!(total(&world, hero), 15.0);
    }

    #[test]
    fn ratio_contributes_a_share_of_base() {
        let mut world = World::new();
        let hero = spawn_owner(&mut world, 10.0, [Modifier::ratio(0.5)]);

        assert_eq!(total(&world, hero), 15.0);
    }

    /// **一条修正可以同时带平加和比例**，两者分开算再相加。
    #[test]
    fn one_modifier_can_carry_both() {
        let mut world = World::new();
        let hero = spawn_owner(&mut world, 10.0, [Modifier::new(5.0, 0.5)]);

        // 10 + 5 + 10 × 0.5 = 20
        assert_eq!(total(&world, hero), 20.0);
    }

    /// 加与减可以同时挂，一起归约。
    #[test]
    fn plus_and_minus_coexist() {
        let mut world = World::new();
        let hero = spawn_owner(
            &mut world,
            10.0,
            [
                Modifier::flat(5.0),
                Modifier::flat(-3.0),
                Modifier::ratio(0.5),
                Modifier::ratio(-0.2),
            ],
        );

        // 10 + 5 - 3 + 10 × 0.5 - 10 × 0.2 = 15
        assert_eq!(total(&world, hero), 15.0);
    }

    /// 平加与比例都基于基础值，所以登记顺序不影响结果。
    #[test]
    fn modifiers_commute() {
        let mut world = World::new();
        let a = spawn_owner(&mut world, 10.0, [Modifier::flat(5.0), Modifier::ratio(0.5)]);
        let b = spawn_owner(&mut world, 10.0, [Modifier::ratio(0.5), Modifier::flat(5.0)]); // 顺序反过来

        assert_eq!(total(&world, a), 20.0);
        assert_eq!(total(&world, a), total(&world, b));
    }
}
