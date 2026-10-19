//! 代价：**给一条量标个价**。
//!
//! [`Cost<Q>`] 说的是"这次行动要扣 `Q` 这条量多少"。**一种量一个组件** ✓ 所以一个行动可以同时要两种：
//!
//! ```ignore
//! ManaCost(10.0)      // = Cost<Mana>(10.0)
//! StaminaCost(5.0)    // = Cost<Stamina>(5.0)
//! ```
//!
//! 它只是**数据** ✓ 检查与扣减在 `voxelith_rules`（那一层认识资源清单）✓
//! 谁在什么时候付、付不起怎么办，见 [`docs/design/04-time-and-actions.md`](../../../docs/design/04-time-and-actions.md)。

use crate::value::Value;
use bevy_ecs::prelude::*;
use core::marker::PhantomData;

/// 这次行动要扣某条量多少。
///
/// 第二个字段是"扣哪条量"的类型标记 —— 和 `Max<Q>` / `Total<Mark>` 同一个套路 ✓
/// `bsn!` 里用别名写 `ManaCost(10.0)` 就行（未指定的字段默认）✓
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Cost<Q: Value>(pub f32, PhantomData<Q>);

impl<Q: Value> Cost<Q> {
    /// 给一条量标个价。
    pub fn new(amount: f32) -> Self {
        Self(amount, PhantomData)
    }

    /// 要多少。
    pub fn amount(&self) -> f32 {
        self.0
    }
}

// 手写 `Default`：派生版本会给 `Q` 加约束，而 `bsn!` 的 `{ … }` 写法与 `#[require]` 都需要
// 它在泛型上就有默认（与 `Max` / `Total` / `Modifier` 同样的理由）。
impl<Q: Value> Default for Cost<Q> {
    fn default() -> Self {
        Self::new(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource::{Health, Mana};

    #[test]
    fn cost_carries_which_quantity_and_how_much() {
        let mana = Cost::<Mana>::new(10.0);
        let health = Cost::<Health>::new(3.0);

        assert_eq!(mana.amount(), 10.0);
        assert_eq!(health.amount(), 3.0);
        assert_eq!(Cost::<Mana>::default().amount(), 0.0, "默认不要钱");
    }

    /// 一种量一个组件：同一个实体上可以同时挂两条不同量的代价。
    #[test]
    fn one_cost_per_quantity_can_coexist() {
        let mut world = World::new();
        let entity = world.spawn((Cost::<Mana>::new(10.0), Cost::<Health>::new(3.0))).id();

        assert_eq!(world.get::<Cost<Mana>>(entity).unwrap().amount(), 10.0);
        assert_eq!(world.get::<Cost<Health>>(entity).unwrap().amount(), 3.0);
    }
}
