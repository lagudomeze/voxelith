//! 派生：某些量由属性算出来。
//!
//! 一条派生 = **一个实体**（与装备一样，是“修正的一个来源”），它身上有两样东西：
//!
//! - [`Derived<From, To>`]：声明“`To` 由 `From` 算出来”，参数在配置资产里；
//! - [`Modifier<To>`]：算出来的结果（由 `#[require]` 自动带上），由 [`sync_derived`] 写。
//!
//! **读出的地方完全不用知道“派生”这回事**：它看到的还是 `Modifier<To>`，与装备给的修正同构。

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
    From: Send + Sync + 'static,
    To: Send + Sync + 'static,
{
    /// 用哪份配置。
    pub config: Handle<RuleConfig>,
    /// 为什么是 `PhantomData<fn() -> (From, To)>` 而不是 `PhantomData<(From, To)>`：
    ///
    /// 手写 `FromTemplate` 会让这个字段去实现 `Template`，而那要求它 `Unpin`；
    /// `PhantomData<T>` 的 auto trait **跟着 T**，泛型参数证不出 `Unpin` → 编译不过。
    /// `fn() -> …` 是函数指针，永远 `Send + Sync + Unpin`，把这条依赖切断。
    ///
    /// 试过的另外两条路（都不如这条）：
    /// - 拆成两个独立 marker（`PhantomData<From>` + `PhantomData<To>`）：**同样不过**，
    ///   报 `To cannot be unpinned ... within PhantomData<To>`；
    /// - 保留 `PhantomData<(From, To)>` 但给两个参数加 `Unpin` 约束：**能过**，
    ///   但会把 `Unpin` 漏进 `Derived` 的公开约束（struct 与每个 impl 都得写，漏一处就报错）。
    _marker: PhantomData<fn() -> (From, To)>,
}

impl<From, To> Derived<From, To>
where
    From: Send + Sync + 'static,
    To: Send + Sync + 'static,
{
    /// 用一份配置建一条派生规则（`bsn!` 里直接写 `{ config: "路径.ron" }` 也行）。
    pub fn new(config: Handle<RuleConfig>) -> Self {
        Self {
            config,
            _marker: PhantomData,
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

// ---- 别名：配置与 `bsn!` 里用的名字 ----

/// 血量上限由体质来。
pub type DerHealthMaxFromConstitution = Derived<Constitution, Max<Health>>;
/// 精力上限由意志来。
pub type DerStaminaMaxFromWillpower = Derived<Willpower, Max<Stamina>>;
/// 精力回复由意志来（`SY-56`：精力上限与回复 ← 意志）。
pub type DerStaminaRegenFromWillpower = Derived<Willpower, StaminaRegen>;

// ---- 一个泛型 system 管所有规则 ----
//
// 加一条新规则 = 在插件里加一行注册（`sync_derived::<属性, 量>`），不用复制函数。

/// 把每条派生算出来，写进它那条修正。
///
/// **要求配置一定在**：缺席就是错误（`bsn!` 里漏写，或者装配层没等它加载好）。
/// “等加载”是装配层的事——进游戏前把用到的资产等齐，别让 system 自己处理“还没到货”。
///
/// ⚠️ 下面那三行看着啰嗦，但**不能省**：把 `Mut<T>` 转成 `&mut T`（例如传给 helper）
/// 会立刻调 `DerefMut`，Bevy 就此标记“变了”——于是每帧都标记，下游被连锁触发。
/// 所以：**只在值真的变了的时候才碰它**。
pub fn sync_derived<From, To>(
    configs: Res<Assets<RuleConfig>>,
    owners: Query<&From>,
    mut rules: Query<(&Derived<From, To>, &DerivedFor, &mut Modifier<To>)>,
) where
    From: Value,
    To: Send + Sync + 'static,
{
    for (rule, owner, mut modifier) in &mut rules {
        let Ok(from) = owners.get(owner.0) else {
            continue; // 主人没有这个属性：规则挂错了，将来接上日志再吵
        };
        let config = config_of(&rule.config, &configs);
        let value = from.base() * config.per_point;
        if modifier.flat != value {
            modifier.flat = value;
        }
    }
}

/// 取配置：**没到货就是错误**。
fn config_of<'a>(handle: &Handle<RuleConfig>, configs: &'a Assets<RuleConfig>) -> &'a RuleConfig {
    configs.get(handle).unwrap_or_else(|| {
        if handle.id() == AssetId::<RuleConfig>::default() {
            panic!("派生规则没有写 config（bsn 里漏了？）");
        }
        panic!("派生规则的配置还没加载好：{handle:?}（装配层应该在进游戏前等齐）");
    })
}
