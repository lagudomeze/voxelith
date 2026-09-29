//! L2 顶层装配：`VoxelithPlugin`。
//!
//! 按 **R42/R43**，`main.rs` 只注册**顶层**插件；逻辑层与调试层的具体装配收在本插件内部，
//! 子插件不暴露给 `main.rs`。

use bevy::prelude::*;
use voxelith_axiom::atoms::HealthPlugin;
use voxelith_axiom::atoms::attribute::{AttributeId, AttributePlugin, AttributeValues, Attributes};
use voxelith_axiom::behaviors::attributes::{AttributeBehaviorPlugin, AttributeModifiers};

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
        // L0 属性负责"存值 + 唯一写入口"，L1 属性负责"基础值 + 修饰符 → 最终值"（R112）。
        app.add_plugins((HealthPlugin, AttributePlugin, AttributeBehaviorPlugin));

        // ---- L2 调试设施 ----
        // 需在 `DefaultPlugins` 之后：BRP 与 egui 检查器都依赖它提供的
        // 窗口 / 输入消息 / 渲染。
        debug::install(app, self.brp_port);

        // 骨架阶段的演示实体：让调试通道一启动就有组件可查。
        app.add_systems(Startup, spawn_debug_sample);
    }
}

/// 生成一个带 [`voxelith_axiom::atoms::Health`] 与属性组件的演示实体。
///
/// 属性组件成对出现：`Attributes`（L0 数值）负责唯一写入口，
/// `AttributeModifiers`（L1 槽位）负责接收装备 / 被动 / 状态派生的修饰符。
fn spawn_debug_sample(mut commands: Commands) {
    let mut base = AttributeValues::default();
    base.set(AttributeId::Strength, 10.0);
    base.set(AttributeId::Dexterity, 8.0);
    base.set(AttributeId::Constitution, 12.0);

    commands.spawn((
        Name::new("debug-sample"),
        voxelith_axiom::atoms::Health::new(100),
        Attributes::new(base, 5.0),
        AttributeModifiers::default(),
    ));
    info!(
        "已生成调试演示实体 `debug-sample`（Health 100/100，力量 10 / 敏捷 8 / 体质 12，未分配 5）"
    );
}
