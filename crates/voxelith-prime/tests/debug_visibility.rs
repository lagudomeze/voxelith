//! L2 调试设施的守卫测试。
//!
//! 这些测试把"BRP / 检查器能不能看到东西"钉成可执行断言，
//! 因为漏注册反射的失败模式是**静默的**（不报错，只是查不到）。

use bevy::ecs::reflect::ReflectComponent;
use bevy::prelude::*;
use bevy::reflect::TypeRegistry;
use voxelith_prime::{VoxelithPlugin, debug};

/// **BRP 能读到项目组件**（这条以前是缺的）。
///
/// ## 为什么必须由测试钉住
///
/// BRP 用 `ReflectSerializer` 序列化组件；**未注册反射的组件不报错、只被跳过**。
/// 所以"忘了 `#[derive(Reflect)]`"这个失败模式是**完全静默的** ——
/// 查询返回空，而你会以为是"世界里没有这个组件"。
///
/// 这个坑实际发生过：想用 BRP 查区块网格实体（`ChunkMeshEntity`）来定位
/// 画面上的黑洞，结果 `world.query` 一律返回
/// `Component ... isn't registered or used in the world` ——
/// **连"世界里到底有几个区块网格"都查不到**，排查直接卡住。
///
/// ## 断言的是"能不能当组件取"，不是"类型在不在注册表里"
///
/// 第一版只断言 `registry.get(type_id).is_some()`，**验证时发现它抓不到 bug**
/// （去掉 `Reflect` 后测试照样通过）—— 那种断言等于没写。
///
/// `ReflectComponent` 才是 BRP 真正要用的那份类型数据：
/// 有它才能按键取组件、才算"查得到"。
fn assert_component_reflectable<T: bevy::reflect::TypePath>(registry: &TypeRegistry, what: &str) {
    let id = std::any::TypeId::of::<T>();
    assert!(
        registry.get(id).is_some(),
        "{what} 的类型没注册，BRP 查不到它"
    );
    assert!(
        registry.get_type_data::<ReflectComponent>(id).is_some(),
        "{what} 缺少 ReflectComponent 类型数据：BRP 会**静默跳过**它，\
         查询返回空而你会以为世界里没有这个组件"
    );
}

#[test]
fn debug_channel_can_reflect_the_project_components() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    // **走真实注册路径**：`debug::register_debug_types` 就是产品代码里注册那些
    // 类型的地方。不手工 `finish()` 糊过去 —— 那样测的是测试自己搭的注册表。
    debug::register_debug_types(&mut app);
    app.finish();
    app.cleanup();

    let registry = app.world().resource::<AppTypeRegistry>().read();

    // 地形：定位"画面上有没有这个区块"。
    assert_component_reflectable::<voxelith_prime::voxel_render::ChunkMeshEntity>(
        &registry,
        "ChunkMeshEntity",
    );
    // 角色表现：查位置与动画帧。
    assert_component_reflectable::<voxelith_prime::actor_render::Movement>(&registry, "Movement");
    assert_component_reflectable::<voxelith_prime::actor_render::ActorVisual>(
        &registry,
        "ActorVisual",
    );
    // L0 的池与属性：查血量、查加点。
    assert_component_reflectable::<voxelith_axiom::atoms::actor::Resources>(&registry, "Resources");
    assert_component_reflectable::<voxelith_axiom::atoms::actor::Stats>(&registry, "Stats");
}

/// BRP 的默认端口是 `bevy_remote` 的约定端口。
#[test]
fn default_port_matches_convention() {
    assert_eq!(debug::DEFAULT_BRP_PORT, 15702);
}

/// `VoxelithPlugin` 是完整的顶层插件：构造与配置可用。
#[test]
fn voxelith_plugin_is_constructible() {
    let plugin = VoxelithPlugin::new().with_brp_port(15997).with_rng_seed(7);
    assert_eq!(plugin.brp_port, 15997);
    assert_eq!(plugin.rng_seed, 7);
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
