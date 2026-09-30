//! # voxelith-axiom — 核心机制层（L0 `atoms` + L1 `behaviors`）
//!
//! 本 crate 承载与表现无关的纯机制。硬约束（违反即编译失败或架构守卫失败）：
//!
//! - **R5** 只允许依赖 `bevy_ecs` / `bevy_app` / `bevy_reflect` / `bevy_time`，禁止完整 `bevy`。
//! - **R91** 依赖树中不得出现 `bevy_render` / `bevy_ui` / `bevy_sprite` / `bevy_pbr`。
//! - **R92** 不得依赖 `voxelith-prime`（依赖方向单向：prime → axiom）。
//! - **R8** L0 组件只依赖自己，系统只查询自己，不跨组件查询。
//! - **R9** L0 允许发出事件，事件只含数据，不含渲染句柄。
//! - **R10** L0 禁止出现 `Sprite` / `Text` / `Mesh` / `Transform` / `Handle<Image>`。
//! - **R13** L1 修改 L0 数据必须通过事件，不直接改。
//! - **R14** L1 禁止生成渲染实体（`SpriteBundle` 等），禁止依赖渲染 crate。
//!
//! 详见 `docs/architecture.md` 与 `docs/layers.md`。

pub mod atoms;
pub mod behaviors;
