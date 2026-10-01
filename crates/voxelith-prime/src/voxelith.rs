//! L2 顶层装配：`VoxelithPlugin`。
//!
//! 按 **R42/R43**，`main.rs` 只注册**顶层**插件；逻辑层与调试层的具体装配收在本插件内部，
//! 子插件不暴露给 `main.rs`。
//!
//! 装配顺序（**内容先于机制**）：
//!
//! ```text
//! CombatConfig（引擎参数）→ axom 机制（ActorPlugin + CombatPlugin）
//!   → ContentPlugin（启动时读 RON、注入词汇表 / 目录、生成 PC 与怪物）
//!   → PresentationPlugin（只读相位 / 可用技能 / 战斗日志）
//!   → debug（BRP + egui 检查器）
//! ```

use bevy::prelude::*;
use voxelith_axiom::behaviors::combat::{CombatConfig, CombatPlugin};

use crate::actor_render::ActorRenderPlugin;
use crate::content::ContentPlugin;
use crate::debug::{self, DEFAULT_BRP_PORT};
use crate::presentation::PresentationPlugin;
use crate::voxel_render::VoxelRenderPlugin;

/// Voxelith 的顶层插件（**R41.3** 对外发布的组装入口）。
pub struct VoxelithPlugin {
    /// BRP 监听端口，供 AI / 脚本连接。
    pub brp_port: u16,
    /// 战斗随机种子（同种子 = 同结果）。
    pub rng_seed: u64,
}

impl VoxelithPlugin {
    /// 用默认端口与默认种子构造。
    pub fn new() -> Self {
        Self {
            brp_port: DEFAULT_BRP_PORT,
            rng_seed: CombatConfig::default().rng_seed,
        }
    }

    /// 覆盖 BRP 端口。
    pub fn with_brp_port(mut self, port: u16) -> Self {
        self.brp_port = port;
        self
    }

    /// 覆盖随机种子（复现某一场战斗用）。
    pub fn with_rng_seed(mut self, seed: u64) -> Self {
        self.rng_seed = seed;
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
        // ---- 引擎级配置（内容层注入；数值内容都在 RON 里）----
        app.insert_resource(CombatConfig::new().with_seed(self.rng_seed));

        // ---- L1：核心机制（子插件嵌套，不外泄给 main.rs，R42）----
        // `CombatPlugin` 内部装配 L0 角色域 + 相位 / 状态 / 时间 / 行动各域，
        // 并把跨域系统顺序固定下来（见 `behaviors::combat` 的模块文档）。
        app.add_plugins(CombatPlugin);

        // ---- L2：内容与表现 ----
        // 顺序是契约：内容先注入（图集与地形都要读 world.ron），再建体素表现。
        // 角色表现（纸片人）排在体素表现之后：它要读内容里的角色表。
        app.add_plugins((
            ContentPlugin,
            VoxelRenderPlugin,
            ActorRenderPlugin,
            PresentationPlugin,
        ));

        // ---- L2 调试设施 ----
        // 需在 `DefaultPlugins` 之后：BRP 与 egui 检查器都依赖它提供的
        // 窗口 / 输入消息 / 渲染。
        debug::install(app, self.brp_port);
    }
}
