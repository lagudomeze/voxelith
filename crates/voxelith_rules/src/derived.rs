//! 派生：某些量由属性算出来。
//!
//! 一条派生 = **主人身上的一个公式组件**（[`Formula<From, To>`]，`To` 由 `From` 算出来，
//! 参数在配置资产里）+ **一条算出来的修正**（挂在主人自己身上、指向自己的 [`Modifier<To>`]）。
//!
//! ```text
//! 英雄 ── 体质 + Total<体质>              （属性重算写在 Total 里）
//!      ── Formula<体质, Max<血量>>         （声明：用哪份配置）
//!      ── Modifier<Max<血量>> + Affects(自己)   （结果；重算时与别人的修正一视同仁）
//!      ── Total<Max<血量>>                 （上限重算把修正算进去，读的人看这个）
//! ```
//!
//! 结果那条修正是**自指**的（[`Affects`] 指向自己），所以：
//!
//! - 重算只有一条路径（读名单），不需要"自己那条"特例；
//! - 读的人看到的还是普通修正，与装备给的完全同构。
//!
//! 名字：**概念叫“派生”**（这个模块与它所在的阶段都叫 `derived`），**落地的组件叫 [`Formula`]**——
//! 它表达的就是“用哪条公式、参数在哪份配置”。

use bevy_asset::{AssetId, Assets, Handle};
use bevy_ecs::prelude::*;
use bevy_ecs::query::QueryData;
use core::marker::PhantomData;
use voxelith_core::prelude::*;

use crate::config::RuleConfig;

/// 一条派生公式：`To` 那个量由 `From` 那个属性算出来。
///
/// 参数（每点给多少）在 [`RuleConfig`] 资产里——可改、可复用、可热重载。
/// 因为字段里有 [`Handle`]，这个组件要**手写 `FromTemplate`**（`bsn!` 里才能写路径字符串）。
///
/// **一个量只能有一条自派生公式**：结果写在主人自己身上那条 `Modifier<To>` 里，
/// 一个实体每种组件只能有一份。按 `SY-56`（每个资源各自声明主导属性）目前够用。
#[derive(Component, Clone, Debug, FromTemplate)]
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

/// [`Formula::apply`] 要的四样东西（给这个查询起个名字）。
#[derive(QueryData)]
#[query_data(mutable)]
pub struct ApplyParts<From, To>
where
    From: Send + Sync + 'static + Unpin,
    To: Send + Sync + 'static + Unpin,
{
    /// 主人。
    pub owner: Entity,
    /// 源属性的有效值（派生读它，不是基础值）。
    pub from: &'static Total<From>,
    /// 这条公式。
    pub formula: &'static Formula<From, To>,
    /// 结果那条修正（还没有就是 `None`）。
    pub modifier: Option<&'static mut Modifier<To>>,
}

impl<From, To> Formula<From, To>
where
    From: Send + Sync + 'static + Unpin,
    To: Send + Sync + 'static + Unpin,
{
    /// 在主人身上建一条派生公式（`bsn!` 里直接写 `{ config: "路径.ron" }` 也行）。
    pub fn new(config: Handle<RuleConfig>) -> Self {
        Self {
            config,
            _marker: PhantomData,
        }
    }

    /// 应用这条公式：把结果算出来，写进主人自己那条修正（没有就补一条）。
    ///
    /// **要求配置一定在**：缺席就是错误（`bsn!` 里漏写，或者装配层没等它加载好）。
    /// “等加载”是装配层的事——进游戏前把用到的资产等齐。
    ///
    /// 读的是 `Total<From>`（**有效值**，含装备给的属性加成），不是基础值——所以
    /// “装备 +5 体质”会真的抬血量上限（`SY-58`：面板上的数就是进公式的数）。
    ///
    /// ⚠️ 写值那三行看着啰嗦，但**不能省**：把 `Mut<T>` 转成 `&mut T`（例如传给 helper）
    /// 会立刻调 `DerefMut`，Bevy 就此标记“变了”——于是每帧都标记，下游被连锁触发。
    /// 所以：**只在值真的变了的时候才碰它**。
    pub fn apply(
        configs: Res<Assets<RuleConfig>>,
        mut commands: Commands,
        mut owners: Query<ApplyParts<From, To>>,
    ) {
        for mut item in &mut owners {
            // 取配置：没到货就是错误。
            let config = configs.get(&item.formula.config).unwrap_or_else(|| {
                if item.formula.config.id() == AssetId::<RuleConfig>::default() {
                    panic!("派生公式没有写 config（bsn 里漏了？）");
                }
                panic!(
                    "派生公式的配置还没加载好：{:?}（装配层应该在进游戏前等齐）",
                    item.formula.config
                );
            });

            let value = item.from.value * config.per_point;

            // ⚠️ 用 `as_mut()`，不要用 `as_deref_mut()`：后者一取就调 `DerefMut`，
            // Bevy 立刻标记"变了" ✗ 那样每帧都标记，下游变更检测被连锁触发（踩过两次的坑）。
            match item.modifier.as_mut() {
                // 已经有那条修正：读不标记，只有真写下去才标记
                Some(modifier) => {
                    if modifier.flat != value {
                        modifier.flat = value;
                    }
                }                // 还没有：补一条自指的修正（下次重算就会把它算进 Total）
                None => {
                    let owner = item.owner;
                    commands
                        .entity(owner)
                        .insert((Affects(owner), Modifier::<To>::flat(value)));
                }
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
