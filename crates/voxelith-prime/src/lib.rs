//! # voxelith-prime — 游戏内容与表现层（L2）
//!
//! 本 crate 只做表现、内容与驱动。硬约束（违反即架构守卫失败）：
//!
//! - **R15** 只读 L0/L1 数据，通过事件监听变化。
//! - **R16** 禁止直接修改 `Health` / `Velocity` 等核心数据。
//! - **R17** 禁止写战斗公式、伤害计算。
//! - **R18** 允许使用完整 Bevy、渲染组件、UI。
//! - **R4**  依赖方向单向：`voxelith-prime → voxelith-axiom`，禁止反向。
//!
//! 调试设施（BRP / MCP / egui 检查器）只允许出现在本 crate，禁止下沉到 `axiom`（R5、R99）。
//! 详见 `docs/layers.md`、`docs/debugging.md`。

pub mod debug;
pub mod voxelith;

pub use voxelith::VoxelithPlugin;
