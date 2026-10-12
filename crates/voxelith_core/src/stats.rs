//! 可修正的量：属性与资源。
//!
//! 三件事：**标记**（量的名字）、**两种载体**（单值型 / 资源池型）、**修正与读出**。
//! 语义见 `docs/design/01-attributes-and-resources.md`，Bevy 写法见 `docs/design/bevy/01-attributes-and-resources.md`。
//!
//! **命名注意**：使用方会同时 glob 导入 `bevy_ecs::prelude::*` 与本 crate 的 prelude，
//! 所以这里的名字**不能与 `bevy_ecs::prelude` 撞**。踩过的两个：
//!
//! - `Add` —— 那是 `bevy_ecs::lifecycle::Add`，两边都导入时直接 `E0659` 歧义报错；故改名 [`Flat`]。
//! - `Resource` —— 那是 `bevy_ecs::resource::Resource` 这个 trait，会被遮蔽；故改名 [`Pool`]。

use bevy_ecs::prelude::*;
use core::marker::PhantomData;

// ==================== 标记 ====================
//
// 每个“可修正的量”一个零大小的标记。**必须 `Clone + Copy + Default`**：
// 泛型组件上的 `#[derive(Clone)]` 会给泛型参数加上同样的约束（imperfect derive）。

/// 力量：物理行为的强度。
#[derive(Clone, Copy, Debug, Default)]
pub struct StrengthStat;
/// 敏捷：命中与闪避。
#[derive(Clone, Copy, Debug, Default)]
pub struct DexterityStat;
/// 体质：血量上限与回复，物理豁免。
#[derive(Clone, Copy, Debug, Default)]
pub struct ConstitutionStat;
/// 感知：精神强度、暴击与侦测。
#[derive(Clone, Copy, Debug, Default)]
pub struct PerceptionStat;
/// 魔力：法术强度与法力上限。
#[derive(Clone, Copy, Debug, Default)]
pub struct MagicStat;
/// 意志：法术与精神豁免，精力上限与回复。
#[derive(Clone, Copy, Debug, Default)]
pub struct WillpowerStat;

/// 血量上限。
#[derive(Clone, Copy, Debug, Default)]
pub struct HealthMax;
/// 精力上限。
#[derive(Clone, Copy, Debug, Default)]
pub struct StaminaMax;

// ==================== 两种载体 ====================

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
///
/// **只有当前值是状态**（由事件改，不走修正）；上限走与单值型同一套修正。
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

// ==================== 别名 ====================
//
// 别名把标记绑到一个好写的名字上：数据文件与 `bsn!` 里用的就是这些。

/// 力量。
pub type Strength = Value<StrengthStat>;
/// 敏捷。
pub type Dexterity = Value<DexterityStat>;
/// 体质。
pub type Constitution = Value<ConstitutionStat>;
/// 感知。
pub type Perception = Value<PerceptionStat>;
/// 魔力。
pub type Magic = Value<MagicStat>;
/// 意志。
pub type Willpower = Value<WillpowerStat>;

/// 血量。
pub type Health = Pool<HealthMax>;
/// 精力。
pub type Stamina = Pool<StaminaMax>;

// ==================== 修正 ====================

/// 平加：加一个绝对值（`+5 力量`）。
#[derive(Component, Clone, Debug, Default)]
pub struct Flat<M> {
    pub amount: f32,
    pub _marker: PhantomData<M>,
}

impl<M> Flat<M> {
    pub fn new(amount: f32) -> Self {
        Self {
            amount,
            _marker: PhantomData,
        }
    }
}

/// 比例：加“**基础值**”的一个比例（`0.5` = `+50% 基础值`）。
///
/// 注意是按基础值，不是按“已经加过别的修正之后的值”——这样叠加才可交换。
#[derive(Component, Clone, Debug, Default)]
pub struct Ratio<M> {
    pub value: f32,
    pub _marker: PhantomData<M>,
}

impl<M> Ratio<M> {
    pub fn new(value: f32) -> Self {
        Self {
            value,
            _marker: PhantomData,
        }
    }
}

/// 力量平加。
pub type FlatStrength = Flat<StrengthStat>;
/// 敏捷平加。
pub type FlatDexterity = Flat<DexterityStat>;
/// 体质平加。
pub type FlatConstitution = Flat<ConstitutionStat>;
/// 感知平加。
pub type FlatPerception = Flat<PerceptionStat>;
/// 魔力平加。
pub type FlatMagic = Flat<MagicStat>;
/// 意志平加。
pub type FlatWillpower = Flat<WillpowerStat>;
/// 血量上限平加。
pub type FlatHealthMax = Flat<HealthMax>;
/// 精力上限平加。
pub type FlatStaminaMax = Flat<StaminaMax>;

/// 力量比例。
pub type RatioStrength = Ratio<StrengthStat>;
/// 血量上限比例。
pub type RatioHealthMax = Ratio<HealthMax>;

// ==================== 关系：修正指回主人 ====================

/// 修正指回被修正的角色。
///
/// `Entity` 没有 `Default`，所以必须派生 `FromTemplate`，`bsn!` 才能写它。
#[derive(Component, FromTemplate)]
#[relationship(relationship_target = Modifiers)]
pub struct ModifiesOf(pub Entity);

/// 被修正者身上自动维护的修正列表。
///
/// `linked_spawn`：主人销毁时，修正实体自动一起消失——“卸下装备 = 销毁那个修正实体”不用手写清理。
#[derive(Component, Default)]
#[relationship_target(relationship = ModifiesOf, linked_spawn)]
pub struct Modifiers(Vec<Entity>);

// ==================== 读出 ====================

/// 指向某一个量的修正总和。
#[derive(Default, Clone, Copy, Debug, PartialEq)]
pub struct Totals {
    /// 平加之和。
    pub add: f32,
    /// 比例之和。
    pub ratio: f32,
}

impl Totals {
    /// 公式：`基础值 + Σ平加 + 基础值 × Σ比例`。
    ///
    /// 纯函数——不带世界、不遍历，顺序无关，可并行算完再归约。
    pub fn resolve(&self, base: f32) -> f32 {
        base + self.add + base * self.ratio
    }
}

/// 收集指向某个标记的所有修正（只读、不计算）。
pub fn collect<M: Send + Sync + 'static>(world: &World, owner: Entity) -> Totals {
    let mut totals = Totals::default();
    if let Some(mods) = world.get::<Modifiers>(owner) {
        for m in mods.iter() {
            if let Some(a) = world.get::<Flat<M>>(m) {
                totals.add += a.amount;
            }
            if let Some(r) = world.get::<Ratio<M>>(m) {
                totals.ratio += r.value;
            }
        }
    }
    totals
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
    use super::*;

    #[test]
    fn formula_is_a_pure_function() {
        let totals = Totals {
            add: 5.0,
            ratio: 0.5,
        };
        assert_eq!(totals.resolve(10.0), 20.0);
    }

    /// 平加与比例都基于基础值，所以登记顺序不影响结果。
    #[test]
    fn modifiers_commute() {
        let mut world = World::new();

        let a = world.spawn(Strength::new(10.0)).id();
        world.spawn((ModifiesOf(a), FlatStrength::new(5.0)));
        world.spawn((ModifiesOf(a), RatioStrength::new(0.5)));

        let b = world.spawn(Strength::new(10.0)).id();
        world.spawn((ModifiesOf(b), RatioStrength::new(0.5))); // 顺序反过来
        world.spawn((ModifiesOf(b), FlatStrength::new(5.0)));

        assert_eq!(resolve_value::<StrengthStat>(&world, a), Some(20.0));
        assert_eq!(resolve_value::<StrengthStat>(&world, b), Some(20.0));
    }

    /// 主人销毁时，修正实体随 `linked_spawn` 一起消失。
    #[test]
    fn modifiers_die_with_their_owner() {
        let mut world = World::new();

        let owner = world.spawn(Strength::new(10.0)).id();
        let m1 = world.spawn((ModifiesOf(owner), FlatStrength::new(5.0))).id();
        let m2 = world.spawn((ModifiesOf(owner), FlatStrength::new(3.0))).id();

        assert_eq!(resolve_value::<StrengthStat>(&world, owner), Some(18.0));

        world.despawn(owner);
        assert!(world.get_entity(owner).is_err());
        assert!(world.get_entity(m1).is_err());
        assert!(world.get_entity(m2).is_err());
    }

    /// 资源上限与单值型共用同一套修正；当前值不被修正改动。
    #[test]
    fn resource_max_uses_the_same_modifiers() {
        let mut world = World::new();

        let e = world.spawn(Health::new(8.0, 30.0)).id();
        world.spawn((ModifiesOf(e), FlatHealthMax::new(20.0)));
        world.spawn((ModifiesOf(e), RatioHealthMax::new(0.5)));

        // 30 + 20 + 30 × 0.5 = 65
        assert_eq!(resolve_max::<HealthMax>(&world, e), Some(65.0));
        assert_eq!(world.get::<Health>(e).unwrap().current, 8.0);
    }
}
