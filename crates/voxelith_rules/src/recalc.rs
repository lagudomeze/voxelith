//! 重算：把「raw + Σ修正」写进 [`Total`] 缓存。
//!
//! **只有一个泛型系统**、**没有泛型插件**：归属是库自带的唯一一条关系（[`Affects`] /
//! [`Modifiers`]），资源上限也只是个 [`Value`]（`Max<Health>`）。
//!
//! 求和就是把主人名单上的修正都加起来——读的人（UI、伤害、AI）只看 `Total`，不认识归属。
//! 派生也是同一个形状：它给自己挂一条修正（自指），于是"自己那条"和别人的修正**同一条路径**。
//!
//! ## 顺序
//!
//! 派生是"读属性、写修正"，而回复速率与资源上限又是"读修正、写缓存"，所以一帧里要分三段：
//!
//! ```text
//! RecalcAttributes（属性）→ DerivedRules（派生写修正）→ RecalcTotals（回复速率 + 资源上限）
//! ```
//!
//! 三段由 [`crate::RulesPlugin`] 串起来。

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_ecs::query::QueryData;
use voxelith_core::prelude::*;

/// 第一段：六项属性的有效值。
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct RecalcAttributes;

/// 第二段：派生规则写自己那条修正。
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct DerivedRules;

/// 第三段：回复速率与资源上限的有效值。
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct RecalcTotals;

/// 重算一个量要的三样东西。
///
/// 用 `QueryData` 给这个"三元组"起个名字：签名短，字段也有名字。
/// `list` 是 `Option`：**最后一个修正没了，名单组件本身会消失**。
#[derive(QueryData)]
#[query_data(mutable)]
pub struct RecomputeParts<Q: Value> {
    /// 量自己（取 raw）。
    pub value: &'static Q,
    /// 归属名单（库自带的那条关系；没有就是空的）。
    pub list: Option<&'static Modifiers>,
    /// 有效值缓存（写它）。
    pub total: &'static mut Total<Q>,
}

/// 重算一个量的有效值。
///
/// 只在**值真的变了**的时候写——否则每帧都标记变更，下游的变更检测被连锁触发。
pub fn recompute<Q: Value>(
    mut owners: Query<RecomputeParts<Q>>,
    modifiers: Query<&Modifier<Q>>,
) {
    for mut item in &mut owners {
        let raw = item.value.raw();
        let mut resolved = raw;

        if let Some(list) = item.list {
            for entity in list.iter() {
                if let Ok(modifier) = modifiers.get(entity) {
                    resolved += modifier.contribution(raw);
                }
            }
        }

        if item.total.value != resolved {
            item.total.value = resolved;
        }
    }
}

/// 把重算系统挂上去，并把三段串起来。由 [`crate::RulesPlugin`] 调用。
pub(crate) fn install(app: &mut App) {
    app.configure_sets(
        Update,
        (RecalcAttributes, DerivedRules, RecalcTotals).chain(),
    );

    app.add_systems(
        Update,
        (
            recompute::<Strength>,
            recompute::<Dexterity>,
            recompute::<Constitution>,
            recompute::<Perception>,
            recompute::<Magic>,
            recompute::<Willpower>,
        )
            .in_set(RecalcAttributes),
    );

    app.add_systems(
        Update,
        (
            // 回复速率也是 Value
            recompute::<HealthRegen>,
            recompute::<StaminaRegen>,
            recompute::<ManaRegen>,
            // 资源上限 = Value<Max<资源>>，走同一个泛型系统
            recompute::<Max<Health>>,
            recompute::<Max<Stamina>>,
            recompute::<Max<Mana>>,
        )
            .in_set(RecalcTotals),
    );
}
