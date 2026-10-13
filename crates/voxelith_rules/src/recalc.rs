//! 重算：把「基础值 + Σ修正」写进 [`Total`] 缓存。
//!
//! 这一层是**唯一认识挂法**的地方：每个项目自己决定修正怎么挂在主人身上（装备槽、增益列表……），
//! 然后由这里的两个泛型系统按那个挂法去求和。读的人（UI、伤害、AI）只看 `Total`，不认识挂法。
//!
//! 求和的两条来源：
//!
//! 1. **自己身上**那条同类型的修正（`Modifier<Q>`）——派生规则就是往这里写；
//! 2. 挂着的那串修正实体（由泛型参数 `L` 决定的 relationship）。
//!
//! ## 顺序
//!
//! 派生是"读属性、写修正"，而资源上限又是"读修正、写缓存"，所以一帧里要分三段：
//!
//! ```text
//! RecalcAttributes（属性）→ DerivedRules（派生写修正）→ RecalcTotals（回复速率 + 资源上限）
//! ```
//!
//! 三段由 [`crate::RulesPlugin`] 串起来。

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
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

/// 单值型的重算。
///
/// 只在**值真的变了**的时候写——否则每帧都标记变更，下游的变更检测被连锁触发。
#[allow(clippy::type_complexity, reason = "查询就是要写这么长，拆 type 反而看不清")]
pub fn recompute<Q, L>(
    mut owners: Query<(&Q, Option<&L>, Option<&Modifier<Q>>, &mut Total<Q>)>,
    modifiers: Query<&Modifier<Q>>,
) where
    Q: Value,
    L: RelationshipTarget,
{
    for (value, link, own, mut total) in &mut owners {
        let base = value.base();
        let mut resolved = base;

        // 自己身上那条（派生规则写在这里）
        if let Some(own) = own {
            resolved += own.contribution(base);
        }

        // 挂着的那些
        if let Some(link) = link {
            for entity in link.collection().iter() {
                if let Ok(modifier) = modifiers.get(entity) {
                    resolved += modifier.contribution(base);
                }
            }
        }

        if total.value != resolved {
            total.value = resolved;
        }
    }
}

/// 资源上限的重算。与 [`recompute`] 同形，只是标记换成 [`Max<R>`]。
#[allow(clippy::type_complexity, reason = "同上")]
pub fn recompute_max<R, L>(
    mut owners: Query<(&R, Option<&L>, Option<&Modifier<Max<R>>>, &mut Total<Max<R>>)>,
    modifiers: Query<&Modifier<Max<R>>>,
) where
    R: Resourceable,
    L: RelationshipTarget,
{
    for (resource, link, own, mut total) in &mut owners {
        let base = resource.base_max();
        let mut resolved = base;

        if let Some(own) = own {
            resolved += own.contribution(base);
        }

        if let Some(link) = link {
            for entity in link.collection().iter() {
                if let Ok(modifier) = modifiers.get(entity) {
                    resolved += modifier.contribution(base);
                }
            }
        }

        if total.value != resolved {
            total.value = resolved;
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
            recompute::<HealthRegen, L>,
            recompute::<StaminaRegen, L>,
            recompute::<ManaRegen, L>,
            recompute_max::<Health, L>,
            recompute_max::<Stamina, L>,
            recompute_max::<Mana, L>,
        )
            .in_set(RecalcTotals),
    );
}
