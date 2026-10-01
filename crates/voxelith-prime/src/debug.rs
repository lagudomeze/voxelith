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
    register_debug_types(app);
    install_brp(app, brp_port);
    install_inspector(app);
}

/// **显式**注册调试通道要读的类型。
///
/// ## 为什么不能只靠 `#[derive(Reflect)]` + `reflect_auto_register`
///
/// `reflect_auto_register` 的语义是"**被 `register_type` 过的**类型，
/// 连带注册它的字段类型"——它**不会**把项目类型自动扫进来。
/// 只加 `#[derive(Reflect)]` / `#[reflect(Component)]` 而不注册，
/// BRP 依然**静默跳过**那个组件（查询返回空，不报错）。
///
/// 这里实测过：加了 derive 与 `#[reflect(Component)]` 之后，
/// `registry.get_type_data::<ReflectComponent>()` 仍然返回 `None`，
/// 必须在本函数里 `register_type` 才算数。
///
/// ## 想查一个新东西时怎么办
///
/// 1. 给类型加 `#[derive(Reflect)]`（组件再加 `#[reflect(Component)]`）；
/// 2. **在这里加一行 `register_type::<T>()`**；
/// 3. `tests/debug_visibility.rs` 会因为"新类型没被断言"提醒你补断言 —— 不，
///    它只断言清单里的那几个；所以**清单和这里要一起维护**。
pub fn register_debug_types(app: &mut App) {
    use voxelith_axiom::atoms::actor::{
        ActionEnergy, Actor, ActorState, ActorTags, Cooldowns, Faction, Monster, Player, Resources,
        Stats,
    };
    app.register_type::<Actor>()
        .register_type::<Player>()
        .register_type::<Monster>()
        .register_type::<Faction>()
        .register_type::<ActorTags>()
        .register_type::<Resources>()
        .register_type::<Stats>()
        .register_type::<Cooldowns>()
        .register_type::<ActorState>()
        .register_type::<ActionEnergy>()
        .register_type::<crate::voxel_render::ChunkMeshEntity>()
        .register_type::<crate::actor_render::Movement>()
        .register_type::<crate::actor_render::ActorVisual>()
        // **资源也要注册**（组件用 #[reflect(Component)]，资源用 #[reflect(Resource)]）。
        // 没注册的话 BRP 会报 Unknown resource type —— 这也是静默失败的一种：
        // 你以为"读不到相机参数"，其实是没注册。
        .register_type::<crate::voxel_render::camera::CameraConfig>()
        // **地形的"哪些区块建好了网格"** —— 排查画面黑洞靠它。
        .register_type::<crate::voxel_render::ChunkMeshIndex>()
        // **体素表**：dited 字段是"编辑有没有生效"的运行期证据
        // （chunks 用 #[reflect(ignore)] 跳过了 —— 一个区块 32768 个体素）。
        .register_type::<voxelith_axiom::world::VoxelStore>();

    // `Resources` / `Stats` 内部是 `HashMap<Id, _>`，把 ID 也注册上，
    // 否则 BRP 序列化到键时会缺类型信息。
    app.register_type::<voxelith_axiom::behaviors::content::ResourceId>()
        .register_type::<voxelith_axiom::behaviors::content::StatId>()
        // **字段类型也要注册**：ReflectComponent 序列化组件时要按字段类型查注册表，
        // 缺一个字段类型就可能整条查询返回空（而且不报错）。
        // ChunkMeshEntity 的字段就是它 —— 踩过一次。
        .register_type::<voxelith_axiom::world::ChunkPos>();
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
    commands.spawn((
        Name::new(EGUI_CAMERA_NAME),
        // **不参与 3D 渲染**：`order` 最大 = 最后画，把 egui 面板叠到已画好的场景上。
        // `clear_color: None` 是关键——它一旦清屏，战场就被刷成纯色
        // （实测表现就是"地形看不见了"，而所有实体与网格其实都在）。
        Camera {
            order: 1_000,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        Camera2d,
    ));
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
