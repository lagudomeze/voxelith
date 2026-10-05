//! # voxelith-axiom — 核心机制层（L0 `atoms` + L1 `behaviors`）
//!
//! 本 crate 承载与表现无关的纯机制。硬约束（违反即编译失败或架构守卫失败）：
//!
//! - **R5** 只允许依赖 `bevy_ecs` / `bevy_app` / `bevy_reflect` / `bevy_time` / `bevy_state`
//!   （后两者见 Q21 / Q26），禁止完整 `bevy`。
//! - **R91** 依赖树中不得出现 `bevy_render` / `bevy_ui` / `bevy_sprite` / `bevy_pbr`。
//! - **R92** 不得依赖 `voxelith-prime`（依赖方向单向：prime → axiom）。
//! - **R8** L0 组件只依赖自己，系统只查询自己，不跨组件查询。
//! - **R9** L0 允许发出事件，事件只含数据，不含渲染句柄。
//! - **R10** L0 禁止出现 `Sprite` / `Text` / `Mesh` / `Transform` / `Handle<Image>`。
//! - **R13** L1 修改 L0 数据必须通过事件 / 效果执行器，不直接改。
//! - **R14** L1 禁止生成渲染实体（`SpriteBundle` 等），禁止依赖渲染 crate。
//!
//! **顶层只有两个模块，名字就是层名**：[`atoms`]（L0）与 [`behaviors`]（L1）。
//! 领域（角色 / 体素世界 / 行动 / 状态……）住在层**里面**（R115：按功能组织）。
//!
//! 详见 `docs/architecture.md`、`docs/layers.md` 与 `docs/combat-design.md`。

pub mod atoms;
pub mod behaviors;

/// L0 的体素世界域**住在 [`atoms`] 下**——它一行系统都没有，是纯数据 + 纯计算。
///
/// 这里把它转出，是为了让 `voxelith_axiom::world::…` 这个公开路径照旧可用
/// （`prime` 的渲染层、存档、调试通道都在用）。
///
/// ⚠️ **BRP 的组件路径跟着「定义所在模块」走，不跟转出走**：所以
/// `ChunkPos` / `VoxelStore` 这类组件在 BRP 查询里要用
/// `voxelith_axiom::atoms::world::…`（踩过一次，见 `docs/bevy-queries.md`）。
pub use atoms::world;
