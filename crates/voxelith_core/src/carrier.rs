//! 可修正的量的**两种载体**。
//!
//! - [`Value`]：**单值型**——只有一个基础值（属性、护甲、移动速度、挖掘力……）。
//! - [`Pool`]：**资源池型**——一个当前值 + 一个基础上限（血量、精力、法力……）。
//!
//! 读出分两步：两步都在这里给到——一步到位的入口是本模块的 [`resolve_value`] /
//! [`resolve_max`]，它们内部走 [`crate::modifier::collect`] 与
//! [`crate::modifier::Totals::resolve`]。
//!
//! **只有 `Pool` 的当前值是状态**（由事件改，不走修正）；上限走与单值型同一套修正。

use crate::modifier::collect;
use bevy_ecs::prelude::*;
use core::marker::PhantomData;

/// 单值型：只有一个基础值。
#[derive(Component, Clone, Debug, Default)]
pub struct Value<M> {
    pub base: f32,
    pub _marker: PhantomData<M>,
}

impl<M> Value<M> {
    pub fn new(base: f32) -> Self {
        Self {
            base,
            _marker: PhantomData,
        }
    }
}

/// 资源池型：一个当前值 + 一个基础上限。
#[derive(Component, Clone, Debug, Default)]
pub struct Pool<M> {
    pub current: f32,
    pub base_max: f32,
    pub _marker: PhantomData<M>,
}

impl<M> Pool<M> {
    pub fn new(current: f32, base_max: f32) -> Self {
        Self {
            current,
            base_max,
            _marker: PhantomData,
        }
    }
}

/// 单值型：读出“基础值 + 修正”。
pub fn resolve_value<M: Send + Sync + 'static>(world: &World, owner: Entity) -> Option<f32> {
    let base = world.get::<Value<M>>(owner)?.base;
    Some(collect::<M>(world, owner).resolve(base))
}

/// 资源池型：读出“最终上限”。当前值不走这里——它是状态。
pub fn resolve_max<M: Send + Sync + 'static>(world: &World, owner: Entity) -> Option<f32> {
    let base = world.get::<Pool<M>>(owner)?.base_max;
    Some(collect::<M>(world, owner).resolve(base))
}

#[cfg(test)]
mod tests {
    use crate::modifier::{Flat, ModifiesOf, Ratio};
    use super::*;

    #[derive(Clone, Copy, Debug, Default)]
    struct TestStat;

    /// 资源上限与单值型共用同一套修正；当前值不被修正改动。
    #[test]
    fn pool_max_uses_the_same_modifiers_as_value() {
        let mut world = World::new();

        let single = world.spawn(Value::<TestStat>::new(30.0)).id();
        world.spawn((ModifiesOf(single), Flat::<TestStat>::new(20.0)));
        world.spawn((ModifiesOf(single), Ratio::<TestStat>::new(0.5)));

        let pool = world.spawn(Pool::<TestStat>::new(8.0, 30.0)).id();
        world.spawn((ModifiesOf(pool), Flat::<TestStat>::new(20.0)));
        world.spawn((ModifiesOf(pool), Ratio::<TestStat>::new(0.5)));

        // 两边算出来一模一样：30 + 20 + 30 × 0.5 = 65
        assert_eq!(resolve_value::<TestStat>(&world, single), Some(65.0));
        assert_eq!(resolve_max::<TestStat>(&world, pool), Some(65.0));
        // 当前值不动
        assert_eq!(world.get::<Pool<TestStat>>(pool).unwrap().current, 8.0);
    }
}
