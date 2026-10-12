//! 资源池。
//!
//! 每条资源有**两个**值——当前值与基础上限——所以它需要一个**上限的标记**
//! （属性那边不需要：单值型的 `Mark` 就是它自己）。
//!
//! **当前值是状态**——由事件改，不走修正；**上限走修正**，与属性共用同一套机制。
//! 哪些资源人人都有、哪些按角色配置，见 `docs/design/01-attributes-and-resources.md`。

use crate::modifier::{Flat, Modifier, Ratio};
use crate::value::Resolvable;
use bevy_ecs::prelude::*;

/// 血量。
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Health {
    /// 当前值。状态，由事件改。
    pub current: f32,
    /// 基础上限。修正加在它上面。
    pub base_max: f32,
}

/// 血量上限那个量。
#[derive(Clone, Copy, Debug, Default)]
pub struct HealthMax;

impl Resolvable for Health {
    type Mark = HealthMax;

    fn base(&self) -> f32 {
        self.base_max
    }
}

impl Health {
    /// 一步到位：读出**最终上限**。当前值不走这里——它是状态。
    pub fn resolved_max(&self, world: &World, owner: Entity) -> f32 {
        self.resolved(world, owner)
    }
}

/// 精力。
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Stamina {
    /// 当前值。状态，由事件改。
    pub current: f32,
    /// 基础上限。修正加在它上面。
    pub base_max: f32,
}

/// 精力上限那个量。
#[derive(Clone, Copy, Debug, Default)]
pub struct StaminaMax;

impl Resolvable for Stamina {
    type Mark = StaminaMax;

    fn base(&self) -> f32 {
        self.base_max
    }
}

impl Stamina {
    /// 一步到位：读出**最终上限**。当前值不走这里——它是状态。
    pub fn resolved_max(&self, world: &World, owner: Entity) -> f32 {
        self.resolved(world, owner)
    }
}

// ---- 修正别名 ----

/// 血量上限的修正。
pub type ModHealthMax = Modifier<HealthMax>;
/// 精力上限的修正。
pub type ModStaminaMax = Modifier<StaminaMax>;

/// 血量上限平加。
pub type FlatHealthMax = Flat<HealthMax>;
/// 血量上限比例。
pub type RatioHealthMax = Ratio<HealthMax>;
/// 精力上限平加。
pub type FlatStaminaMax = Flat<StaminaMax>;
/// 精力上限比例。
pub type RatioStaminaMax = Ratio<StaminaMax>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attribute::{ModStrength, Strength};
    use crate::modifier::ModifiesOf;
    use crate::value::Resolvable;

    /// 单值型与资源上限共用一条读出规则；池的当前值不被修正改动。
    #[test]
    fn pool_limit_and_single_value_share_one_rule() {
        let mut world = World::new();

        let single = world.spawn(Strength(10.0)).id();
        world.spawn((ModifiesOf(single), ModStrength::Flat(Flat::new(5.0))));
        world.spawn((ModifiesOf(single), ModStrength::Ratio(Ratio::new(0.5))));

        let pool = world
            .spawn(Health {
                current: 8.0,
                base_max: 30.0,
            })
            .id();
        world.spawn((ModifiesOf(pool), ModHealthMax::Flat(Flat::new(20.0))));
        world.spawn((ModifiesOf(pool), ModHealthMax::Ratio(Ratio::new(0.5))));

        // 属性：10 + 5 + 10 × 0.5 = 20
        let single_value = world.get::<Strength>(single).unwrap();
        assert_eq!(single_value.resolved(&world, single), 20.0);

        // 血量上限：30 + 20 + 30 × 0.5 = 65
        let health = world.get::<Health>(pool).unwrap();
        assert_eq!(health.resolved_max(&world, pool), 65.0);
        assert_eq!(health.current, 8.0, "当前值是状态，不该被修正改动");
    }

    /// 修正的目标是类型，所以给精力的修正不会跑到血量上。
    #[test]
    fn modifier_targets_are_distinct() {
        let mut world = World::new();

        let e = world.spawn(Health::default()).id();
        world.spawn((ModifiesOf(e), ModStaminaMax::Flat(Flat::new(99.0))));

        let health = world.get::<Health>(e).unwrap();
        assert_eq!(health.resolved_max(&world, e), 0.0);
    }
}
