//! 规则层：**量之间的关系**——有效值缓存的重算，以及“派生”（某些量由属性算出来）。
//!
//! 分工（见 `docs/design/`）：
//!
//! | | 住哪 |
//! | --- | --- |
//! | **公式的实质** | `voxelith_core`（`Modifier::contribution`：比例按基础值） |
//! | **求和与写缓存** | 这里（[`recalc::recompute`]，它知道修正怎么挂） |
//! | **公式的参数** | 资产 [`RuleConfig`]（一份文件一条规则：可改、可复用、可热重载） |
//! | **有哪些规则、挂在谁身上** | `bsn!` / 模板（内容声明有无） |
//! | **修正怎么挂** | 不用管：归属是库自带的 `Affects` / `Modifiers`；来源用标签组件区分 |
//!
//! 这一层**依赖 `bevy_asset` 与 `bevy_app`**，所以不放在 `voxelith_core`——core 只有
//! `bevy_ecs`，保持“无资产、无渲染也能测”。

mod config;
mod derived;
mod recalc;

use bevy_app::prelude::*;
use bevy_asset::AssetApp;
use bevy_ecs::prelude::*;

pub use config::{RuleConfig, RuleConfigLoader};
pub use derived::*;
pub use recalc::{DerivedRules, RecalcAttributes, RecalcTotals, recompute};

/// 把规则层接上：**重算有效值 + 派生**，顺序也一并定好。
///
/// **没有泛型参数**：归属是库自带的唯一一条关系（`Affects` / `Modifiers`），所以不可能出现"挂两个实例、各算一半"的情况。
///
/// ```ignore
/// app.add_plugins(RulesPlugin);
/// ```
///
/// 一帧里的三段顺序：属性重算 → 派生写修正 → 回复速率与资源上限重算。
///
/// **加一条派生规则**＝两处各一行：内容里给一份 `.ron`，下面 `add_systems` 里加一行注册。
#[derive(Default)]
pub struct RulesPlugin;

impl Plugin for RulesPlugin {
    fn build(&self, app: &mut App) {
        // 有效值缓存的重算（认识归属的那一半）
        recalc::install(app);

        // 配置资产 + 派生规则
        app.init_asset::<RuleConfig>()
            .register_asset_loader(RuleConfigLoader)
            .add_systems(
                Update,
                (
                    HealthMaxFromConstitution::apply,
                    StaminaMaxFromWillpower::apply,
                    StaminaRegenFromWillpower::apply,
                )
                    .in_set(DerivedRules),
            );
    }
}

/// 常用类型的入口。
pub mod prelude {
    pub use crate::config::RuleConfig;
    pub use crate::derived::{
        ApplyParts, Formula, HealthMaxFromConstitution, StaminaMaxFromWillpower,
        StaminaRegenFromWillpower,
    };
    pub use crate::recalc::{DerivedRules, RecalcAttributes, RecalcTotals, RecomputeParts};
    pub use crate::{RulesPlugin, recompute};
}
