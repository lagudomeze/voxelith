//! **内容走资产管线**的端到端验证：集合 → 状态机 → 读文本 → 内容就绪。
//!
//! 这条测的意义：在此之前 `bevy_asset_loader` 只是**装了个空转的状态机**
//! （`LoadingState` 里没有任何集合）✗。现在四份 `.ron` 是一个集合，
//! 集合加载完状态机才翻到 `Ready`，内容才从 `Assets<RonSource>` 里读出来。

use bevy::asset::AssetPlugin;
use bevy::input::InputPlugin;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use voxelith_prime::combat::{CombatContent, CombatContentPlugin, Playable};
use voxelith_prime::combat_assets::CombatAssetsPlugin;
use voxelith_prime::ecosystem::ContentLoad;

/// 资产根：与 `main.rs` 里 `AssetPlugin` 用的是同一个（编译期定死，不看当前目录）。
fn asset_root() -> String {
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets").to_string()
}

fn test_app() -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        StatesPlugin,
        AssetPlugin {
            file_path: asset_root(),
            ..default()
        },
        bevy::scene::ScenePlugin,
        InputPlugin,
        voxelith_abilities::plugin(),
        // 资产管线（集合 + 状态机 + `ContentSource::Assets`）
        CombatAssetsPlugin,
        // 内容装配（来源已经被上一个插件切成资产，所以磁盘那条不会跑）
        CombatContentPlugin,
    ));
    app
}

#[test]
fn the_asset_pipeline_carries_the_content() {
    let mut app = test_app();

    // 资产是异步加载的：驱动若干帧，直到内容装起来（或超时）。
    let mut loaded = false;
    for _ in 0..900 {
        app.update();
        if app
            .world()
            .get_resource::<CombatContent>()
            .is_some_and(|content| !content.abilities.is_empty())
        {
            loaded = true;
            break;
        }
    }

    assert!(
        loaded,
        "资产管线该把内容装起来（集合 → `ContentLoad::Ready` → 从 `Assets<RonSource>` 读文本）"
    );
    // 内容是在"集合出现"那一帧读到的，而状态机翻到 `Ready` 走的是 `StateTransition`
    // （可能在同帧的更早/更晚），所以再走两帧让状态落定。
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(
        *app.world().resource::<State<ContentLoad>>().get(),
        ContentLoad::Ready,
        "集合加载完状态机该翻到 `Ready`"
    );

    // 内容齐了，演示对局也该生成了。
    let playable = app
        .world_mut()
        .query_filtered::<Entity, With<Playable>>()
        .iter(app.world())
        .count();
    assert_eq!(
        playable, 1,
        "内容就绪之后该生成一个可玩角色（且只生成一个）"
    );

    // 走资产这条路读出来的内容，与磁盘那条是同一份：角色目录里有 player / goblin。
    let content = app.world().resource::<CombatContent>();
    assert!(content.actors.get("player").is_some());
    assert!(content.actors.get("goblin").is_some());
}
