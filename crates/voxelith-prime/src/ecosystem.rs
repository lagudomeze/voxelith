//! **L2 生态接线**：8 个生态库 + 新架构中间层，一处装配。
//!
//! ```text
//! 输入   leafwing-input-manager   GameAction（Actionlike）+ 默认绑定
//! 音频   bevy_kira_audio          AudioPlugin
//! 粒子   bevy_hanabi              HanabiPlugin
//! 补间   bevy_tweening            TweeningPlugin
//! 物理   avian3d                  PhysicsPlugins
//! 资产   bevy_asset_loader        ContentLoad 状态机 + `CombatAssets` 集合（四份 .ron）
//! 存档   moonshine-save           observer 式：Save / Load（不是 Plugin）
//! 调试   bevy_egui                 EguiPlugin（**本插件里装**；inspector 是可选附加）
//! 机制   voxelith-abilities       `abilities::plugin()`：格子后端 + 三件套
//! ```
//!
//! ## 三个"形态不一样"的库（踩过才知道）
//!
//! | 库 | 形态 | 怎么装 |
//! |---|---|---|
//! | 多数 | 一个 `Plugin` | `add_plugins(XxxPlugin)` |
//! | `bevy_asset_loader` | **状态机**：没有总插件 | `add_loading_state(LoadingState::new(A).continue_to_state(B))` |
//! | `moonshine-save` | **observer**：没有总插件 | `add_observer(save_on_default_event)` / `add_observer(load_on_default_event)`，用 `commands.trigger_save(...)` / `trigger_load(...)` 触发 |
//!
//! ## 顺序
//!
//! 生态库都排在 `DefaultPlugins` **之后**（它们依赖窗口 / 资产 / 渲染 / 输入）。
//! `abilities::plugin()` 里的 gauge 属性插件有一个**进程级全局 interner**，
//! 所以内容解析必须发生在它之后（见 `voxelith_abilities::attributes` 的模块文档）。

use avian3d::prelude::PhysicsPlugins;
use bevy::prelude::*;
// 只在"没有 inspector"时用得上（两个 egui 插件不能同时装，见下面 `build` 里的说明）。
#[cfg(not(feature = "inspector"))]
use bevy_egui::EguiPlugin;
use bevy_hanabi::HanabiPlugin;
use bevy_kira_audio::AudioPlugin;
use bevy_tweening::TweeningPlugin;
use leafwing_input_manager::prelude::*;
use moonshine_save::prelude::*;

/// 玩家动作（`leafwing-input-manager` 的 `Actionlike`）。
///
/// 加一个动作 = 加一行 + 一条默认绑定；绑定表是内容，不该散在系统里。
#[derive(Actionlike, Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect)]
pub enum GameAction {
    /// 向北走一格。
    MoveNorth,
    /// 向南走一格。
    MoveSouth,
    /// 向东走一格。
    MoveEast,
    /// 向西走一格。
    MoveWest,
    /// 确认（攻击 / 交互）。
    Confirm,
    /// 取消（关闭窗口 / 退出瞄准）。
    Cancel,
    /// 技能 1。
    Ability1,
    /// 技能 2。
    Ability2,
    /// 技能 3。
    Ability3,
}

/// 内容加载状态（`bevy_asset_loader` 驱动）。
///
/// 现在**不挂任何集合**：旧的 `ContentPlugin` 还在自己读 `.ron`。
/// 等内容迁到新管线时，把集合挂到这里，启动流程就变成
/// "`Loading` → 资产齐了 → `Ready`"，而不是各插件自己抢 `Startup`。
#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ContentLoad {
    /// 正在加载。
    #[default]
    Loading,
    /// 加载完成，可以开始。
    Ready,
}

/// 装齐 8 个生态库与中间层。
pub struct EcosystemPlugin;

impl Plugin for EcosystemPlugin {
    fn build(&self, app: &mut App) {
        // ---- 输入 ----
        app.add_plugins(InputManagerPlugin::<GameAction>::default());

        // ---- 音频 / 粒子 / 补间 ----
        app.add_plugins((AudioPlugin, HanabiPlugin, TweeningPlugin));

        // ---- 物理 ----
        app.add_plugins(PhysicsPlugins::default());

        // ---- 调试 UI（egui）----
        // `bevy_egui` 是这 8 个库之一，所以它有**自己的接线口**（不靠 inspector 间接带进来）。
        //
        // ⚠️ 但**同一时刻只能装一个 egui 插件**：`bevy-inspector-egui` 还停在
        // `bevy_egui 0.40`（工作区是 0.42），而它的 `install_inspector` 会装一个
        // **它那个版本**的 `EguiPlugin`。两个都装 = 两套 egui 上下文（真跑起来会出问题 ✗）。
        //
        // 所以按特性二选一：
        //   · 开 `inspector`（默认）：用 inspector 那一套（它的 egui 版本与它匹配）
        //   · 关 `inspector`：装我们自己的 0.42
        // 版本对齐之后这个 `cfg` 就能删掉（那时两处指的是同一个 egui）。
        #[cfg(not(feature = "inspector"))]
        app.add_plugins(EguiPlugin::default());
        #[cfg(feature = "inspector")]
        info!("[ecosystem] `inspector` 特性开着：egui 由 `bevy-inspector-egui` 提供（0.40）");

        // ---- 资产加载：状态机形态（**这里是它真正干活的地方**）----
        // ① 注册我们自己的 `.ron` 文本资产与加载器（Bevy 自带的只认 `.txt`）；
        // ② 把四份内容做成一个集合——集合加载完状态机才翻到 `Ready`（以前是空转）。
        // 状态与集合都由这个插件装（它内部 `init_state` + `add_loading_state`）——
        // 这里再 init 一次会撞"状态已存在"。
        app.add_plugins(crate::combat_assets::CombatAssetsPlugin);

        // ---- 存档：observer 形态（没有总插件）----
        app.add_observer(save_on_default_event)
            .add_observer(load_on_default_event);

        // ---- 新架构中间层：格子后端 + gauge / gearbox / diesel ----
        app.add_plugins(voxelith_abilities::plugin());
    }
}
