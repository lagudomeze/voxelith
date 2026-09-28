//! L2 顶层装配：`VoxelithPlugin`。
//!
//! 按 **R42/R43**，`main.rs` 只注册**顶层**插件；逻辑层与调试层的具体装配收在本插件内部，
//! 子插件不暴露给 `main.rs`。

use bevy::prelude::*;
use voxelith_axiom::atoms::HealthPlugin;

use crate::debug::{self, DEFAULT_BRP_PORT};

/// Voxelith 的顶层插件（**R41.3** 对外发布的组装入口）。
pub struct VoxelithPlugin {
    /// BRP 监听端口，供 AI / 脚本连接。
    pub brp_port: u16,
}

impl VoxelithPlugin {
    /// 用默认端口构造。
    pub fn new() -> Self {
        Self {
            brp_port: DEFAULT_BRP_PORT,
        }
    }

    /// 覆盖 BRP 端口。
    pub fn with_brp_port(mut self, port: u16) -> Self {
        self.brp_port = port;
        self
    }
}

impl Default for VoxelithPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for VoxelithPlugin {
    fn build(&self, app: &mut App) {
        // ---- L0 / L1：核心机制（子插件嵌套，不外泄给 main.rs，R42）----
        app.add_plugins(HealthPlugin);

        // ---- L2 调试设施 ----
        // 需在 `DefaultPlugins` 之后：BRP 与 egui 检查器都依赖它提供的
        // 窗口 / 输入消息 / 渲染。
        debug::install(app, self.brp_port);

        // 骨架阶段的演示实体：让调试通道一启动就有组件可查。
        app.add_systems(Startup, spawn_debug_sample);
    }
}

/// 生成一个带 [`voxelith_axiom::atoms::Health`] 的演示实体。
fn spawn_debug_sample(mut commands: Commands) {
    commands.spawn((
        Name::new("debug-sample"),
        voxelith_axiom::atoms::Health::new(100),
    ));
    info!("已生成调试演示实体 `debug-sample`（Health 100/100）");
}
