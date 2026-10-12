//! 资源池。
//!
//! 每条资源有**两个**值——当前值与基础上限。**只有上限可修正**：当前值是状态，
//! 由事件（伤害、治疗、回复）改；上限走与属性同一套修正，标记是 `Max<T>`。
//!
//! 哪些资源人人都有、哪些按角色配置，见 `docs/design/01-attributes-and-resources.md`。

use crate::value::{Max, Modifier, Resourceable};
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
    use crate::value::Value;

    /// 测试自己定怎么挂（同 `value` 模块里那份说明）。
    #[derive(Component, Default)]
    struct Attached(Vec<Entity>);

    /// 单值型与资源上限共用同一条公式；池的当前值不被修正改动。
    #[test]
    fn pool_limit_and_single_value_share_one_rule() {
        let mut world = World::new();

        // 属性：挂两条修正
        let single = world.spawn(Strength(10.0)).id();
        let mods: Vec<Entity> = [ModStrength::flat(5.0), ModStrength::ratio(0.5)]
            .into_iter()
            .map(|m| world.spawn(m).id())
            .collect();
        world.entity_mut(single).insert(Attached(mods));

        // 血量上限：挂两条修正
        let pool = world
            .spawn(Health {
                current: 8.0,
                base_max: 30.0,
            })
            .id();
        let mods: Vec<Entity> = [ModHealthMax::flat(20.0), ModHealthMax::ratio(0.5)]
            .into_iter()
            .map(|m| world.spawn(m).id())
            .collect();
        world.entity_mut(pool).insert(Attached(mods));

        let attached = world.get::<Attached>(single).unwrap();
        let strength = world.get::<Strength>(single).unwrap();
        let strength_total = strength.resolve(
            attached
                .0
                .iter()
                .filter_map(|e| world.get::<Modifier<Strength>>(*e)),
        );
        assert_eq!(strength_total, 20.0); // 10 + 5 + 10 × 0.5

        let attached = world.get::<Attached>(pool).unwrap();
        let health = world.get::<Health>(pool).unwrap();
        let max_hp = health.resolve_max(
            attached
                .0
                .iter()
                .filter_map(|e| world.get::<Modifier<Max<Health>>>(*e)),
        );
        assert_eq!(max_hp, 65.0); // 30 + 20 + 30 × 0.5
        assert_eq!(health.current(), 8.0, "当前值是状态，不该被修正改动");
    }

    /// 修正的目标是类型，所以给精力的修正不会跑到血量上。
    #[test]
    fn modifier_targets_are_distinct() {
        let mut world = World::new();

        let e = world.spawn(Health::default()).id();
        let mods: Vec<Entity> = [ModStaminaMax::flat(99.0)]
            .into_iter()
            .map(|m| world.spawn(m).id())
            .collect();
        world.entity_mut(e).insert(Attached(mods));

        let attached = world.get::<Attached>(e).unwrap();
        let health = world.get::<Health>(e).unwrap();
        let max_hp = health.resolve_max(
            attached
                .0
                .iter()
                .filter_map(|x| world.get::<Modifier<Max<Health>>>(*x)),
        );
        assert_eq!(max_hp, 0.0, "精力的修正不该出现在血量上");
    }
}
