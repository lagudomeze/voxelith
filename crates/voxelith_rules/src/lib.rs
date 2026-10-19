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

mod action;
mod config;
mod cost;
mod derived;
mod recalc;
mod time;

use bevy_app::prelude::*;
use bevy_asset::AssetApp;
use bevy_ecs::prelude::*;
use bevy_time::TimePlugin;

use voxelith_core::prelude::{Adjust, Health, Mana, Stamina, apply_adjustments};

use crate::action::{BeginAction, ResolveAction};
use crate::time::Freeze;

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
        // 时钟：世界该不该走，靠 Freeze 三个触发决定。
        // TimePlugin 由我们补上（rules 自己就能跑起来，不用装配层记得加）。
        if !app.is_plugin_added::<TimePlugin>() {
            app.add_plugins(TimePlugin);
        }
        app.init_resource::<Freeze>()
            // 推进权只有一个地方：apply_freeze（RunFixedMainLoop 之前）
            .add_systems(PreUpdate, time::apply_freeze);

        // 行动：释放与到点都住 FixedUpdate，与时间轴同频。
        app.add_message::<BeginAction>()
            .add_message::<ResolveAction>()
            // 改值消息也归这里注册：注册系统却不注册消息，运行时会直接报“资源不存在”
            .add_message::<Adjust<Health>>()
            .add_message::<Adjust<Stamina>>()
            .add_message::<Adjust<Mana>>()
            .add_systems(
                FixedUpdate,
                (
                    // 改值的唯一通路：三种资源的当前值都从这里落地
                    apply_adjustments::<Health>,
                    apply_adjustments::<Stamina>,
                    apply_adjustments::<Mana>,
                    action::begin_actions,
                    action::resolve_actions,
                )
                    .chain(),
            );

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
    pub use crate::cost::{Costs, RejectReason, ResourceKind, try_pay};
    pub use crate::derived::{
        ApplyParts, Formula, HealthMaxFromConstitution, StaminaMaxFromWillpower,
        StaminaRegenFromWillpower,
    };
    pub use crate::action::{
        Action, ActionOf, ActionParams, Actions, BeginAction, ParamKind, Potency, ResolveAction,
        SkillDef, SkillId, begin_actions, cancel_action, resolve_actions, skill_def,
    };
    pub use crate::recalc::{DerivedRules, RecalcAttributes, RecalcTotals, RecomputeParts};
    pub use crate::time::{Freeze, apply_freeze, round_to_step, seconds_to_step};
    pub use crate::{RulesPlugin, recompute};
}
