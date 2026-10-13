//! 派生：某些量由属性算出来。
//!
//! 一条派生 = **一个实体**（与装备一样，是“修正的一个来源”），它身上有两样东西：
//!
//! - [`Derived<From, To>`]：声明“`To` 由 `From` 算出来”，参数在配置资产里；
//! - [`Modifier<To>`]：算出来的结果（由 `#[require]` 自动带上），由 [`Derived::sync`] 写。
//!
//! **读出的地方完全不用知道“派生”这回事**：它看到的还是 `Modifier<To>`，与装备给的修正同构。
//!
//! 一条规则的两半都在 [`Derived`] 身上：`new` 造它、`sync` 算它；插件里每种组合注册一行。

use bevy_asset::{AssetId, Assets, Handle};
use bevy_ecs::prelude::*;
use core::marker::PhantomData;
use voxelith_core::prelude::*;

use crate::config::RuleConfig;

/// 一条派生规则：`To` 那个量由 `From` 那个属性算出来。
///
/// 参数（每点给多少）在 [`RuleConfig`] 资产里——可改、可复用、可热重载。
/// 因为字段里有 [`Handle`]，这个组件要**手写 `FromTemplate`**（`bsn!` 里才能写路径字符串）。
#[derive(Component, Clone, Debug, FromTemplate)]
#[require(Modifier<To>)]
pub struct Derived<From, To>
where
    From: Send + Sync + 'static + Unpin,
    To: Send + Sync + 'static + Unpin,
{
    /// 用哪份配置。
    pub config: Handle<RuleConfig>,
    /// `Unpin` 那两条约束是**给这个字段的**：手写 `FromTemplate` 会让它去实现 `Template`，
    /// 而 `PhantomData<T>` 的 auto trait 跟着 `T`——泛型参数不写 `Unpin` 就证不出来。
    /// （另一条路是 `PhantomData<fn() -> (From, To)>`：函数指针永远 `Unpin`，但那样更绕。）
    _marker: PhantomData<(From, To)>,
}

impl<From, To> Derived<From, To>
where
    From: Send + Sync + 'static + Unpin,
    To: Send + Sync + 'static + Unpin,
{
    /// 用一份配置建一条派生规则（`bsn!` 里直接写 `{ config: "路径.ron" }` 也行）。
    pub fn new(config: Handle<RuleConfig>) -> Self {
        Self {
            config,
            _marker: PhantomData,
        }
    }

    /// 把这条规则算出来，写进它那条结果修正。
    ///
    /// **要求配置一定在**：缺席就是错误（`bsn!` 里漏写，或者装配层没等它加载好）。
    /// “等加载”是装配层的事——进游戏前把用到的资产等齐，别让 system 自己处理“还没到货”。
    ///
    /// ⚠️ 下面那三行看着啰嗦，但**不能省**：把 `Mut<T>` 转成 `&mut T`（例如传给 helper）
    /// 会立刻调 `DerefMut`，Bevy 就此标记“变了”——于是每帧都标记，下游被连锁触发。
    /// 所以：**只在值真的变了的时候才碰它**。
    pub fn sync(
        configs: Res<Assets<RuleConfig>>,
        owners: Query<&From>,
        mut rules: Query<(&Derived<From, To>, &DerivedFor, &mut Modifier<To>)>,
    ) where
        From: Value,
    {
        for (rule, owner, mut modifier) in &mut rules {
            let Ok(from) = owners.get(owner.0) else {
                continue; // 主人没有这个属性：规则挂错了，将来接上日志再吵
            };

            // 取配置：没到货就是错误。
            let config = configs.get(&rule.config).unwrap_or_else(|| {
                if rule.config.id() == AssetId::<RuleConfig>::default() {
                    panic!("派生规则没有写 config（bsn 里漏了？）");
                }
                panic!(
                    "派生规则的配置还没加载好：{:?}（装配层应该在进游戏前等齐）",
                    rule.config
                );
            });

            let value = from.base() * config.per_point;
            if modifier.flat != value {
                modifier.flat = value;
            }
        }
    }
}

/// 这条派生规则挂在哪个主人身上。
#[derive(Component, FromTemplate)]
#[relationship(relationship_target = DerivedModifiers)]
pub struct DerivedFor(pub Entity);

/// 主人身上自动维护的派生规则列表。
#[derive(Component, Default)]
#[relationship_target(relationship = DerivedFor, linked_spawn)]
pub struct DerivedModifiers(Vec<Entity>);

// ---- 别名：配置、`bsn!`、以及插件注册里用的名字 ----

/// 血量上限由体质来。
pub type DerHealthMaxFromConstitution = Derived<Constitution, Max<Health>>;
/// 精力上限由意志来。
pub type DerStaminaMaxFromWillpower = Derived<Willpower, Max<Stamina>>;
/// 精力回复由意志来（`SY-56`：精力上限与回复 ← 意志）。
pub type DerStaminaRegenFromWillpower = Derived<Willpower, StaminaRegen>;
