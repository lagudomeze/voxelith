//! 重算：把「raw + Σ修正」写进 [`Total`] 缓存。
//!
//! 这一层是**唯一认识挂法**的地方：每个项目自己决定修正怎么挂在主人身上（装备槽、增益列表……），
//! 然后由这里的泛型系统按那个挂法去求和。读的人（UI、伤害、AI）只看 `Total`，不认识挂法。
//!
//! 求和的两条来源：
//!
//! 1. **自己身上**那条同类型的修正（`Modifier<Q>`）——派生规则就是往这里写；
//! 2. 挂着的那串修正实体（由泛型参数 `L` 决定的 relationship）。
//!
//! **只有一个泛型系统**：资源的上限也是个 [`Value`]（`Max<Health>`），所以"重算上限"就是
//! `recompute::<Max<Health>, L>`，不需要第二条路径。
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
use bevy_ecs::relationship::RelationshipSourceCollection;
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

/// 重算一个量要的四样东西。
///
/// 用 `QueryData` 给这个"四元组"起个名字：签名短了，每个字段也有名字——
/// 比 `Query<(&Q, Option<&L>, Option<&Modifier<Q>>, &mut Total<Q>)>` 那种靠位置记的写法好读。
#[derive(QueryData)]
#[query_data(mutable)]
pub struct RecomputeParts<Q: Value, L: RelationshipTarget> {
    /// 量自己（取 raw）。
    pub value: &'static Q,
    /// 挂法：这条链接上的修正实体（没有链接就是空的）。
    pub link: Option<&'static L>,
    /// 自己身上那条修正（派生规则写在这里）。
    pub own: Option<&'static Modifier<Q>>,
    /// 有效值缓存（写它）。
    pub total: &'static mut Total<Q>,
}

/// 重算一个量的有效值。
///
/// 只在**值真的变了**的时候写——否则每帧都标记变更，下游的变更检测被连锁触发。
pub fn recompute<Q, L>(
    mut owners: Query<RecomputeParts<Q, L>>,
    modifiers: Query<&Modifier<Q>>,
) where
    Q: Value,
    L: RelationshipTarget,
{
    for mut item in &mut owners {
        let raw = item.value.raw();
        let mut resolved = raw;

        // 自己身上那条（派生规则写在这里）
        if let Some(own) = item.own {
            resolved += own.contribution(raw);
        }

        // 挂着的那些
        if let Some(link) = item.link {
            for entity in link.collection().iter() {
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
pub(crate) fn install<L: RelationshipTarget>(app: &mut App) {
    app.configure_sets(
        Update,
        (RecalcAttributes, DerivedRules, RecalcTotals).chain(),
    );

    app.add_systems(
        Update,
        (
            recompute::<Strength, L>,
            recompute::<Dexterity, L>,
            recompute::<Constitution, L>,
            recompute::<Perception, L>,
            recompute::<Magic, L>,
            recompute::<Willpower, L>,
        )
            .in_set(RecalcAttributes),
    );

    app.add_systems(
        Update,
        (
            // 回复速率也是 Value
            recompute::<HealthRegen, L>,
            recompute::<StaminaRegen, L>,
            recompute::<ManaRegen, L>,
            // 资源上限 = Value<Max<资源>>，走同一个泛型系统
            recompute::<Max<Health>, L>,
            recompute::<Max<Stamina>, L>,
            recompute::<Max<Mana>, L>,
        )
            .in_set(RecalcTotals),
    );
}
