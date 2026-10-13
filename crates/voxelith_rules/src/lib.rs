//! 规则层：**量之间的依赖**——目前只有“派生”（某些量由属性算出来）。
//!
//! 分工（见 `docs/design/`）：
//!
//! | | 住哪 |
//! | --- | --- |
//! | **公式** | 代码（[`sync_derived`] 里那几行） |
//! | **参数** | 资产 [`RuleConfig`]（一份文件一条规则：可改、可复用、可热重载） |
//! | **有哪些规则、挂在谁身上** | `bsn!` / 模板（内容声明有无） |
//!
//! 这一层**依赖 `bevy_asset`**，所以不放在 `voxelith_core`——core 只有 `bevy_ecs`，
//! 保持“无资产、无渲染也能测”。

mod config;
mod derived;

pub use config::{RuleConfig, RuleConfigLoader};
pub use derived::*;

use bevy_app::prelude::*;
use bevy_asset::AssetApp;
use voxelith_core::prelude::*;

/// 把派生规则接上：注册配置资产 + **注册每种 (属性 → 量) 的组合**。
///
/// 加一条新规则＝在下面加一行 `sync_derived::<属性, 量>`。
///
/// **顺序**：这些系统写的是修正，所以夹取当前值、读上限的那些系统要排在它们后面
/// （等有了那些系统再显式定 set 顺序）。
pub struct DerivedRulesPlugin;

impl Plugin for DerivedRulesPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<RuleConfig>()
            .register_asset_loader(RuleConfigLoader)
            .add_systems(
                Update,
                (
                    sync_derived::<Constitution, Max<Health>>,
                    sync_derived::<Willpower, Max<Stamina>>,
                    sync_derived::<Willpower, StaminaRegen>,
                ),
            );
    }
}

/// 常用类型的入口。
pub mod prelude {
    pub use crate::config::RuleConfig;
    pub use crate::derived::{
        DerHealthMaxFromConstitution, DerStaminaMaxFromWillpower, DerStaminaRegenFromWillpower,
        Derived, DerivedFor, DerivedModifiers,
    };
    pub use crate::{sync_derived, DerivedRulesPlugin};
}
