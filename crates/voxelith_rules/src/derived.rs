//! 派生：某些量由属性算出来。
//!
//! 一条派生就是**主人身上的一组组件**：声明 [`Formula<From, To>`]（`To` 由 `From` 算出来，
//! 参数在配置资产里）+ 结果那条修正 `Modifier<To>`（由 `#[require]` 自动带上）。
//!
//! ```text
//! 英雄 ── 体质 + Total<体质>              （属性重算写在 Total 里）
//!      ── Formula<体质, Max<血量>>         （声明：用哪份配置）
//!      ── Modifier<Max<血量>>              （结果写在这里）
//!      ── Total<Max<血量>>                 （上限重算把修正算进去，读的人看这个）
//! ```
//!
//! 所以**不需要任何关系**：派生读自己身上的 `Total<From>`，写自己身上那条修正，
//! 再由第三段重算把它算进 `Total<To>`。读的人看到的还是普通修正，与装备给的完全同构。
//!
//! 名字：**概念叫“派生”**（这个模块与它所在的阶段都叫 `derived`），**落地的组件叫 [`Formula`]**——
//! 它表达的就是“用哪条公式、参数在哪份配置”。

use bevy_asset::{AssetId, Assets, Handle};
use bevy_ecs::prelude::*;
use bevy_ecs::query::QueryData;
use core::marker::PhantomData;
use voxelith_core::prelude::*;

use crate::config::RuleConfig;

/// 一条派生规则：`To` 那个量由 `From` 那个属性算出来。
///
/// 参数（每点给多少）在 [`RuleConfig`] 资产里——可改、可复用、可热重载。
/// 因为字段里有 [`Handle`]，这个组件要**手写 `FromTemplate`**（`bsn!` 里才能写路径字符串）。
///
/// **一个量只能有一条自派生规则**（`Modifier<To>` 一份，两个规则会抢同一个格子）。
/// 按 `SY-56`（每个资源各自声明主导属性）目前够用；真需要两条时，那第二条得是一份
/// 挂在别的实体上的修正。
#[derive(Component, Clone, Debug, FromTemplate)]
#[require(Modifier<To>)]
pub struct Formula<From, To>
where
    From: Send + Sync + 'static + Unpin,
    To: Send + Sync + 'static + Unpin,
{
    /// 用哪份配置。
    pub config: Handle<RuleConfig>,
    /// `Unpin` 那两条约束是**给这个字段的**：手写 `FromTemplate` 会让它去实现 `Template`，
    /// 而 `PhantomData<T>` 的 auto trait 跟着 `T`——泛型参数不写 `Unpin` 就证不出来。
    _marker: PhantomData<(From, To)>,
}

/// [`Formula::apply`] 要的三样东西（给这个三元组起个名字）。
#[derive(QueryData)]
#[query_data(mutable)]
pub struct ApplyParts<From, To>
where
    From: Send + Sync + 'static + Unpin,
    To: Send + Sync + 'static + Unpin,
{
    /// 源属性的有效值（派生读它，不是基础值）。
    pub from: &'static Total<From>,
    /// 这条规则（声明 + 用哪份配置）。
    pub rule: &'static Formula<From, To>,
    /// 结果那条修正（写它）。
    pub modifier: &'static mut Modifier<To>,
}

impl<From, To> Formula<From, To>
where
    From: Send + Sync + 'static + Unpin,
    To: Send + Sync + 'static + Unpin,
{
    /// 在主人身上建一条派生规则（`bsn!` 里直接写 `{ config: "路径.ron" }` 也行）。
    pub fn new(config: Handle<RuleConfig>) -> Self {
        Self {
            config,
            _marker: PhantomData,
        }
    }

    /// 应用这条公式：把结果算出来，写进自己身上那条修正。
    ///
    /// **要求配置一定在**：缺席就是错误（`bsn!` 里漏写，或者装配层没等它加载好）。
    /// “等加载”是装配层的事——进游戏前把用到的资产等齐，别让 system 自己处理“还没到货”。
    ///
    /// 读的是 `Total<From>`（**有效值**，含装备给的属性加成），不是基础值——所以
    /// “装备 +5 体质”会真的抬血量上限（`SY-58`：面板上的数就是进公式的数）。
    ///
    /// ⚠️ 下面那三行看着啰嗦，但**不能省**：把 `Mut<T>` 转成 `&mut T`（例如传给 helper）
    /// 会立刻调 `DerefMut`，Bevy 就此标记“变了”——于是每帧都标记，下游被连锁触发。
    /// 所以：**只在值真的变了的时候才碰它**。
    pub fn apply(
        configs: Res<Assets<RuleConfig>>,
        mut owners: Query<ApplyParts<From, To>>,
    ) {
        for mut item in &mut owners {
            // 取配置：没到货就是错误。
            let config = configs.get(&item.rule.config).unwrap_or_else(|| {
                if item.rule.config.id() == AssetId::<RuleConfig>::default() {
                    panic!("派生规则没有写 config（bsn 里漏了？）");
                }
                panic!(
                    "派生规则的配置还没加载好：{:?}（装配层应该在进游戏前等齐）",
                    item.rule.config
                );
            });

            let value = item.from.value * config.per_point;
            if item.modifier.flat != value {
                item.modifier.flat = value;
            }
        }
    }
}

// ---- 别名：配置与 `bsn!` 里用的名字 ----

/// 血量上限由体质来。
pub type HealthMaxFromConstitution = Formula<Constitution, Max<Health>>;
/// 精力上限由意志来。
pub type StaminaMaxFromWillpower = Formula<Willpower, Max<Stamina>>;
/// 精力回复由意志来（`SY-56`：精力上限与回复 ← 意志）。
pub type StaminaRegenFromWillpower = Formula<Willpower, StaminaRegen>;
