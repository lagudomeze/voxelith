//! 资源池：当前值 + 上限，以及它们的**回复速率**。
//!
//! 每条资源有**三个**量：当前值（状态）、基础上限、基础回复速率（每秒多少）。
//! **只有上限与回复速率可修正**：当前值是状态，由事件（[`crate::change::Adjust`]）改。
//!
//! 哪些资源人人都有、哪些按角色配置、以及各自的模型（小幅回复 / 不回靠赚 / 越高越糟 /
//! 动态衰减 / 挨打才涨 / 硬上限），见 `docs/design/01-attributes-and-resources.md`。

use crate::value::{Max, Modifier, Resourceable, Value};
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

    fn current_mut(&mut self) -> &mut f32 {
        &mut self.current
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

    fn current_mut(&mut self) -> &mut f32 {
        &mut self.current
    }
}

/// 法力。**不是人人都有**（`SY-01`：按角色配置）。
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Mana {
    /// 当前值。状态，由事件改。
    pub current: f32,
    /// 基础上限。修正加在它上面。
    pub base_max: f32,
}

impl Resourceable for Mana {
    fn current(&self) -> f32 {
        self.current
    }

    fn base_max(&self) -> f32 {
        self.base_max
    }

    fn current_mut(&mut self) -> &mut f32 {
        &mut self.current
    }
}

// ---- 回复速率：每条资源一个量（`SY-56`：精力上限**与回复** ← 意志）----
//
// 时间单位是秒，所以它们是“每秒回复多少”。与上限一样可被修正与派生。

/// 血量每秒回复多少。
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct HealthRegen(pub f32);

impl Value for HealthRegen {
    fn base(&self) -> f32 {
        self.0
    }
}

/// 精力每秒回复多少。
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct StaminaRegen(pub f32);

impl Value for StaminaRegen {
    fn base(&self) -> f32 {
        self.0
    }
}

/// 法力每秒回复多少。
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct ManaRegen(pub f32);

impl Value for ManaRegen {
    fn base(&self) -> f32 {
        self.0
    }
}

// ---- 修正别名 ----

/// 血量上限的修正。
pub type ModHealthMax = Modifier<Max<Health>>;
/// 精力上限的修正。
pub type ModStaminaMax = Modifier<Max<Stamina>>;
/// 法力上限的修正。
pub type ModManaMax = Modifier<Max<Mana>>;

/// 血量回复的修正。
pub type ModHealthRegen = Modifier<HealthRegen>;
/// 精力回复的修正。
pub type ModStaminaRegen = Modifier<StaminaRegen>;
/// 法力回复的修正。
pub type ModManaRegen = Modifier<ManaRegen>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attribute::{ModStrength, Strength};
    use crate::value::Value;

    /// 测试自己定怎么挂（同 `value` 模块里那份说明）。
    #[derive(Component, Default)]
    struct Attached(Vec<Entity>);

    fn attach(world: &mut World, owner: Entity, modifiers: impl IntoIterator<Item = impl Bundle>) {
        let ids: Vec<Entity> = modifiers
            .into_iter()
            .map(|m| world.spawn(m).id())
            .collect();
        world.entity_mut(owner).insert(Attached(ids));
    }

    /// 单值型与资源上限共用同一条公式；池的当前值不被修正改动。
    #[test]
    fn pool_limit_and_single_value_share_one_rule() {
        let mut world = World::new();

        let single = world.spawn(Strength(10.0)).id();
        attach(
            &mut world,
            single,
            [ModStrength::flat(5.0), ModStrength::ratio(0.5)],
        );

        let pool = world
            .spawn(Health {
                current: 8.0,
                base_max: 30.0,
            })
            .id();
        attach(
            &mut world,
            pool,
            [ModHealthMax::flat(20.0), ModHealthMax::ratio(0.5)],
        );

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

    /// 三条资源各自独立：给血量的修正不会跑到法力上。
    #[test]
    fn modifier_targets_are_distinct() {
        let mut world = World::new();

        let e = world.spawn((Health::default(), Mana::default())).id();
        attach(&mut world, e, [ModManaMax::flat(99.0)]);

        let attached = world.get::<Attached>(e).unwrap();
        let health = world.get::<Health>(e).unwrap();
        let max_hp = health.resolve_max(
            attached
                .0
                .iter()
                .filter_map(|x| world.get::<Modifier<Max<Health>>>(*x)),
        );
        assert_eq!(max_hp, 0.0, "法力的修正不该出现在血量上");

        let mana = world.get::<Mana>(e).unwrap();
        let max_mana = mana.resolve_max(
            attached
                .0
                .iter()
                .filter_map(|x| world.get::<Modifier<Max<Mana>>>(*x)),
        );
        assert_eq!(max_mana, 99.0);
    }

    /// 回复速率也是一个可修正的量：意志派生出来的回复加成走同一条公式。
    #[test]
    fn regen_is_a_value_too() {
        let mut world = World::new();

        let e = world.spawn(StaminaRegen(1.0)).id();
        attach(&mut world, e, [ModStaminaRegen::flat(0.5)]);

        let attached = world.get::<Attached>(e).unwrap();
        let regen = world.get::<StaminaRegen>(e).unwrap();
        let total = regen.resolve(
            attached
                .0
                .iter()
                .filter_map(|x| world.get::<Modifier<StaminaRegen>>(*x)),
        );
        assert_eq!(total, 1.5);

        // 没有修正就是基础值
        let bare = world.spawn(StaminaRegen(2.0)).id();
        let regen = world.get::<StaminaRegen>(bare).unwrap();
        assert_eq!(regen.resolve(std::iter::empty()), 2.0);
    }

    /// 没有挂任何东西的实体，读出来就是基础值。
    #[test]
    fn no_attachment_reads_base() {
        let mut world = World::new();
        let e = world
            .spawn(Mana {
                current: 3.0,
                base_max: 9.0,
            })
            .id();
        let mana = world.get::<Mana>(e).unwrap();
        assert_eq!(mana.resolve_max(std::iter::empty()), 9.0);
        assert_eq!(mana.current(), 3.0);
    }
}
