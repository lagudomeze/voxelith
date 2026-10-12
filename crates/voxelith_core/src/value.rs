//! “读出”这件事：可修正的量怎么算出最终值。
//!
//! 可修正的量只有两种形状，**但不需要泛型载体**——每个量自己就是一个类型：
//!
//! | 形状 | 怎么定义 | 例 |
//! | --- | --- | --- |
//! | **单值型** | 一个 newtype，`Mark = Self` | `Strength(pub f32)` |
//! | **资源池型** | 当前值 + 基础上限，`Mark` 指向上限那个标记 | `Health { current, base_max }` |
//!
//! 读出规则只有一条：
//!
//! ```text
//! 最终值 = 基础值 + Σ每条修正的贡献(基础值)      // 比例按基础值算
//! ```
//!
//! **只有资源池的当前值是状态**（由事件改，不走修正）；上限与单值型走同一套修正。

use crate::modifier::{modifiers, Modifier};
use bevy_ecs::prelude::*;

/// 一个有基础值、能被修正读出的量。
pub trait Resolvable {
    /// 指向这个量的修正用哪个标记。
    ///
    /// 单值型直接用自己（`type Mark = Self`）；资源池用“上限”那个标记。
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

    /// 一步到位：从世界里读出这个量的最终值（对资源池就是**最终上限**）。
    fn resolved(&self, world: &World, owner: Entity) -> f32
    where
        Self: Sized,
        Self::Mark: Send + Sync + 'static,
    {
        self.resolve(modifiers::<Self::Mark>(world, owner))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modifier::{Modifier, ModifiesOf};

    /// 单值型的最简形态：newtype + `Mark = Self`。
    #[derive(Component, Clone, Copy, Debug, Default)]
    struct TestValue(pub f32);

    impl Resolvable for TestValue {
        type Mark = Self;
        fn base(&self) -> f32 {
            self.0
        }
    }

    fn modify(world: &mut World, owner: Entity, m: Modifier<TestValue>) {
        world.spawn((ModifiesOf(owner), m));
    }

    /// 平加与比例都基于基础值，所以登记顺序不影响结果。
    #[test]
    fn modifiers_commute() {
        let mut world = World::new();

        let a = world.spawn(TestValue(10.0)).id();
        modify(&mut world, a, Modifier::flat(5.0));
        modify(&mut world, a, Modifier::ratio(0.5));

        let b = world.spawn(TestValue(10.0)).id();
        modify(&mut world, b, Modifier::ratio(0.5)); // 顺序反过来
        modify(&mut world, b, Modifier::flat(5.0));

        let a_resolved = world.get::<TestValue>(a).unwrap().resolved(&world, a);
        let b_resolved = world.get::<TestValue>(b).unwrap().resolved(&world, b);
        assert_eq!(a_resolved, 20.0);
        assert_eq!(a_resolved, b_resolved);
    }

    /// 没有修正时，读出来就是基础值。
    #[test]
    fn no_modifiers_means_base_value() {
        let mut world = World::new();
        let e = world.spawn(TestValue(7.0)).id();
        assert_eq!(world.get::<TestValue>(e).unwrap().resolved(&world, e), 7.0);
    }
}
