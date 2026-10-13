//! 派生：某些量由属性算出来。
//!
//! 一条派生 = **一个实体**（与装备一样，是“修正的一个来源”），它身上有两样东西：
//!
//! - [`Derived<From, To>`]：声明“`To` 由 `From` 算出来”，参数在配置资产里；
//! - [`Modifier<To>`]：算出来的结果（由 `#[require]` 自动带上），由下面的同步系统写。
//!
//! **读出的地方完全不用知道“派生”这回事**：它看到的还是 `Modifier<To>`，与装备给的修正同构。

use bevy_asset::{AssetId, AssetServer, Assets, Handle, LoadState};
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

// ---- 读配置：缺了要吵，没到货就等 ----

/// 取配置。三种情况分得很清楚：
///
/// - 到货了 → 用它；
/// - **没写 `config`**（`bsn!` 里漏了，字段还是默认 handle）→ **报错**；
/// - **加载失败**（文件不在、格式坏）→ **报错**；
/// - 还在加载 → 这一帧不算（`None`），加载完那一帧自然补上。
fn config_of<'a>(
    handle: &Handle<RuleConfig>,
    configs: &'a Assets<RuleConfig>,
    asset_server: &AssetServer,
    owner: Entity,
) -> Option<&'a RuleConfig> {
    if let Some(config) = configs.get(handle) {
        return Some(config);
    }
    if handle.id() == AssetId::<RuleConfig>::default() {
        panic!("实体 {owner:?} 的派生规则没有写 config（bsn 里漏了？）");
    }
    match asset_server.load_state(handle) {
        LoadState::Failed(err) => panic!("实体 {owner:?} 的派生规则配置加载失败：{err}"),
        _ => None,
    }
}

/// 这条派生规则应该给多少。
fn derived_value<From: Value>(from: &From, config: &RuleConfig) -> f32 {
    from.base() * config.per_point
}

// ---- 每条 (属性 → 量) 一条同步系统 ----
//
// 加一条新规则 = 复制一段、换类型、在插件里注册一行。
//
// ⚠️ 下面那三行看着啰嗦，但**不能省**：把 `Mut<T>` 转成 `&mut T`（例如
// `apply(…, &mut modifier)`）会立刻调 `DerefMut`，Bevy 就此标记“变了”——
// 于是每帧都标记，下游的变更检测被连锁触发。
// 所以：**只在值真的变了的时候才碰它**，比较与赋值都留在 system 里。

pub(crate) fn sync_health_max_from_constitution(
    asset_server: Res<AssetServer>,
    configs: Res<Assets<RuleConfig>>,
    owners: Query<&Constitution>,
    mut rules: Query<(
        &Derived<Constitution, Max<Health>>,
        &DerivedFor,
        &mut Modifier<Max<Health>>,
    )>,
) {
    for (rule, owner, mut modifier) in &mut rules {
        let Ok(constitution) = owners.get(owner.0) else {
            continue; // 主人没有体质：规则挂错了，将来接上日志再吵
        };
        let Some(config) = config_of(&rule.config, &configs, &asset_server, owner.0) else {
            continue;
        };
        let value = derived_value(constitution, config);
        if modifier.flat != value {
            modifier.flat = value;
        }
    }
}

pub(crate) fn sync_stamina_max_from_willpower(
    asset_server: Res<AssetServer>,
    configs: Res<Assets<RuleConfig>>,
    owners: Query<&Willpower>,
    mut rules: Query<(
        &Derived<Willpower, Max<Stamina>>,
        &DerivedFor,
        &mut Modifier<Max<Stamina>>,
    )>,
) {
    for (rule, owner, mut modifier) in &mut rules {
        let Ok(willpower) = owners.get(owner.0) else {
            continue;
        };
        let Some(config) = config_of(&rule.config, &configs, &asset_server, owner.0) else {
            continue;
        };
        let value = derived_value(willpower, config);
        if modifier.flat != value {
            modifier.flat = value;
        }
    }
}

pub(crate) fn sync_stamina_regen_from_willpower(
    asset_server: Res<AssetServer>,
    configs: Res<Assets<RuleConfig>>,
    owners: Query<&Willpower>,
    mut rules: Query<(
        &Derived<Willpower, StaminaRegen>,
        &DerivedFor,
        &mut Modifier<StaminaRegen>,
    )>,
) {
    for (rule, owner, mut modifier) in &mut rules {
        let Ok(willpower) = owners.get(owner.0) else {
            continue;
        };
        let Some(config) = config_of(&rule.config, &configs, &asset_server, owner.0) else {
            continue;
        };
        let value = derived_value(willpower, config);
        if modifier.flat != value {
            modifier.flat = value;
        }
    }
}
