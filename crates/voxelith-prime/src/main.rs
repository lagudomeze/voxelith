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

use bevy::prelude::*;
use voxelith_prime::VoxelithPlugin;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(VoxelithPlugin::new())
        .run();
}
