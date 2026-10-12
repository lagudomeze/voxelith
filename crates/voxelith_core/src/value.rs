//! 可修正的量的**两种载体**，以及“读出”这件事本身。
//!
//! - [`Value`]：**单值型**——只有一个基础值（属性、护甲、移动速度、挖掘力……）。
//! - [`Pool`]：**资源池型**——一个当前值 + 一个基础上限（血量、精力、法力……）。
//!
//! 两者都实现 [`Resolvable`]，读出规则只有一条：
//!
//! ```text
//! 最终值 = 基础值 + Σ每条修正的贡献(基础值)      // 比例按基础值算
//! ```
//!
//! **只有 `Pool` 的当前值是状态**（由事件改，不走修正）；上限与单值型走同一套修正。

use crate::modifier::{modifiers, Modifier};
use bevy_ecs::prelude::*;
use core::marker::PhantomData;

/// 一个有基础值、能被修正读出的量。
pub trait Resolvable {
    /// 指向这个量的修正用哪个标记。
    type Mark;

    /// 基础值。
    fn base(&self) -> f32;

    /// 读出：**基础值 + Σ每条修正的贡献**。
    ///
    /// 纯函数（给定迭代器）——顺序无关，所以可交换、可并行。
    fn resolve<'a>(&'a self, modifiers: impl Iterator<Item = &'a Modifier<Self::Mark>>) -> f32
    where
        Self: Sized,
    {
        let base = self.base();
        base + modifiers.map(|m| m.contribution(base)).sum::<f32>()
    }
}

/// 单值型：只有一个基础值。
#[derive(Component, Clone, Debug, Default)]
pub struct Value<M> {
    pub base: f32,
    pub _marker: PhantomData<M>,
}

impl<M: Send + Sync + 'static> Value<M> {
    pub fn new(base: f32) -> Self {
        Self {
            base,
            _marker: PhantomData,
        }
    }

    /// 一步到位：读出这个量在当前世界里的最终值。
    pub fn resolved(&self, world: &World, owner: Entity) -> f32 {
        self.resolve(modifiers::<M>(world, owner))
    }
}

impl<M: Send + Sync + 'static> Resolvable for Value<M> {
    type Mark = M;

    fn base(&self) -> f32 {
        self.base
    }
}

/// 资源池型：一个当前值 + 一个基础上限。
#[derive(Component, Clone, Debug, Default)]
pub struct Pool<M> {
    pub current: f32,
    pub base_max: f32,
    pub _marker: PhantomData<M>,
}

impl<M: Send + Sync + 'static> Pool<M> {
    pub fn new(current: f32, base_max: f32) -> Self {
        Self {
            current,
            base_max,
            _marker: PhantomData,
        }
    }

    /// 一步到位：读出**最终上限**。当前值不走这里——它是状态。
    pub fn resolved_max(&self, world: &World, owner: Entity) -> f32 {
        self.resolve(modifiers::<M>(world, owner))
    }
}

impl<M: Send + Sync + 'static> Resolvable for Pool<M> {
    type Mark = M;

    fn base(&self) -> f32 {
        self.base_max
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modifier::{Flat, ModifiesOf, Ratio};

    #[derive(Clone, Copy, Debug, Default)]
    struct TestStat;

    /// 单值型与资源池型走同一条读出规则；池的当前值不被修正改动。
    #[test]
    fn pool_max_and_value_share_one_rule() {
        let mut world = World::new();

        let single = world.spawn(Value::<TestStat>::new(30.0)).id();
        world.spawn((ModifiesOf(single), Modifier::<TestStat>::Flat(Flat::new(20.0))));
        world.spawn((ModifiesOf(single), Modifier::<TestStat>::Ratio(Ratio::new(0.5))));

        let pool = world.spawn(Pool::<TestStat>::new(8.0, 30.0)).id();
        world.spawn((ModifiesOf(pool), Modifier::<TestStat>::Flat(Flat::new(20.0))));
        world.spawn((ModifiesOf(pool), Modifier::<TestStat>::Ratio(Ratio::new(0.5))));

        // 两边算出来一模一样：30 + 20 + 30 × 0.5 = 65
        let single_value = world.get::<Value<TestStat>>(single).unwrap();
        assert_eq!(single_value.resolved(&world, single), 65.0);

        let pool_value = world.get::<Pool<TestStat>>(pool).unwrap();
        assert_eq!(pool_value.resolved_max(&world, pool), 65.0);
        assert_eq!(pool_value.current, 8.0, "当前值是状态，不该被修正改动");
    }

    /// 平加与比例都基于基础值，所以登记顺序不影响结果。
    #[test]
    fn modifiers_commute() {
        let mut world = World::new();

        let a = world.spawn(Value::<TestStat>::new(10.0)).id();
        world.spawn((ModifiesOf(a), Modifier::<TestStat>::Flat(Flat::new(5.0))));
        world.spawn((ModifiesOf(a), Modifier::<TestStat>::Ratio(Ratio::new(0.5))));

        let b = world.spawn(Value::<TestStat>::new(10.0)).id();
        world.spawn((ModifiesOf(b), Modifier::<TestStat>::Ratio(Ratio::new(0.5)))); // 顺序反过来
        world.spawn((ModifiesOf(b), Modifier::<TestStat>::Flat(Flat::new(5.0))));

        let a_resolved = world.get::<Value<TestStat>>(a).unwrap().resolved(&world, a);
        let b_resolved = world.get::<Value<TestStat>>(b).unwrap().resolved(&world, b);
        assert_eq!(a_resolved, 20.0);
        assert_eq!(a_resolved, b_resolved);
    }
}
