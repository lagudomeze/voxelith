//! L2 表现层：**只读** L0/L1 数据，把状态翻成"给人看的东西"。
//!
//! **R15 / R17**：这里不写战斗公式、不改池，只做两件事：
//!
//! 1. [`hud`]：左上 PC 状态面板 + 左下技能列表（**读** L1 的派生数据，点击发 `CastRequest`）；
//! 2. 把新产生的 [`CombatLog`] 条目打出去（真 UI 做出来之前的临时出口）。
//!
//! ## 为什么没有中间层 `CombatView`
//!
//! 曾经有一份 `CombatView`（相位 / 可用技能 / 行动进度的快照）+ `refresh_view` 每帧填它。
//! HUD 落地后它**没有任何读者**：HUD 直接读 `Resources` / `AvailableSkills` / `CombatPhase`
//! （那些本来就是 L1 算好的只读派生数据），中间再抄一份只会多一层要同步的状态。
//! 所以删掉了。将来需要"给 BRP / 调试通道看的一份快照"时再加——而且那时它必须
//! `#[derive(Reflect)]`，否则调试通道照样读不到（见 `work/TODO.md`）。
//!
//! 行动进度同理：`Action::progress()` 在 L1 就有，画行动条时直接读，不必先抄进 Resource。

mod hud;

pub use hud::{
    HudPlugin, HudResourceBar, HudResourceRow, HudResourceValue, HudRoot, HudSkillButton,
    HudSkillsPanel, HudStatusPanel, HudTitle,
};

use bevy::prelude::*;
use voxelith_axiom::behaviors::phase::CombatLog;

/// 把新产生的战斗日志打出去（真 UI 做出来之前的临时出口）。
///
/// 读完就清空，所以每条只打一次。
pub fn print_log(mut log: ResMut<CombatLog>) {
    if log.is_empty() {
        return;
    }
    for entry in log.entries() {
        info!("[战斗] {entry}");
    }
    log.clear();
}

/// 注册表现层。
pub struct PresentationPlugin;

impl Plugin for PresentationPlugin {
    fn build(&self, app: &mut App) {
        // `HudPlugin` 是表现层的**子插件**（R42：不外泄给 `main.rs`）。
        // UiThemePlugin 先于 HudPlugin：HUD 建骨架时要读 UiTheme 里的纹理句柄。
        // 两者都在 Startup，而 uild_ui_theme 用 Commands 插入资源——
        // 同阶段的 Commands 要到阶段末尾才落地，所以这里显式链一下顺序。
        app.add_plugins(crate::ui_theme::UiThemePlugin)
            .add_systems(Update, print_log)
            .add_plugins(HudPlugin);
    }
}
