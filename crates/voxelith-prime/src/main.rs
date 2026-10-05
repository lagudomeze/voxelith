#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! # voxelith-prime — 游戏内容与表现层（L2）入口
//!
//! 硬约束（违反即架构守卫失败）：
//!
//! - **R15** 只读 L0/L1 数据，通过事件监听变化。
//! - **R16** 禁止直接修改 `Health` / `Velocity` 等核心数据。
//! - **R17** 禁止写战斗公式、伤害计算。
//! - **R18** 允许使用完整 Bevy、渲染组件、UI。
//! - **R4**  依赖方向单向：`voxelith-prime → voxelith-axiom`，禁止反向。
//! - **R43** `main.rs` 只注册顶层插件，业务装配在 [`voxelith_prime::VoxelithPlugin`]。
//!
//! 调试通道（BRP、egui 检查器）见 `docs/debugging.md`。

use bevy::{log::LogPlugin, prelude::*};
use voxelith_prime::VoxelithPlugin;

fn main() {
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(LogPlugin {
                    filter: "icu_provider=error".to_string(),
                    ..default()
                })
                // **资产根指向工作区根的 `assets/`。**
                //
                // 为什么必须显式指定：Bevy 的默认资产根是
                // **可执行文件旁边的 `assets/`**（`target/debug/assets/`），
                // 不是工作目录。直接 `cargo run` 时会报
                // `Path not found: ...\target\debug\assets\sprites/hero.png`
                // —— 而素材明明在工作区根（踩过）。
                //
                // 用 `CARGO_MANIFEST_DIR` 往上退两级（`crates/voxelith-prime` → 工作区根）：
                // 编译期就定死，运行期不用管当前目录在哪。
                //
                // **发布时**：仍然要把 `assets/` 拷到 exe 旁边，
                // 或者把这里换成"exe 旁边的 assets，找不到再退回源码树"。
                // 现在只有开发路径，所以先只处理开发。
                .set(bevy::asset::AssetPlugin {
                    file_path: concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets").to_string(),
                    // 配置热重载只在开发构建里开：它会给运行时挂一个文件监视器，
                    // 出货版本不需要（`bevy/file_watcher` 特性始终编进去，这里只决定开不开）。
                    watch_for_changes_override: Some(cfg!(debug_assertions)),
                    ..default()
                }),
        )
        // **六份静态配置不再在 `main` 里解析**：`ContentPlugin` 用 `ContentManifest`
        // （`SceneComponent`）在 `PreStartup` 里装载，路径全写在清单的 `scene()` 里。
        .add_plugins(VoxelithPlugin::new())
        .run();
}
