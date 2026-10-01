//! # voxelith-prime — 游戏内容与表现层（L2）
//!
//! 本 crate 只做**表现、内容与驱动**。硬约束（违反即架构守卫失败）：
//!
//! - **R15** 只读 L0/L1 数据，通过事件 / 消息监听变化。
//! - **R16** 禁止直接修改 `Resources` 里的池等核心数据。
//! - **R17** 禁止写战斗公式、伤害计算。
//! - **R18** 允许使用完整 Bevy、渲染组件、UI。
//! - **R4**  依赖方向单向：`voxelith-prime → voxelith-axiom`，禁止反向。
//!
//! 内容加载（读 `.ron` + 反序列化）在 [`content`]，**只在 L2**：
//! `axiom` 只提供描述结构与"字符串 → 词汇 ID"的解析（**R101 精神**）。
//!
//! 调试设施（BRP / egui 检查器）只允许出现在本 crate，禁止下沉到 `axiom`（R5、R99）。
//! 详见 `docs/layers.md`、`docs/combat-design.md`、`docs/debugging.md`。

pub mod actor_render;
pub mod content;
pub mod debug;
pub mod presentation;
pub mod save;
pub mod ui_theme;
pub mod voxel_render;
pub mod voxelith;

pub use voxelith::VoxelithPlugin;
