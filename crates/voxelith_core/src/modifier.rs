//! 修正：别的东西怎么改一个量。
//!
//! 一个修正 = **一个实体**（带 [`Flat`] 或 [`Ratio`]），通过 [`ModifiesOf`] 指回主人；
//! 主人身上的 [`Modifiers`] 由关系自动维护（`linked_spawn`：主人销毁时一起清理）。
//!
//! 读出分两步（见 `docs/design/01-attributes-and-resources.md`）：
//! [`collect`] 收集出总和，[`Totals::resolve`] 是那个纯函数的公式。

use bevy_ecs::prelude::*;
use core::marker::PhantomData;

/// 平加：加一个绝对值（`+5 力量`）。
#[derive(Component, Clone, Debug, Default)]
pub struct Flat<M> {
    pub amount: f32,
    pub _marker: PhantomData<M>,
}

impl<M> Flat<M> {
    pub fn new(amount: f32) -> Self {
        Self {
            amount,
            _marker: PhantomData,
        }
    }
}

/// 比例：加“**基础值**”的一个比例（`0.5` = `+50% 基础值`）。
///
/// 注意是按基础值，不是按“已经加过别的修正之后的值”——这样叠加才可交换、可并行。
#[derive(Component, Clone, Debug, Default)]
pub struct Ratio<M> {
    pub value: f32,
    pub _marker: PhantomData<M>,
}

impl<M> Ratio<M> {
    pub fn new(value: f32) -> Self {
        Self {
            value,
            _marker: PhantomData,
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

/// 指向某一个量的修正总和。
#[derive(Default, Clone, Copy, Debug, PartialEq)]
pub struct Totals {
    /// 平加之和。
    pub add: f32,
    /// 比例之和。
    pub ratio: f32,
}

impl Totals {
    /// 公式：`基础值 + Σ平加 + 基础值 × Σ比例`。
    ///
    /// 纯函数——不带世界、不遍历，顺序无关，可并行算完再归约。
    pub fn resolve(&self, base: f32) -> f32 {
        base + self.add + base * self.ratio
    }
}

/// 收集指向某个标记的所有修正（只读、不计算）。
///
/// 走主人自己的 [`Modifiers`] 组件 + 那几个修正实体，**不做全库扫描**。
pub fn collect<M: Send + Sync + 'static>(world: &World, owner: Entity) -> Totals {
    let mut totals = Totals::default();
    if let Some(mods) = world.get::<Modifiers>(owner) {
        for m in mods.iter() {
            if let Some(a) = world.get::<Flat<M>>(m) {
                totals.add += a.amount;
            }
            if let Some(r) = world.get::<Ratio<M>>(m) {
                totals.ratio += r.value;
            }
        }
    }
    totals
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试用的量：不依赖具体属性或资源。
    #[derive(Clone, Copy, Debug, Default)]
    struct TestStat;

    #[test]
    fn formula_is_a_pure_function() {
        let totals = Totals {
            add: 5.0,
            ratio: 0.5,
        };
        assert_eq!(totals.resolve(10.0), 20.0);
    }

    /// 平加与比例都基于基础值，所以登记顺序不影响结果。
    #[test]
    fn modifiers_commute() {
        let mut world = World::new();

        let a = world.spawn_empty().id();
        world.spawn((ModifiesOf(a), Flat::<TestStat>::new(5.0)));
        world.spawn((ModifiesOf(a), Ratio::<TestStat>::new(0.5)));

        let b = world.spawn_empty().id();
        world.spawn((ModifiesOf(b), Ratio::<TestStat>::new(0.5))); // 顺序反过来
        world.spawn((ModifiesOf(b), Flat::<TestStat>::new(5.0)));

        assert_eq!(collect::<TestStat>(&world, a).resolve(10.0), 20.0);
        assert_eq!(collect::<TestStat>(&world, b).resolve(10.0), 20.0);
    }

    /// 主人销毁时，修正实体随 `linked_spawn` 一起消失。
    #[test]
    fn modifiers_die_with_their_owner() {
        let mut world = World::new();

        let owner = world.spawn_empty().id();
        let m1 = world.spawn((ModifiesOf(owner), Flat::<TestStat>::new(5.0))).id();
        let m2 = world.spawn((ModifiesOf(owner), Flat::<TestStat>::new(3.0))).id();

        assert_eq!(collect::<TestStat>(&world, owner).resolve(10.0), 18.0);

        world.despawn(owner);
        assert!(world.get_entity(owner).is_err());
        assert!(world.get_entity(m1).is_err());
        assert!(world.get_entity(m2).is_err());
    }
}
