//! 资源池。
//!
//! 每条资源有**两个**值——当前值与基础上限。**只有上限可修正**：当前值是状态，
//! 由事件（伤害、治疗、回复）改；上限走与属性同一套修正，标记是 [`Max<T>`]。
//!
//! 哪些资源人人都有、哪些按角色配置，见 `docs/design/01-attributes-and-resources.md`。

use crate::modifier::Modifier;
use crate::value::{Max, Resourceable};
use bevy_ecs::prelude::*;

/// 血量。
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Health {
    /// 当前值。状态，由事件改。
    pub current: f32,
    /// 基础上限。修正加在它上面。
    pub base_max: f32,
}

impl Resourceable for Health {
    fn current(&self) -> f32 {
        self.current
    }

    fn base_max(&self) -> f32 {
        self.base_max
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

impl Resourceable for Stamina {
    fn current(&self) -> f32 {
        self.current
    }

    fn base_max(&self) -> f32 {
        self.base_max
    }
}

// ---- 修正别名 ----

/// 血量上限的修正。
pub type ModHealthMax = Modifier<Max<Health>>;
/// 精力上限的修正。
pub type ModStaminaMax = Modifier<Max<Stamina>>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attribute::{ModStrength, Strength};
    use crate::modifier::{modifiers, ModifiesOf};
    use crate::value::Resolvable;

    /// 单值型与资源上限共用同一条公式；池的当前值不被修正改动。
    #[test]
    fn pool_limit_and_single_value_share_one_rule() {
        let mut world = World::new();

        let single = world.spawn(Strength(10.0)).id();
        world.spawn((ModifiesOf(single), ModStrength::flat(5.0)));
        world.spawn((ModifiesOf(single), ModStrength::ratio(0.5)));

        let pool = world
            .spawn(Health {
                current: 8.0,
                base_max: 30.0,
            })
            .id();
        world.spawn((ModifiesOf(pool), ModHealthMax::flat(20.0)));
        world.spawn((ModifiesOf(pool), ModHealthMax::ratio(0.5)));

        // 属性：10 + 5 + 10 × 0.5 = 20
        let strength = world.get::<Strength>(single).unwrap();
        assert_eq!(strength.resolve(modifiers::<Strength>(&world, single)), 20.0);

        // 血量上限：30 + 20 + 30 × 0.5 = 65
        let health = world.get::<Health>(pool).unwrap();
        assert_eq!(
            health.resolve_max(modifiers::<Max<Health>>(&world, pool)),
            65.0
        );
        assert_eq!(health.current(), 8.0, "当前值是状态，不该被修正改动");
    }

    /// 修正的目标是类型，所以给精力的修正不会跑到血量上。
    #[test]
    fn modifier_targets_are_distinct() {
        let mut world = World::new();

        let e = world.spawn(Health::default()).id();
        world.spawn((ModifiesOf(e), ModStaminaMax::flat(99.0)));

        let health = world.get::<Health>(e).unwrap();
        assert_eq!(
            health.resolve_max(modifiers::<Max<Health>>(&world, e)),
            0.0
        );
    }
}
