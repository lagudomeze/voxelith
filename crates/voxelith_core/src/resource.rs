//! 资源池：**当前值 + 上限**（拆成两个组件）+ 回复速率。
//!
//! 每条资源是**两个组件**，靠 `#[require]` 绑在一起：
//!
//! | 组件 | 是什么 | 可修正？ |
//! | --- | --- | --- |
//! | `Health(pub f32)` | **当前值**——状态，由事件（[`crate::change::Adjust`]）改 | ✗ 不挂 `Total`、不注册重算（约定） |
//! | [`HealthMax`] = `Max<Health>` | **基础上限**——本身是个 [`Value`] | ✓ 有效上限在 `Total<Max<Health>>` |
//! | `HealthRegen(pub f32)` | **基础回复速率**（每秒多少） | ✓ 有效速率在 `Total<HealthRegen>` |
//!
//! 拆开的好处：上限直接复用 [`Value`] 的修正能力与缓存，不需要第二个 trait，也不需要第二条重算路径。
//!
//! 哪些资源人人都有、哪些按角色配置、以及各自的模型（小幅回复 / 不回靠赚 / 越高越糟 /
//! 动态衰减 / 挨打才涨 / 硬上限），见 `docs/design/01-attributes-and-resources.md`。

use crate::value::{Max, Modifier, Total, Value};
use bevy_ecs::prelude::*;

/// 血量：当前值（状态）。
#[derive(Component, Clone, Copy, Debug, Default)]
#[require(Max<Health>)]
pub struct Health(pub f32);

impl Value for Health {
    fn raw(&self) -> f32 {
        self.0
    }

    fn raw_mut(&mut self) -> &mut f32 {
        &mut self.0
    }
}

/// 精力：当前值（状态）。
#[derive(Component, Clone, Copy, Debug, Default)]
#[require(Max<Stamina>)]
pub struct Stamina(pub f32);

impl Value for Stamina {
    fn raw(&self) -> f32 {
        self.0
    }

    fn raw_mut(&mut self) -> &mut f32 {
        &mut self.0
    }
}

/// 法力：当前值（状态）。**不是人人都有**（`SY-01`：按角色配置）。
#[derive(Component, Clone, Copy, Debug, Default)]
#[require(Max<Mana>)]
pub struct Mana(pub f32);

impl Value for Mana {
    fn raw(&self) -> f32 {
        self.0
    }

    fn raw_mut(&mut self) -> &mut f32 {
        &mut self.0
    }
}

// ---- 上限：每个资源一个别名，`bsn!` 里就用它们 ----
//
// `Max<Q>` 本体在 `crate::value`；这里只起名字，因为 `bsn!` 里写不出 `Max::<Health>` 那种路径。

/// 血量上限的基础值（有效上限在 `Total<Max<Health>>`）。
pub type HealthMax = Max<Health>;
/// 精力上限的基础值。
pub type StaminaMax = Max<Stamina>;
/// 法力上限的基础值。
pub type ManaMax = Max<Mana>;

// ---- 回复速率：每条资源一个量（`SY-56`：精力上限**与回复** ← 意志）----
//
// 时间单位是秒，所以它们是“每秒回复多少”。与上限一样可被修正与派生。

/// 血量每秒回复多少。
#[derive(Component, Clone, Copy, Debug, Default)]
#[require(Total<HealthRegen>)]
pub struct HealthRegen(pub f32);

impl Value for HealthRegen {
    fn raw(&self) -> f32 {
        self.0
    }

    fn raw_mut(&mut self) -> &mut f32 {
        &mut self.0
    }
}

/// 精力每秒回复多少。
#[derive(Component, Clone, Copy, Debug, Default)]
#[require(Total<StaminaRegen>)]
pub struct StaminaRegen(pub f32);

impl Value for StaminaRegen {
    fn raw(&self) -> f32 {
        self.0
    }

    fn raw_mut(&mut self) -> &mut f32 {
        &mut self.0
    }
}

/// 法力每秒回复多少。
#[derive(Component, Clone, Copy, Debug, Default)]
#[require(Total<ManaRegen>)]
pub struct ManaRegen(pub f32);

impl Value for ManaRegen {
    fn raw(&self) -> f32 {
        self.0
    }

    fn raw_mut(&mut self) -> &mut f32 {
        &mut self.0
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

    /// 一条资源 = 两个组件：有当前值就有上限，上限自己带缓存（`#[require]` 传递）。
    #[test]
    fn resource_is_two_components() {
        let mut world = World::new();
        let e = world.spawn(Health(30.0)).id();

        assert_eq!(world.get::<Health>(e).unwrap().raw(), 30.0);
        assert!(world.get::<Max<Health>>(e).is_some(), "上限该被自动带上");
        assert!(
            world.get::<Total<Max<Health>>>(e).is_some(),
            "上限的缓存该被传递带上"
        );
    }

    /// 上限是个量：能改、能被修正，而当前值只是状态。
    #[test]
    fn max_is_modifiable_and_current_is_state() {
        let mut world = World::new();
        let e = world.spawn(Health(30.0)).id();

        // 上限：写基础值 + 算一条修正的贡献
        world.get_mut::<Max<Health>>(e).unwrap().0 = 60.0;
        let modifier = ModHealthMax::new(20.0, 0.5);
        let base = world.get::<Max<Health>>(e).unwrap().raw();
        assert_eq!(base, 60.0);
        assert_eq!(base + modifier.contribution(base), 110.0); // 60 + 20 + 30

        // 当前值：状态，靠 raw_mut 改
        *world.get_mut::<Health>(e).unwrap().raw_mut() += 7.0;
        assert_eq!(world.get::<Health>(e).unwrap().raw(), 37.0);
    }

    /// 每条资源的修正各指各的（血量上限与法力上限是两个不同的标记）。
    #[test]
    fn modifier_targets_are_distinct() {
        // 这两个测试只看公式，归属随便填
        let health = ModHealthMax::flat(5.0);
        let mana = ModManaMax::flat(5.0);

        assert_eq!(health.contribution(10.0), 5.0);
        assert_eq!(mana.contribution(10.0), 5.0);
        assert_eq!(health.flat, mana.flat);
    }

    /// 回复速率也是一个可修正的量，走同一条公式。
    #[test]
    fn regen_is_a_value_too() {
        let regen = StaminaRegen(1.0);
        let modifier = ModStaminaRegen::flat(0.5);

        assert_eq!(regen.raw(), 1.0);
        assert_eq!(regen.raw() + modifier.contribution(regen.raw()), 1.5);
    }

    /// 三个资源各带自己的上限与缓存。
    #[test]
    fn every_pool_gets_its_own_components() {
        let mut world = World::new();
        let e = world.spawn((Health(30.0), Stamina(20.0), Mana(10.0))).id();

        assert!(world.get::<Total<Max<Health>>>(e).is_some());
        assert!(world.get::<Total<Max<Stamina>>>(e).is_some());
        assert!(world.get::<Total<Max<Mana>>>(e).is_some());
        assert_eq!(world.get::<Mana>(e).unwrap().raw(), 10.0);
    }
}
