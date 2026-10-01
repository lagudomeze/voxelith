//! L2 体素与战场表现：**网格化 + 图集 + 俯视相机**（**R71–R73、R84**）。
//!
//! ## 与数据层的分工（**R74–R77、R107**）
//!
//! ```text
//! world（L0/L1，数据）  ──ChunkDirtyMessage──►  voxel_render（L2，表现）
//!   存体素 / 程序化生成                            贪婪网格化 / 图集 / 材质 / 相机
//!   不知道 Mesh / Transform                        不许自己发明位置
//! ```
//!
//! 本模块**只读** `VoxelStore`：位置全部经 `ChunkPos::origin`（`world` 提供），
//! 体素全部经 `VoxelStore::get` + 程序化回退。
//!
//! ## 子模块
//!
//! | 模块 | 干什么 |
//! |---|---|
//! | [`atlas`] | 程序化生成方块贴图集（配置驱动，不需要美术资源） |
//! | [`mesher`] | 贪婪网格化（纯计算，可异步） |
//! | [`materials`] | 地形材质与光照 |
//! | [`terrain`] | 区块 → `Mesh` 实体，监听 `ChunkDirtyMessage` |
//! | [`camera`] | 俯视角战场相机（可配置俯角 / 距离 / 视野） |

pub mod async_mesh;
mod atlas;
pub mod camera;
mod floor_grid;
mod gltf_material;
mod ground_style;
pub mod materials;
pub mod mesher;
mod model_gallery;
mod model_offset;
mod orbit_camera;
pub mod shade;
pub mod terrain;
mod voxel_edit;
mod world_axes;

use bevy::prelude::*;

pub use atlas::{AtlasImage, BlockFace, BlockTexture, FaceTexture, Pattern};
pub use camera::{BattlefieldCamera, CameraConfig, CameraFollow};
pub use materials::VoxelMaterial;
pub use terrain::{ChunkMeshEntity, ChunkMeshIndex, TerrainLoadRadius};

/// 体素表现的**系统阶段**，用来固定"图集 → 材质 → 网格"的顺序。
///
/// 顺序在这里是**契约**：网格化要查图集（哪一格是哪个面），材质要引用图集。
/// 用 `SystemSet` 而不是 `.after(某个函数)` 是因为函数签名会变，集合名不会。
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VoxelRenderSet {
    /// 建图集。
    Atlas,
    /// 建材质与光照。
    Material,
    /// 建 / 重建区块网格。
    Build,
}

/// 注册全部体素表现。
pub struct VoxelRenderPlugin;

impl Plugin for VoxelRenderPlugin {
    fn build(&self, app: &mut App) {
        // **一条显式链**，不是几个并列的 `Startup`：后面的系统读前面的产物
        // （材质读图集、地形读材质与地形参数），而 `Commands` 要到系统之间才落地——
        // 并列注册会让"读不到资源"变成运行期 panic。
        //
        // 五步：图集 → 内容资源 → 材质 → 地形参数 → 地形网格。
        // `setup_content_resources` 夹在图集之后，是因为它产的 `ContentData` 是
        // 图集与地形都要读的输入；`crate::content::ContentPlugin` 只负责生成角色。
        app.add_systems(
            Startup,
            (
                atlas::build_atlas,
                materials::build_material,
                materials::install_terrain_config,
                terrain::build_initial_terrain,
            )
                .chain()
                // 排在内容翻译之后：图集要读方块表、地形要读地形参数。
                .after(crate::content::ContentSet::Translate),
        )
        .add_systems(
            Startup,
            (
                materials::spawn_terrain_light,
                camera::spawn_battlefield_camera,
            ),
        )
        .add_systems(Update, terrain::rebuild_dirty_chunks)
        // **收异步网格化的结果**：`rebuild_dirty_chunks` 与开局的
        // `build_initial_terrain` 都只派发任务，算完由它登记 `Mesh3d`。
        // 必须每帧跑 —— 它是"后台算完 → 上屏"的唯一通道。
        .add_systems(Update, async_mesh::collect)
        // 体素编辑：鼠标左键挖、右键放。改完发 `ChunkDirtyMessage`，
        // 由 `rebuild_dirty_chunks` 重新网格化 —— 整条链路都是既有设施。
        .add_plugins(voxel_edit::VoxelEditPlugin)
        // **模型展示台**：把 Kenney 的 GLB 方块一个一个摆出来（素材浏览器）。
        // 默认**关闭**；用环境变量 MODEL_GALLERY=1 打开。
        .add_plugins(model_gallery::ModelGalleryPlugin)
        // **地板网格**：用 GLB 瓦片铺一格一格的灰色地面（与体素地形二选一）。
        .add_plugins(ground_style::GroundStylePlugin)
        .add_plugins(floor_grid::FloorGridPlugin)
        // **glTF 材质修正**：Kenney 的 quad 是双面重叠三角形，会 z-fighting 到背光那一面。
        .add_plugins(gltf_material::GltfMaterialPlugin)
        // **坐标轴指示器**（`SHOW_AXES=1` 打开）：等距视图下 X/Z 极易看混。
        .add_plugins(world_axes::WorldAxesPlugin)
        // **可交互相机**（`ORBIT_CAMERA=1` 打开）：中键平移 / 滚轮缩放 / 右键转视角 / R 复位。
        .add_plugins(orbit_camera::OrbitCameraPlugin)
        // **模型展示台**：把 Kenney 的 GLB 方块一个一个摆出来（素材浏览器）。
        // 默认**关闭**；用环境变量 `MODEL_GALLERY=1` 打开，
        // 这样"看素材"不用改代码、也不用改默认行为。
        .add_systems(Update, camera::follow_target)
        // 前几帧打印相机实测值（`Startup` 的 spawn 此时已落地）。
        // 用帧计数而不是 `on_timer`：虚拟时间会被战斗系统冻结，计时器不触发。
        .add_systems(Update, camera::log_cameras)
        // `CameraConfig` 的**唯一来源就在这里**：带环境变量覆盖的默认值。
        // 不放到 `voxelith.rs`：那里插入的时机要靠插件顺序保证，
        // 而"取景参数"和"用它的系统"本就该注册在同一处。
        .init_resource::<camera::CameraConfig>()
        .init_resource::<orbit_camera::OrbitCamera>()
        .init_resource::<camera::DiagnosticTick>()
        .init_resource::<terrain::TerrainLoadRadius>()
        .init_resource::<terrain::ChunkMeshIndex>();
    }
}

#[cfg(test)]
mod terrain_gap_tests;
