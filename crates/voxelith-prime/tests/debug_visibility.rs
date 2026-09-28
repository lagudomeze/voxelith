//! L2 调试设施的守卫测试。
//!
//! 这些测试把"BRP / 检查器能不能看到组件"钉成可执行断言，
//! 因为漏注册反射的失败模式是**静默的**（不报错，只是查不到）。

use bevy::prelude::*;
use voxelith_prime::{VoxelithPlugin, debug};

/// 每个需要在调试视图里可见的组件，都必须能在反射注册表里查到
/// `ReflectComponent` 类型数据。否则 BRP 的 `world.query` 会静默返回空。
#[test]
fn health_is_visible_to_brp() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);

    let registry = app.world().resource::<AppTypeRegistry>().clone();
    let registry = registry.read();

    let registration = registry
        .get_with_type_path("voxelith_axiom::atoms::health::Health")
        .expect("Health 必须已注册反射：BRP 依赖它序列化组件");

    assert!(
        registration.data::<ReflectComponent>().is_some(),
        "Health 缺少 ReflectComponent：请确认派生了 #[reflect(Component)]"
    );
}

/// BRP 的默认端口是 `bevy_remote` 的约定端口。
#[test]
fn default_port_matches_convention() {
    assert_eq!(debug::DEFAULT_BRP_PORT, 15702);
}

/// `VoxelithPlugin` 是完整的顶层插件：构造与配置可用。
#[test]
fn voxelith_plugin_is_constructible() {
    let plugin = VoxelithPlugin::new().with_brp_port(15997);
    assert_eq!(plugin.brp_port, 15997);
    assert_eq!(VoxelithPlugin::default().brp_port, debug::DEFAULT_BRP_PORT);
}

/// 回归保护：egui 检查器必须有渲染目标相机，否则窗口**纯黑且不报错**。
///
/// 实测：无相机时 BRP 截图是 1280×720 全黑（仅 1 种颜色）；
/// 加 `Camera2d` 后出现 146 种颜色、面板落在 x=16–366。
/// 这个失败模式完全静默（进程正常、无警告），所以必须由测试钉住。
#[cfg(feature = "inspector")]
#[test]
fn inspector_spawns_a_camera() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    debug::add_egui_camera(&mut app);

    // 跑一帧让 Startup 系统执行。
    app.update();

    let mut query = app.world_mut().query::<(&Name, &Camera2d)>();
    let names: Vec<String> = query
        .iter(app.world())
        .map(|(name, _)| name.as_str().to_string())
        .collect();

    assert_eq!(
        names,
        vec![debug::EGUI_CAMERA_NAME.to_string()],
        "egui 检查器缺少渲染目标相机：窗口会全黑且不报错"
    );
}
