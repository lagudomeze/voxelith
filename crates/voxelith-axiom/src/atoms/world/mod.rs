//! 体素**世界数据**：地形、坐标、区块存储、射线检测。
//!
//! ## 定位（**R74–R77、R101**）
//!
//! 本模块是**纯数据 + 纯计算**，属于 L0/L1：
//!
//! | 可以做 | 绝对不可以 |
//! |---|---|
//! | 存体素、程序化生成、`get_voxel` / `set_voxel`、DDA 射线、发 `ChunkDirtyMessage` | 加载纹理、碰 `AssetServer` / `Mesh` / `Transform`（R75、R103） |
//!
//! 网格化、图集、材质、相机全部在 L2 的 `voxel_render`。渲染层**不许自己算世界坐标**
//! （R107）——坐标换算统一走 [`VoxelPos::to_world`] 这类函数。
//!
//! ## 可配置（本目标的"注意配置化"）
//!
//! 地形形状、噪声、区块尺寸、世界高度全是数据：见 [`WorldConfig`]（引擎级，Rust）
//! 与 L2 的 `assets/data/world.ron`（数值内容）。改地形**不用改代码**。
//!
//! ## 两层存储（R65、R66）
//!
//! | 层 | 内容 | 存储 |
//! |---|---|---|
//! | 程序化层 | 噪声生成的**初始**世界 | 无存储（确定性函数，按需算） |
//! | 持久化层 | 改过的体素 | `HashMap<ChunkPos, Chunk>`（只留与生成结果不同的格） |
//!
//! 查询顺序：**先已加载的区块，未命中回退程序化函数**（R66）。

mod chunk;
mod generate;
mod raycast;
mod store;
mod voxel;

pub use chunk::{CHUNK_SIZE, CHUNK_VOLUME, Chunk, ChunkPos};
pub use generate::{BlockDef, FaceShade, FaceTexture, NoiseParams, TerrainParams, TexturePattern};
pub use raycast::{VoxelHit, raycast_voxels};
pub use store::{ChunkDirtyMessage, VoxelStore, load_around};
pub use voxel::{Voxel, VoxelAppearance, VoxelId, VoxelPalette};

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;

use crate::atoms::vocabulary::{NameTable, UnknownName};

/// 世界配置（**Resource**）：地形与存储的引擎级参数。
///
/// 数值内容（各种方块、生成参数）放 `.ron`；这里是"C 结构本身"的参数。
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct WorldConfig {
    /// 地形生成参数。
    pub terrain: TerrainParams,
    /// 同时保留多少个区块（超出按**最近最久未用**丢弃；`0` = 不限制）。
    ///
    /// 战场规模下"全部常驻"通常更省事，所以默认不限制。
    pub max_loaded_chunks: usize,
}

impl Default for WorldConfig {
    fn default() -> Self {
        Self {
            terrain: TerrainParams::default(),
            max_loaded_chunks: 0,
        }
    }
}

/// 方块名字表（**注册制**，与内容层的 `Vocab` 同一套路）。
///
/// 名字只在加载期出现，运行时只跑 `u16` ID。
///
/// ## ⚠️ 方块 ID 从 1 开始，`0` 永久保留给空气
///
/// [`Voxel::AIR`](super::Voxel::AIR) 的判据是 `id == 0`，所以**空气必须占住 0 号**，
/// 任何一个真方块都不许拿到 0。
///
/// 底层 [`NameTable::register`] 是 **0-based**（第一个名字返回 0），
/// 所以这里**统一加一**再交给调用方。
///
/// 这里踩过一次很隐蔽的坑：直接把 `NameTable` 的 0-based 号当 ID 用，
/// 于是 `world.ron` 里第一个方块（`grass`）拿到了 **ID 0 = 空气**——
/// 地表方块全被判成空气，**地形一个可见面都画不出来**。
/// 更阴的是：它不报错、`palette` 也"看起来对"，只有几何悄悄变成空。
///
/// 教训：**不要把"保留值"的约束交给内容作者去记**。
/// "空气不占号"是引擎的契约，就该由引擎强制（`+ 1` 这一行），
/// 而不是写成文档让人别踩。
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct VoxelNames {
    /// 名字 → ID（ID 已含 `+ 1` 偏移，即真方块的 ID 是 `1..=n`）。
    pub ids: NameTable,
    /// ID → 名字（**下标 = ID - 1**）。
    pub names: Vec<String>,
}

impl VoxelNames {
    /// 登记一个方块名，返回它的 ID（从 `1` 开始；重复登记返回已存在的）。
    ///
    /// `1` 号留给"空气之外"本身不占位——空气没有名字，它的 ID `0` 是约定的保留值。
    pub fn register(&mut self, name: &str) -> VoxelId {
        if let Some(existing) = self.ids.id(name) {
            return VoxelId(existing + 1);
        }
        let zero_based = self.ids.register(name);
        self.names.push(name.to_owned());
        debug_assert_eq!(zero_based as usize, self.names.len() - 1);
        // **`+ 1`：把 0 号让给空气。**
        VoxelId(zero_based + 1)
    }

    /// 名字 → ID。
    pub fn id(&self, name: &str) -> Result<VoxelId, UnknownName> {
        self.ids
            .id(name)
            .map(|zero_based| VoxelId(zero_based + 1))
            .ok_or_else(|| UnknownName::new("voxel", name))
    }

    /// ID → 名字（空气/没登记过给个占位，不 panic：日志不该因为内容漏配而崩）。
    pub fn name(&self, id: VoxelId) -> &str {
        if id == VoxelId(0) {
            return "air";
        }
        match self.names.get(id.0 as usize - 1) {
            Some(name) => name,
            None => "?",
        }
    }

    /// 登记了几个方块（不含空气）。
    pub fn len(&self) -> usize {
        self.names.len()
    }

    /// 空表？
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
}

/// 注册体素世界数据层。
pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WorldConfig>()
            .init_resource::<VoxelStore>()
            .init_resource::<VoxelNames>()
            .init_resource::<VoxelPalette>()
            .add_message::<ChunkDirtyMessage>();
    }
}
