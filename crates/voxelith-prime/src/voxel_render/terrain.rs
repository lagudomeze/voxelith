//! 地形渲染：监听 `ChunkDirtyMessage`，把区块网格化成 `Mesh` 实体（**R70–R73**）。
//!
//! ## 职责边界
//!
//! | 做 | 不做 |
//! |---|---|
//! | 读 `VoxelStore` + `VoxelPalette`，贪婪网格化，建 `Mesh` 实体 | 自己发明位置（**R107**：位置全部来自 `ChunkPos::origin`）（**R76、R88**） |
//!
//! 位置与体素全部来自数据层；本模块只把"体素 → 三角形"。
//!
//! ## 为什么"每个区块一个实体"
//!
//! 区块是天然的分块单位：改一格只重建它所属的区块（`ChunkDirtyMessage` 正好带这个粒度）。
//! 合并成一个大网格会让"改一格"变成重建全世界。
//!
//! ## 邻居查询为什么要跨区块
//!
//! 区块边界上的格子，其"外面那一面"取决于**邻区块**的体素。只查自己会让接缝处
//! 出现双面（两边的墙都贴着）。所以 `sample` 走 `VoxelStore::get`（跨区块）+ 程序化回退。

use std::collections::HashMap;

use bevy::platform::collections::HashSet;
use bevy::prelude::*;
use voxelith_axiom::world::{
    ChunkDirtyMessage, ChunkPos, VoxelPalette, VoxelStore, WorldConfig, load_around,
};

use super::VoxelRenderSet;
use super::atlas::AtlasImage;

/// 一个区块的渲染实体。
#[derive(Component, Debug, Clone, Copy, Reflect)]
#[reflect(Component)]
pub struct ChunkMeshEntity {
    /// 对应哪个区块。
    pub chunk: ChunkPos,
}

/// 开局加载的区域半径（**区块**数）。
///
/// **为什么是 3 而不是 1**：相机是**斜 45°** 的，斜率下它沿视线能看到很远的 Z。
/// 半径 1（3×3 = 96×96 格）会让视野边缘越过地形尽头、直接看到清屏色
/// ——表现就是画面上出现一片蓝灰的"空洞"。
/// 边长必须**显著大于"视野能看到的距离"**，否则斜视必然看到边界。
///
/// 这是**一次性加载**，不是每帧流式加载（目标是"俯视角看一片战场"，不是无限世界）。
/// 真要更大的世界时，这里换成"跟随相机按需加载"即可。
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Reflect)]
pub struct TerrainLoadRadius {
    /// 区块半径。
    pub chunks: i32,
}

impl Default for TerrainLoadRadius {
    fn default() -> Self {
        // 战场规模：5×5 = 160×160 格。
        // 取 2 而不是 1 是因为**相机是斜 45° 的**：它沿视线方向能看到很远的 Z，
        // 3×3（96×96）会让视野边缘露出地形的侧面/内部（表现为一片蓝灰）。
        Self { chunks: 3 }
    }
}

/// 区块 → 网格实体（重建时替换，不重复 spawn）。
///
/// ## 为什么带一个"可反射的快照"字段
///
/// `entities` 是**私有** `HashMap<ChunkPos, Entity>`。Bevy 的 `Reflect` 只反射
/// 公开字段与标注过的字段，私有的不会被序列化 —— 于是 BRP 查这个资源只会看到
/// `{}`，**查了等于没查**（而这不报错）。
///
/// `loaded` 是给调试通道看的：**已建好网格的区块坐标**。有了它，
/// "画面上某个位置该不该有地形" 就能在运行期直接问，不必靠截图猜。
#[derive(Resource, Debug, Default, Reflect)]
#[reflect(Resource)]
pub struct ChunkMeshIndex {
    /// 区块 → 实体（内部用）。
    #[reflect(ignore)]
    entities: HashMap<ChunkPos, Entity>,
    /// **已建好网格的区块坐标**（调试通道用；与 `entities` 的键同步）。
    loaded: Vec<ChunkPos>,
}

impl ChunkMeshIndex {
    /// 这个区块的渲染实体。
    pub fn get(&self, chunk: ChunkPos) -> Option<Entity> {
        self.entities.get(&chunk).copied()
    }

    /// 记下某个区块的实体。
    pub fn insert(&mut self, chunk: ChunkPos, entity: Entity) {
        self.entities.insert(chunk, entity);
        if !self.loaded.contains(&chunk) {
            self.loaded.push(chunk);
        }
    }

    /// 移除某个区块的记录（重建时先清旧的）。
    pub fn remove(&mut self, chunk: ChunkPos) {
        self.entities.remove(&chunk);
        self.loaded.retain(|other| *other != chunk);
    }

    /// 已建好网格的区块坐标（调试通道读它）。
    pub fn loaded(&self) -> &[ChunkPos] {
        &self.loaded
    }

    /// 已建了多少区块。
    pub fn len(&self) -> usize {
        self.entities.len()
    }

    /// 空？
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }
}

/// 注册地形渲染。
pub struct TerrainPlugin;

impl Plugin for TerrainPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TerrainLoadRadius>()
            .init_resource::<ChunkMeshIndex>()
            // 必须在图集建好之后：网格化要查"哪一格是哪个面"。
            .add_systems(Startup, build_initial_terrain.in_set(VoxelRenderSet::Build))
            .add_systems(Update, rebuild_dirty_chunks.in_set(VoxelRenderSet::Build));
    }
}

/// 开局把战场范围内的区块加载并建好网格。
#[allow(clippy::too_many_arguments)]
pub fn build_initial_terrain(
    mut commands: Commands,
    config: Res<WorldConfig>,
    radius: Res<TerrainLoadRadius>,
    camera: Res<super::camera::CameraConfig>,
    mut store: ResMut<VoxelStore>,
    palette: Res<VoxelPalette>,
    atlas: Res<AtlasImage>,
    style: Res<super::ground_style::GroundStyle>,
    content: Option<Res<crate::content::ContentData>>,
) {
    // **两种地面二选一**：判据来自 `GroundStyle`（唯一决策点），不在这里读环境变量。
    //
    // 曾经两边各自读 `FLOOR_GRID`，而"默认值"的写法不一样，结果**两边都跑了**
    // —— 地板被绿色地形整个盖住。这类 bug 的可怕之处是两边代码单看都对。
    if style.is_tile_grid() {
        info!("地形：当前是 GLB 地板网格，跳过体素地形（两者互斥）");
        return;
    }

    // 地形参数**以内容为准**：`WorldConfig` 里可能是 `WorldPlugin` 的默认值
    // （`build_atlas` 排队改它的命令还没落地）。这样不依赖命令的落地时机。
    let terrain = content
        .as_ref()
        .map_or(config.terrain, |content| content.world.terrain);

    // **加载区域以相机注视点为中心**，而不是恒以世界原点为中心。
    //
    // 踩过的坑：等距相机的方位角一转（45°），**视野覆盖的世界范围就平移了**
    // （相机在 `+X+Z` 象限 ⇒ 屏幕中心落在 `-X-Z` 一侧）。若地形还以原点为中心加载，
    // 画面上就会出现成片"没有网格"的黑块 —— 看起来像渲染 bug，其实是
    // **取景与加载范围没对齐**。这条不变量要永远成立：**看得见的地方一定有地形**。
    let center = camera.center_chunk();
    let loaded = load_around(&mut store, &terrain, center, radius.chunks);
    info!(
        "地形：加载 {loaded} 个区块（半径 {}，中心 {:?}）",
        radius.chunks, center
    );

    let chunks: Vec<ChunkPos> = store.loaded().collect();
    let mut dispatched = 0usize;
    for chunk in chunks {
        // **异步派发**（R72）：网格化在 `AsyncComputeTaskPool` 里跑，
        // 主线程只起了几十个任务就返回 —— 启动不再一区块卡一帧。
        // 结果由 `async_mesh::collect` 在后续帧登记。
        if super::async_mesh::dispatch(&mut commands, &store, &terrain, &palette, &atlas, chunk)
            .is_some()
        {
            dispatched += 1;
        }
    }
    info!("地形：派发 {dispatched} 个区块网格任务（异步）");
}

/// 有区块变脏就重建它（**只重建脏的那个**）。
///
/// **网格化本身是异步的**（见 [`super::async_mesh`]）：这个系统只负责
/// "销毁旧网格 + 派发任务"，算完由 `async_mesh::collect` 接手。
/// 所以这里不再需要 `Assets<Mesh>` / `VoxelMaterial` —— 它们是收集阶段的事。
pub fn rebuild_dirty_chunks(
    mut commands: Commands,
    mut dirty: MessageReader<ChunkDirtyMessage>,
    index: ResMut<ChunkMeshIndex>,
    store: Res<VoxelStore>,
    config: Res<WorldConfig>,
    palette: Res<VoxelPalette>,
    atlas: Res<AtlasImage>,
    content: Option<Res<crate::content::ContentData>>,
) {
    // 与开局一致：地形参数以内容为准。
    let terrain = content
        .as_ref()
        .map_or(config.terrain, |content| content.world.terrain);
    let mut touched: HashSet<ChunkPos> = HashSet::new();
    for message in dirty.read() {
        touched.insert(message.chunk);
    }
    for chunk in touched {
        // 旧实体直接销毁：网格是"整块替换"，不做增量更新。
        //
        // **注意**：这里销毁的是**旧网格**。新网格走异步派发，
        // 中间会有一段时间该区块没有 `Mesh3d` —— 那是**可见的空窗**。
        // 之所以可以接受：改一个方块时，相邻区块的重建几乎同时完成，
        // 而且玩家看到的是"方块消失再出现"，不是"世界破了个洞"。
        // 真要做到零空窗，得保留旧网格到新网格就绪（双缓冲）——
        // 那要额外的每区块状态，等有实际需要再加。
        if let Some(old) = index.get(chunk) {
            commands.entity(old).despawn();
        }
        // 派发异步任务：网格化在计算线程池里跑，主线程不卡。
        // `dispatch` 返回 `None` 表示区块是空的（不建实体）。
        if let Some(entity) =
            super::async_mesh::dispatch(&mut commands, &store, &terrain, &palette, &atlas, chunk)
        {
            // **不在这里 `index.insert`**：网格还没算出来。
            // `async_mesh::collect` 算完才登记 —— 这样 `ChunkMeshIndex`
            // 里的条目**一定有 `Mesh3d`**，查询它的人不会拿到半成品。
            let _ = entity;
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy::prelude::*;
    use voxelith_axiom::world::{
        ChunkPos, TerrainParams, VoxelAppearance, VoxelPalette, load_around,
    };

    use super::super::atlas::{AtlasImage, BlockTexture, FaceTexture, Pattern};
    use super::super::mesher::greedy_mesh;
    use crate::content::parse_raw;
    use voxelith_axiom::world::VoxelStore;

    /// 从真的 `world.ron` 造出地形参数（**复刻游戏的配置来源**，不用假数据）。
    fn real_world_config() -> (TerrainParams, Vec<voxelith_axiom::world::BlockDef>) {
        let raw = parse_raw().expect("world.ron 该配好");
        // 名字 → ID 在加载期完成；这里走和 `resolve_world` 同一条路径。
        let mut names = voxelith_axiom::world::VoxelNames::default();
        for block in &raw.world.blocks {
            names.register(&block.id);
        }
        let t = &raw.world.terrain;
        let lookup = |name: &str| names.id(name).unwrap_or(voxelith_axiom::world::VoxelId(0));
        let terrain = TerrainParams {
            base_height: t.base_height,
            amplitude: t.amplitude,
            height: voxelith_axiom::world::NoiseParams {
                frequency: t.height.frequency,
                octaves: t.height.octaves,
                lacunarity: t.height.lacunarity,
                persistence: t.height.persistence,
                seed: t.height.seed,
            },
            soil_depth: t.soil_depth,
            surface: lookup(&t.surface),
            soil: lookup(&t.soil),
            deep: lookup(&t.deep),
            floor_y: t.floor_y,
        };
        let blocks = raw
            .world
            .blocks
            .iter()
            .map(|b| voxelith_axiom::world::BlockDef {
                id: b.id.clone(),
                side: voxelith_axiom::world::FaceTexture {
                    base: b.side.base,
                    pattern: voxelith_axiom::world::TexturePattern::Solid,
                },
                top: None,
                bottom: None,
                opaque: b.opaque,
            })
            .collect();
        (terrain, blocks)
    }

    #[test]
    fn the_real_world_config_produces_visible_terrain() {
        // **这条是"地形画不出来"的复刻测试**：用真 `world.ron` 的方块名 / 地形参数，
        // 加载 3×3 区块，逐个网格化。任何一个区块产出 0 个可见面就说明地表不可见。
        let (terrain, blocks) = real_world_config();
        assert!(!blocks.is_empty(), "`world.ron` 该有方块");
        assert_ne!(
            terrain.surface,
            voxelith_axiom::world::VoxelId(0),
            "地表方块不能是空气（ID 0）：`world.ron` 里 `surface` 名字没解析出来"
        );

        let mut palette = VoxelPalette::default();
        for block in &blocks {
            palette.push(VoxelAppearance {
                atlas_index: 0,
                opaque: block.opaque,
            });
        }
        let mut images = Assets::<Image>::default();
        let textures: Vec<BlockTexture> = blocks
            .iter()
            .map(|b| BlockTexture {
                side: FaceTexture {
                    base: b.side.base,
                    pattern: Pattern::Solid,
                },
                top: None,
                bottom: None,
                opaque: b.opaque,
            })
            .collect();
        let atlas = AtlasImage::build(&mut images, &textures);

        let mut store = VoxelStore::default();
        let loaded = load_around(&mut store, &terrain, ChunkPos::default(), 1);
        assert_eq!(loaded, 9, "开局该加载 3×3");

        let mut with_faces = 0;
        let mut total_quads = 0;
        for chunk in store.loaded().collect::<Vec<_>>() {
            let mesh = greedy_mesh(chunk, |pos| store.get(pos, &terrain), &palette, &atlas);
            if !mesh.is_empty() {
                with_faces += 1;
                total_quads += mesh.quads;
            }
        }
        assert!(
            with_faces > 0,
            "3×3 区块里至少该有一个区块产出可见面（否则地表完全看不见）"
        );
        assert!(
            total_quads > 100,
            "地表该有可观的四边形数量，实际 {total_quads}"
        );
    }

    #[test]
    fn the_terrain_bounding_box_is_where_the_camera_expects() {
        // **量出来**而不是看截图猜：地形的真实包围盒必须落在相机视野内。
        let (terrain, blocks) = real_world_config();
        let mut palette = VoxelPalette::default();
        for block in &blocks {
            palette.push(VoxelAppearance {
                atlas_index: 0,
                opaque: block.opaque,
            });
        }
        let textures: Vec<BlockTexture> = blocks
            .iter()
            .map(|b| BlockTexture {
                side: FaceTexture {
                    base: b.side.base,
                    pattern: Pattern::Solid,
                },
                top: None,
                bottom: None,
                opaque: b.opaque,
            })
            .collect();
        let mut images = Assets::<Image>::default();
        let atlas = AtlasImage::build(&mut images, &textures);
        let mut store = VoxelStore::default();
        load_around(&mut store, &terrain, ChunkPos::default(), 1);

        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for chunk in store.loaded().collect::<Vec<_>>() {
            let mesh = greedy_mesh(chunk, |pos| store.get(pos, &terrain), &palette, &atlas);
            for p in &mesh.positions {
                for axis in 0..3 {
                    lo[axis] = lo[axis].min(p[axis]);
                    hi[axis] = hi[axis].max(p[axis]);
                }
            }
        }
        eprintln!(
            "地形包围盒 x={:?}..{:?} y={:?}..{:?} z={:?}..{:?}",
            lo[0], hi[0], lo[1], hi[1], lo[2], hi[2]
        );
        // 地表高度必须在配置的 base_height ± amplitude 内。
        assert!(
            lo[1] >= -3.0 && hi[1] <= 3.0,
            "地表 y 范围异常：{}..{}",
            lo[1],
            hi[1]
        );
        // 3×3 区块 = 96×96 格，中心在原点。
        assert!(
            lo[0] <= -32.0 && hi[0] >= 32.0,
            "x 覆盖不足：{}..{}",
            lo[0],
            hi[0]
        );
        assert!(
            lo[2] <= -32.0 && hi[2] >= 32.0,
            "z 覆盖不足：{}..{}",
            lo[2],
            hi[2]
        );
    }

    #[test]
    fn the_surface_layer_is_actually_above_the_floor() {
        // 地形不该整块埋在地下：地表高度附近必须有空气、下面必须有实心。
        let (terrain, _) = real_world_config();
        let mut air_above = 0;
        let mut solid_at = 0;
        for x in -8..8 {
            for z in -8..8 {
                let surface = terrain.surface_height(x, z);
                if terrain.voxel_at([x, surface + 1, z]).is_air() {
                    air_above += 1;
                }
                if terrain.voxel_at([x, surface, z]).is_solid() {
                    solid_at += 1;
                }
            }
        }
        assert_eq!(air_above, 256, "地表之上必须都是空气");
        assert_eq!(solid_at, 256, "地表那格必须是实心");
    }
}
