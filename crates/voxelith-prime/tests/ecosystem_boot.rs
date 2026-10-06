//! 新架构的**装配冒烟**：不开窗口，把机制层装齐跑几帧。
//!
//! 它验证的是"这些库能不能同处一个 App"——依赖解析对了、插件注册顺序不冲突、
//! 系统参数没有 B0001/B0002、没有缺资源。
//!
//! 被验证的：`bevy_tweening`（补间）+ `leafwing-input-manager`（输入）
//! + `voxelith-abilities`（gauge / gearbox / diesel + 格子后端）。
//!
//! **不在这里的，以及为什么**（这些只有 `cargo run` 那条路能验证）：
//!
//! | 库 | 为什么进不了无窗口冒烟 |
//! |---|---|
//! | `avian3d` | 它的 collider 缓存读 `MessageReader<AssetEvent<Mesh>>`——**物理库挂在渲染资产路径上**，没有 mesh 插件就是"Message not initialized" |
//! | `bevy_hanabi` | 粒子要渲染图与 GPU 资源 |
//! | `bevy_kira_audio` | 要音频设备 |
//! | `bevy_asset_loader` | 状态机要配合集合才有意义（现在还没有集合） |
//!
//! `InputPlugin` **必须显式加**：leafwing 读 `ButtonInput<KeyCode>` /
//! `ButtonInput<MouseButton>` / `AccumulatedMouseMotion` / `AccumulatedMouseScroll`，
//! 少一个就在第一次 update 报 "Resource does not exist"，而报错里是 leafwing 的系统名。

use bevy::asset::AssetPlugin;
use bevy::input::InputPlugin;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use bevy::transform::TransformPlugin;
use bevy_tweening::TweeningPlugin;
use leafwing_input_manager::prelude::*;
use voxelith_prime::ecosystem::GameAction;

#[test]
fn the_mechanism_stack_assembles_without_a_window() {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        TransformPlugin,
        StatesPlugin,
        InputPlugin,
        AssetPlugin::default(),
        bevy::scene::ScenePlugin,
        // 生态库
        TweeningPlugin,
        InputManagerPlugin::<GameAction>::default(),
        // 新架构中间层：格子后端 + 状态图 + 属性图 + 技能管线
        voxelith_abilities::plugin(),
    ));

    // 装配成功与否，跑一帧就知道（缺资源 / 参数冲突都在第一次 update 炸出来）。
    app.update();
    app.update();
}
