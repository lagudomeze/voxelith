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
//!
//! # 两套管线并存：**旧管线保留作迁移源**（目标明确要求保留）
//!
//! 新栈（diesel + gauge + gearbox）已经覆盖战斗域 / 数值层 / L2，旧引擎仍在跑，
//! 两边**各读各的**（新面板读新栈，旧 HUD 读旧引擎）。保留哪些、为什么、什么时候能退：
//!
//! | 保留的东西 | 为什么还留着 | 退役条件 |
//! |---|---|---|
//! | `voxelith-axiom`（旧引擎：相位 / 行动 / 决策 / 威胁 / 旧状态） | 它是**迁移源**：新栈的每条语义都对着它抄过；旧测试还钉着它的行为 | 新栈补齐"决策 / AI / 阵营"之后 |
//! | `assets/data/skills.ron`（108 KB）、`statuses.ron`（35 KB） | 未迁完的内容（96 条技能 + 大量状态），是新的 `abilities.ron` / `status_defs.ron` 的取材源 | 内容全部迁完 |
//! | `content/`（旧内容管线：词汇表 / 目录 / 生成） | 旧实体（PC / 怪物 / 地形）由它生成 | 角色与地形定义迁到新栈 |
//! | `presentation/hud`（旧 HUD） | 读旧引擎的池 / 相位 / 可用技能；新面板与它并存 | 旧引擎退役时一起走 |
//! | `save.rs`（手写 RON 体素存档） | 它存的是**地形编辑**，与战斗存档（`combat_save`）是两件事 | 不需要退——两者职责不同 |
//!
//! # 遗留清单（诚实版）
//!
//! | 项 | 状态 |
//! |---|---|
//! | 读档后的"按模板重建角色" | ⬜ 只存了快照组件；moonshine 读档会重建实体（见 `combat_save` 的模块文档） |
//! | 层数缩放的**单实例计数器**模型 | ⬜ 现在选的是"N 个实例各算一次"（见 `TickRon::amount_expr`） |
//! | 表现层对三个库的实际使用（hanabi 粒子 / kira 音频 / tweening 补间） | ⬜ 只接线未使用 |
//! | 物理（avian3d）参与战斗 | ⬜ 只接线未使用 |
//! | 两份 `bevy_egui`（inspector 0.40 vs 工作区 0.42） | ⚠️ 已按特性二选一避开运行时冲突，版本对齐后可删 `cfg` |
//! | 战斗面板搬进 egui | ⬜ 现在是整块文本（先能看见） |
//! ```

use bevy::prelude::*;
use voxelith_axiom::behaviors::combat::{CombatConfig, CombatPlugin};

use crate::actor_render::ActorRenderPlugin;
use crate::content::ContentPlugin;
use crate::debug::{self, DEFAULT_BRP_PORT};
use crate::ecosystem::EcosystemPlugin;
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

        // ---- L2：生态接线（8 个库 + 中间层）----
        // 排在内容与表现**之前**：它们提供输入 / 资产加载状态机 / 属性图，
        // 后面的内容与战斗表现都要用（属性图的全局 interner 也在这里初始化）。
        app.add_plugins(EcosystemPlugin);

        // ---- L2：新战斗内容管线（读 .ron → 目录 → 模板 → 可玩角色 → 输入）----
        // 排在生态之后（要 `TemplateRegistry` / 属性图 / 输入动作），
        // 排在旧内容与表现之前（它生成的是新栈的实体）。
        app.add_plugins(crate::combat::CombatContentPlugin);

        // ---- L2：战斗存档（moonshine 的 `Save` / `Load` 观察者已在生态里装好）----
        app.add_plugins(crate::combat_save::CombatSavePlugin);

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
