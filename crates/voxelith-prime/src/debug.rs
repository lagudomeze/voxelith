//! L2 调试设施：让 AI（BRP over HTTP）与 egui 检查器（人）看到 ECS 组件数据。
//!
//! ## BRP 就是一个插件
//!
//! 官方用法只有一行：
//!
//! ```ignore
//! app.add_plugins(BrpExtrasPlugin::default());
//! ```
//!
//! `BrpExtrasPlugin` 自动补齐缺失的 `RemotePlugin`（BRP 方法集）与 `RemoteHttpPlugin`
//! （HTTP 传输，默认 `127.0.0.1:15702`），并追加 `brp_extras/*`（截图 / 输入注入 / 优雅关闭）。
//!
//! **必须添加在 `DefaultPlugins` 之后**：它的输入注入系统依赖 `InputPlugin`/
//! `WindowPlugin` 注册的消息（`KeyboardInput` / `CursorMoved` / `MouseButtonInput`）。
//! 若改用 `MinimalPlugins`，这些消息不存在，会在首帧 panic —— 本项目统一用
//! `DefaultPlugins`，不提供无头变体。
//!
//! ## 组件可见性
//!
//! BRP 用 `ReflectSerializer` 序列化组件，未注册反射的组件**不报错、只被跳过**。
//! Bevy 默认启用的 `reflect_auto_register` 会自动注册 `#[derive(Reflect)]` 类型
//! 及其 type data（含 `#[reflect(Component)]`），所以**不需要手写注册**。
//! 防漏写靠守卫测试：[`tests/debug_visibility.rs`](../../tests/debug_visibility.rs)。
//!
//! 归属：本模块属 L2，只读 L0/L1 数据（**R15**），不写核心数据（**R16**）。
//! 调试设施禁止下沉到 `voxelith-axiom`（**R5**、**R99**）。

use bevy::prelude::*;
use bevy_brp_extras::BrpExtrasPlugin;

/// BRP 默认监听端口（`bevy_remote` 的默认端口约定）。
pub const DEFAULT_BRP_PORT: u16 = 15702;

/// 安装全部调试设施。需在 `DefaultPlugins` 之后调用。
pub fn install(app: &mut App, brp_port: u16) {
    install_brp(app, brp_port);
    install_inspector(app);
}

/// 安装 BRP（AI / 脚本读取与修改组件的主通道）。
fn install_brp(app: &mut App, port: u16) {
    app.add_plugins(BrpExtrasPlugin::with_port(port));
    info!("BRP 已就绪：http://127.0.0.1:{port}（POST JSON-RPC 即可查询）");
}

/// 安装 egui 世界检查器（人用的组件下拉浏览器）。
///
/// Bevy **没有**内置世界检查器（`bevy_dev_tools` 只有 `ci_testing` /
/// `frame_time_graph` / `schedule_data`），所以用 `bevy-inspector-egui`。
///
/// 顺序不可颠倒：`EguiPlugin` → `WorldInspectorPlugin`，否则 panic。
///
/// **必须有相机**：egui 的 UI 是通过相机渲染到窗口的，没有相机时窗口是纯黑、
/// 检查器完全不显示（且不会报错）。本 crate 是骨架阶段，场景里原本没有相机，
/// 所以这里补一个 [`Camera2d`] 作为 egui 的渲染目标。
#[cfg(feature = "inspector")]
fn install_inspector(app: &mut App) {
    // bevy-inspector-egui 转导出 bevy_egui，无需单独声明依赖。
    use bevy_inspector_egui::bevy_egui::EguiPlugin;
    use bevy_inspector_egui::quick::WorldInspectorPlugin;

    if !app.is_plugin_added::<EguiPlugin>() {
        app.add_plugins(EguiPlugin::default());
    }
    app.add_plugins(WorldInspectorPlugin::new());
    add_egui_camera(app);
    info!("egui 世界检查器已启用（仅显示已注册反射的组件）");
}

/// 为 egui 提供一个渲染目标相机。
///
/// 没有它，窗口是纯黑的：egui 的 UI 必须经由某个相机渲染。
/// 实测证据：无相机时 BRP 截图是 1280×720 **全黑（仅 1 种颜色）**；
/// 加上 `Camera2d` 后出现 146 种颜色，检查器面板落在 x=16–366。
///
/// 该相机是 egui 的渲染目标，**不是游戏相机**；接入真实玩法渲染时若已有主相机，
/// 应复用主相机而不是再补一个。
#[cfg(feature = "inspector")]
fn spawn_camera_for_egui(mut commands: Commands) {
    commands.spawn((Name::new(EGUI_CAMERA_NAME), Camera2d));
}

/// egui 渲染目标相机的名字（供测试断言）。
#[cfg(feature = "inspector")]
pub const EGUI_CAMERA_NAME: &str = "egui-camera";

/// 供测试调用：把 egui 渲染目标相机的生成系统加到 app 上。
#[cfg(feature = "inspector")]
pub fn add_egui_camera(app: &mut App) {
    app.add_systems(Startup, spawn_camera_for_egui);
}

#[cfg(not(feature = "inspector"))]
fn install_inspector(_app: &mut App) {
    info!("未启用 `inspector` feature，跳过 egui 检查器（BRP 不受影响）");
}
