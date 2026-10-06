//! 新架构的中间层：**技能三件套**（`bevy_diesel` + `bevy_gauge` + `bevy_gearbox`）
//! 与生态插件的落点。
//!
//! 分层（重写中，逐步落地）：
//!
//! ```text
//! voxelith-axiom      格子 / 体素世界 / 词汇（纯逻辑，零渲染）
//! voxelith-abilities  属性图（gauge） + 状态图（gearbox） + 技能框架（diesel）  ← 本 crate
//! voxelith-prime      Bevy app / 渲染 / UI / 资产 / 存档
//! ```
//!
//! 目前只验证一件事：**三件套能在 Bevy 0.19 上共存**（尤其 `bevy_diesel 0.2`
//! 依赖 `bevy_gearbox ^0.8`，必须与工作区里声明的 gearbox 版本收敛成同一份）。

use bevy_gauge::modifier_set::ModifierSet;

pub mod ability;
pub mod actors;
pub mod attributes;
pub mod builder;
pub mod casting;
pub mod contest;
pub mod counter;
pub mod dispel;
pub mod grid;
pub mod numeric;
pub mod periodic;
pub mod skills;
pub mod stacking;
pub mod statuses;
pub mod tags;
pub mod threat;

pub use grid::{GridBackend, GridBackendPlugin, GridFilter, GridGatherer, GridOffset};

// ── 给 L2 的再导出面 ────────────────────────────────────────────────────────
//
// 表现层需要**少数几个**别人的类型才能装配（注册模板、生成实体时挂属性与归属）。
// 与其让 `voxelith-prime` 直接依赖 diesel/gauge（那样它会开始随手用它们内部的东西），
// 不如在这里开一个**小的、明确的**再导出面。
pub use bevy_diesel::invoker::{InvokedBy, resolve_root};
pub use bevy_diesel::spawn::{SpawnConfig, TemplateRegistry};
pub use bevy_diesel::target::InvokerTarget;
// 状态机的两个原语：读"哪个状态在生效"（`Active`）与"这个状态属于哪个根"（`SubstateOf`）。
// HUD 要按技能的三个生命周期标记（`Ready` / `Invoking` / `Cooling`）显示槽位，
// 而标记挂在**状态实体**上——所以这两个类型必须能出去。
// 注意仍然**不导出 gearbox 本身**：L2 拿不到"随便写状态机"的能力。
pub use bevy_gauge::prelude::{
    AttributeInitializer, Attributes, AttributesMut, InstantExt, InstantModifierSet,
};
pub use bevy_gearbox::{Active, EdgeTimer, SubstateOf, Transitions};

/// 一次性装好新架构的中间层：**格子空间后端 + 技能三件套 + 对抗部件**。
///
/// 状态机（gearbox）、属性图（gauge）、技能管线（diesel）、目标解析（格子后端）、
/// 对抗判定（[`contest`]）全在这一句里；L2 不需要知道 diesel 的存在，
/// 也不必自己拼系统顺序（顺序是契约，写错只表现为"技能不触发"，所以只留一个入口）。
pub fn plugin() -> impl bevy::app::Plugin {
    AbilitiesPlugin
}

/// 中间层的组合插件。
///
/// 为什么不用元组：**Bevy 0.19 不给元组实现 `Plugin`**（`add_plugins((A, B))` 走的是
/// 另一条 `Plugins` 路径），所以 `-> impl Plugin` 只能返回一个真正的插件类型。
pub struct AbilitiesPlugin;

impl bevy::app::Plugin for AbilitiesPlugin {
    fn build(&self, app: &mut bevy::app::App) {
        app.add_plugins((
            GridBackendPlugin,
            contest::ContestPlugin,
            casting::CastingPlugin,
            threat::ThreatPlugin,
            counter::CounterPlugin,
            dispel::DispelPlugin,
            numeric::NumericPlugin,
            periodic::PeriodicPlugin,
            stacking::StackingPlugin,
        ));
    }
}

/// 属性图的最小用法：构造一个属性集。
///
/// 真接入时这里会长成"从 `.ron` 读属性定义 → `ModifierSet` → `try_apply`
/// （表达式编译失败在**加载期**报出来）"。
pub fn smoke_attributes() -> ModifierSet {
    let mut set = ModifierSet::new();
    set.add("MaxHealth", 120.0);
    set.add_expr("Health", "MaxHealth * 1.0");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_three_frameworks_are_present() {
        let set = smoke_attributes();
        assert!(!set.is_empty(), "gauge 的属性集能构造出来");
        // gearbox / diesel 由上面的 `use ... as _` 证明可解析；
        // 版本收敛由 `cargo tree -i bevy_gearbox` 检查（只能有一份）。
    }
}
