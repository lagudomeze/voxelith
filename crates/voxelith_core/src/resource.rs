//! 资源池：当前值 + 上限，以及它们的**回复速率**。
//!
//! 每条资源有**三个**量：当前值（状态）、基础上限、基础回复速率（每秒多少）。
//! **只有上限与回复速率可修正**：有效值在 [`Total`] 里（上限是 `Total<Max<资源>>`，
//! 回复速率是 `Total<回复速率>`）；当前值是状态，由事件（[`crate::change::Adjust`]）改。
//!
//! 哪些资源人人都有、哪些按角色配置、以及各自的模型（小幅回复 / 不回靠赚 / 越高越糟 /
//! 动态衰减 / 挨打才涨 / 硬上限），见 `docs/design/01-attributes-and-resources.md`。

use crate::value::{Max, Modifier, Resourceable, Total, Value};
use bevy_ecs::prelude::*;

/// 血量。
#[derive(Component, Clone, Copy, Debug, Default)]
#[require(Total<Max<Health>>)]
pub struct Health {
    /// 当前值。状态，由事件改。
    pub current: f32,
    /// 基础上限。修正加在它上面，有效上限在 `Total<Max<Health>>`。
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
#[require(Total<Max<Stamina>>)]
pub struct Stamina {
    /// 当前值。状态，由事件改。
    pub current: f32,
    /// 基础上限。修正加在它上面，有效上限在 `Total<Max<Stamina>>`。
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
#[require(Total<Max<Mana>>)]
pub struct Mana {
    /// 当前值。状态，由事件改。
    pub current: f32,
    /// 基础上限。修正加在它上面，有效上限在 `Total<Max<Mana>>`。
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
#[require(Total<HealthRegen>)]
pub struct HealthRegen(pub f32);

impl Value for HealthRegen {
    fn base(&self) -> f32 {
        self.0
    }
}

/// 精力每秒回复多少。
#[derive(Component, Clone, Copy, Debug, Default)]
#[require(Total<StaminaRegen>)]
pub struct StaminaRegen(pub f32);

impl Value for StaminaRegen {
    fn base(&self) -> f32 {
        self.0
    }
}

/// 法力每秒回复多少。
#[derive(Component, Clone, Copy, Debug, Default)]
#[require(Total<ManaRegen>)]
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

    /// 每个量一出生就带一份有效值缓存（`#[require]` 干的），所以 `bsn!` 里不用写。
    #[test]
    fn every_quantity_gets_a_cache_slot() {
        let mut world = World::new();

        let e = world
            .spawn((
                Health::default(),
                Stamina::default(),
                Mana::default(),
                HealthRegen::default(),
                StaminaRegen::default(),
                ManaRegen::default(),
            ))
            .id();

        assert!(world.get::<Total<Max<Health>>>(e).is_some());
        assert!(world.get::<Total<Max<Stamina>>>(e).is_some());
        assert!(world.get::<Total<Max<Mana>>>(e).is_some());
        assert!(world.get::<Total<HealthRegen>>(e).is_some());
        assert!(world.get::<Total<StaminaRegen>>(e).is_some());
        assert!(world.get::<Total<ManaRegen>>(e).is_some());
    }

    /// 每条资源的修正各指各的：血量上限与法力上限是两个不同的标记
    /// （类型不同，互相塞是编译期错误）。
    #[test]
    fn modifier_targets_are_distinct() {
        let health = ModHealthMax::flat(5.0);
        let mana = ModManaMax::flat(5.0);

        // 平加 5 → contribution 就是 5（基础值不参与平加）
        assert_eq!(health.contribution(10.0), 5.0);
        assert_eq!(mana.contribution(10.0), 5.0);
        assert_eq!(health.flat, mana.flat);
    }

    /// 回复速率也是一个可修正的量，走同一条公式。
    #[test]
    fn regen_is_a_value_too() {
        let regen = StaminaRegen(1.0);
        let modifier = ModStaminaRegen::flat(0.5);

        assert_eq!(regen.base(), 1.0);
        assert_eq!(regen.base() + modifier.contribution(regen.base()), 1.5);
    }

    /// 当前值是状态：只有“改值的唯一通路”能写它。
    #[test]
    fn current_value_is_state() {
        let mut health = Health {
            current: 30.0,
            base_max: 60.0,
        };
        assert_eq!(health.current(), 30.0);
        *health.current_mut() += 7.0;
        assert_eq!(health.current(), 37.0);
        assert_eq!(health.base_max(), 60.0, "上限不受当前值影响");
    }
}
