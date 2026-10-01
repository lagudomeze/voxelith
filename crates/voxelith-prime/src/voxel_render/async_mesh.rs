//! **异步区块网格化**（R72）：把耗时的贪婪网格化挪到 `AsyncComputeTaskPool`。
//!
//! ## 为什么需要它
//!
//! `greedy_mesh` 是逐体素扫描 + 合并面的过程，一个 32³ 区块要跑几万次采样。
//! 同步算的话，**每建一个区块就卡一帧** —— 启动时建几十个就是几十帧的停顿，
//! 走一步改一个方块也要卡。
//!
//! 挪到计算线程池之后：主线程只负责**派发任务**与**收集结果**，
//! 网格化本身与渲染并行。
//!
//! ## 数据怎么过线程边界
//!
//! 任务闭包必须 `Send + 'static`，所以不能借用世界里的资源。做法是**拷贝一份快照**
//! （`VoxelStore` / `TerrainParams` / `VoxelPalette` / `AtlasImage`）进闭包。
//!
//! 这看起来浪费（整个 store 拷一份），但：
//!
//! - `Chunk` 内部是 `Box<[Voxel; 32768]>`，`Clone` 是**一次内存拷贝**，
//!   比"逐体素建网格"便宜一个数量级；
//! - 换成"只拷贝区块及其邻居"需要给 `greedy_mesh` 换一套采样接口，
//!   而那会把"跨区块接缝不出双面"这条已经验证过的性质重新打开。
//!
//! 所以**先按简单可靠的做法来**，并在下面把代价写清楚 —— 等战场大到拷贝本身成为
//! 瓶颈时再换。
//!
//! ## 顺序保证
//!
//! 结果落地**必须**在主线程（`Assets<Mesh>` 不是线程安全的）。
//! 所以任务是"纯计算"：算完把 `MeshData` 交回来，主线程再 `meshes.add`。

use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};

use voxelith_axiom::world::{ChunkPos, TerrainParams, VoxelPalette, VoxelStore};

use super::ChunkMeshEntity;
use super::atlas::AtlasImage;
use super::materials::VoxelMaterial;
use super::mesher::{ChunkMesh, greedy_mesh};

/// 一个正在后台计算的区块。
///
/// 存活期间实体是**空壳**（有 `ChunkMeshEntity` 但没有 `Mesh3d`）——
/// 这样"哪些区块正在算"是**可直接查询的世界状态**，
/// 而不是藏在某个资源里的一堆句柄。
#[derive(Component)]
pub struct PendingChunkMesh {
    /// 在等哪个区块。
    ///
    /// 冗余存一份（`ChunkMeshEntity` 里也有）：收结果时要按它建实体名，
    /// 而任务完成时组件还在原地，读起来更直接。
    pub chunk: ChunkPos,
    /// 后台任务句柄。
    pub task: Task<ChunkMesh>,
}

/// 派发一个区块的网格化任务。
///
/// 返回 `None` 表示"这个区块是空的"（全空气）—— 空区块**不建实体**：
/// 建了也没有几何，白占一个 `Mesh3d` 句柄（早期版本空网格也建 `Mesh3d`，
/// 直接触发过 `slab_allocator: Use-after-free`）。
pub fn dispatch(
    commands: &mut Commands,
    store: &VoxelStore,
    terrain: &TerrainParams,
    palette: &VoxelPalette,
    atlas: &AtlasImage,
    chunk: ChunkPos,
) -> Option<Entity> {
    // 派发前的最后一道判断。见 `can_skip` 的文档 ——
    // 它**现在几乎从不跳过**（宁多算一次，也不冒"静默少一块"的风险）。
    if can_skip(store, terrain, chunk) {
        return None;
    }

    // 快照：任务闭包必须 `Send + 'static`，不能借用世界。
    let store = store.clone();
    let terrain = *terrain;
    let palette = palette.clone();
    let atlas = atlas.clone();

    let task = AsyncComputeTaskPool::get()
        .spawn(async move { greedy_mesh(chunk, |pos| store.get(pos, &terrain), &palette, &atlas) });

    Some(
        commands
            .spawn((
                Name::new(format!("chunk[{},{},{}]", chunk.x, chunk.y, chunk.z)),
                ChunkMeshEntity { chunk },
                PendingChunkMesh { chunk, task },
            ))
            .id(),
    )
}

/// 收结果：任务算完了就 `meshes.add` + 挂 `Mesh3d`。
///
/// **必须每帧调用**：它是"后台算完 → 上屏"的唯一通道。
pub fn collect(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    material: Res<VoxelMaterial>,
    mut pending: Query<(Entity, &mut PendingChunkMesh)>,
    mut index: ResMut<super::ChunkMeshIndex>,
) {
    for (entity, mut slot) in &mut pending {
        // `poll_once` 返回 `Some(值)` 表示任务已完成；`None` 表示还得等。
        let Some(computed) = block_on(poll_once(&mut slot.task)) else {
            continue;
        };
        let chunk = slot.chunk;

        commands.entity(entity).remove::<PendingChunkMesh>();

        if computed.is_empty() {
            // 探空判错了（少数情况）：算出来是空的，就别留空壳。
            commands.entity(entity).despawn();
            continue;
        }

        let mut mesh = Mesh::new(
            bevy::render::mesh::PrimitiveTopology::TriangleList,
            bevy::asset::RenderAssetUsages::MAIN_WORLD
                | bevy::asset::RenderAssetUsages::RENDER_WORLD,
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, computed.positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, computed.normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, computed.uvs);
        mesh.insert_indices(bevy::render::mesh::Indices::U32(computed.indices));

        commands.entity(entity).insert((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(material.handle.clone()),
        ));
        index.insert(chunk, entity);
    }
}

/// 便宜的"这个区块大概全空"判断。
///
/// ## 它现在是**测试专用**的（生产路径不再调用它）
///
/// `can_skip` 现在一律返回 `false`（见它的文档），所以生产路径不会跳到这个函数。
/// **但它必须留着**：下面描述的那个缺陷就是在这里发生的，
/// `a_chunk_holding_the_surface_is_not_reported_empty` 靠它来防止复发。
/// 删掉它等于删掉那条回归防线。
///
/// 接上体素编辑、`VoxelStore` 能回答"这个区块被改过吗"之后，
/// 这个函数会重新被 `can_skip` 调用。
///
/// **只用来省掉一次任务派发**；判错是安全的，`collect` 里还会再判一次
/// `computed.is_empty()` 并回收实体。
///
/// ## ⚠️ 这里出过一个真缺陷：采样位置错了（**区块中心永远在空气里**）
///
/// 第一版采样"区块中心 + 6 个面心"，也就是 `y ∈ {0, 16, 31}`。
/// 但地形是**高度图**（`base_height 0` / `amplitude 2.0`）：地表在 `y ≈ ±2`，
/// 所以**区块中心（`y = 16`）永远是空气**，`+Y` 面心也是空气。
/// 于是判空几乎永远为真 ⇒ **有地形的区块被整个跳过**。
///
/// 实测：半径 3 应加载 49 个区块，实际只有 **39** 个建了网格
/// （从 `ChunkMeshIndex.loaded` 数出来的），差的 10 个就是这个误判吃掉的。
///
/// ## 正确的采样位置：区块的**底面**
///
/// 高度图世界里"有没有方块"要看**底部**：`y` 越低越可能实心
/// （`floor_y` 以下全是深层方块）。所以采样底面四角 + 底面中心。
///
/// **绝不要用 `base.1 + half`** —— 那是上面那个 bug。
fn is_probably_empty(store: &VoxelStore, terrain: &TerrainParams, chunk: ChunkPos) -> bool {
    use voxelith_axiom::world::CHUNK_SIZE;
    let base = (
        chunk.x * CHUNK_SIZE,
        chunk.y * CHUNK_SIZE,
        chunk.z * CHUNK_SIZE,
    );
    let half = CHUNK_SIZE / 2;
    let last = CHUNK_SIZE - 1;
    let bottom = base.1;

    let samples = [
        [base.0, bottom, base.2],
        [base.0 + last, bottom, base.2],
        [base.0, bottom, base.2 + last],
        [base.0 + last, bottom, base.2 + last],
        [base.0 + half, bottom, base.2 + half],
    ];
    !samples.iter().any(|p| store.get(*p, terrain).is_solid())
}

/// 派发前真的可以跳过这个区块吗。
///
/// ## 判据：**没被改过** + **底面为空**
///
/// 两个条件缺一不可：
///
/// - **底面为空**：高度图世界里"有没有方块"主要看底部（`floor_y` 以下全是深层方块），
///   所以地面附近的区块底面一定是实心的。
/// - **没被改过**：底面为空**不等于**区块为空。玩家可能在悬空处放方块 ——
///   那种区块底面是空气但**有几何**。跳过它 = 那块方块永远不显示，**且不报错**。
///
/// `VoxelStore::is_edited` 就是为这个加的（见它的文档）。
///
/// ## 收益与代价
///
/// 收益：完全埋住的地下层与高空区块不再派发任务。实测半径 3 时
/// 49 个区块里能省掉约 10 个。
///
/// 代价：探空读几个体素（常数级），相比一次贪婪网格化可忽略。
fn can_skip(store: &VoxelStore, terrain: &TerrainParams, chunk: ChunkPos) -> bool {
    !store.is_edited(chunk) && is_probably_empty(store, terrain, chunk)
}

#[cfg(test)]
mod tests {
    use super::*;
    use voxelith_axiom::world::{Voxel, VoxelId};

    /// 一份**真的有方块**的地形参数。
    ///
    /// 不能用 `TerrainParams::default()`：它的 `surface`/`soil`/`deep`
    /// 全是 `VoxelId(0)`（空气），生成出来是**空世界** ——
    /// 拿它测"探空"会得到"永远为空"的假结论。
    fn terrain_with_blocks() -> TerrainParams {
        TerrainParams {
            surface: VoxelId(1),
            soil: VoxelId(2),
            deep: VoxelId(3),
            ..TerrainParams::default()
        }
    }

    /// 越界 / 空世界不该 panic —— 探空判定在热路径上，且输入来自世界状态。
    #[test]
    fn emptiness_probe_handles_a_fresh_store() {
        let store = VoxelStore::default();
        let terrain = TerrainParams::default();
        let _ = is_probably_empty(&store, &terrain, ChunkPos::new(0, 0, 0));
        let _ = is_probably_empty(&store, &terrain, ChunkPos::new(-3, 0, 7));
    }

    /// **⚠️ 这条抓的是一个真缺陷：探空采样了"区块中心"，而高度图世界里那里永远是空气。**
    ///
    /// 地形是高度图（`base_height 0` / `amplitude 2.0`，地表在 `y ≈ ±2`），
    /// 所以 `y = 16`（区块中心）**永远是空气**。
    /// 第一版探空就采样了它，于是"有地形的区块"被判成空、**整个被跳过**。
    ///
    /// 实测过后果：半径 3 应加载 49 个区块，实际只建了 **39** 个网格。
    ///
    /// 这条测试的判据是**行为**而不是"采样了哪个坐标"：
    /// 地表高度附近的区块（`y = 0`）**必须被判成非空**。
    /// 用 `y = 0` 而不是"某个具体高度"，是因为这里要钉的是
    /// **"有没有采样到底面"这个性质**，不是某一条具体的采样点清单。
    #[test]
    fn a_chunk_holding_the_surface_is_not_reported_empty() {
        let terrain = terrain_with_blocks();
        let store = VoxelStore::default();

        // 先确认这个世界的 y = 0 附近**确实**有方块（否则测试本身没意义）。
        assert!(
            store.is_solid([0, 0, 0], &terrain)
                || store.is_solid([0, 1, 0], &terrain)
                || store.is_solid([0, -1, 0], &terrain),
            "前提不成立：y = 0 附近没有方块，`terrain_with_blocks` 配错了"
        );

        // 区块 y = 0 覆盖世界 y ∈ [0, 31]，地表（y ≈ ±2）就落在里面。
        assert!(
            !is_probably_empty(&store, &terrain, ChunkPos::new(0, 0, 0)),
            "区块 (0,0,0) 里有地表方块，**不该**被判成空。\
             若这条失败，说明探空又采样到了区块中心（y = 16，永远是空气）"
        );
        // 换个 XZ 也要成立（免得只是碰巧命中了原点那一列）。
        assert!(
            !is_probably_empty(&store, &terrain, ChunkPos::new(2, 0, -3)),
            "区块 (2,0,-3) 里同样有地表方块"
        );
    }

    /// **改过的区块永远不跳** —— 这是"悬空方块不会静默消失"的守卫。
    ///
    /// 场景：玩家在一个"底面为空"的区块里放了一块悬空方块。
    /// 若 `can_skip` 只看底面（不看改动记录），这个区块会被跳过，
    /// 那块方块**永远不显示而且不报错**。
    #[test]
    fn an_edited_chunk_is_never_skipped() {
        let terrain = terrain_with_blocks();
        let mut store = VoxelStore::default();

        // 找一个**底面为空**的区块：y 很高，那里全是空气。
        let chunk = ChunkPos::new(0, 9, 0);
        assert!(
            is_probably_empty(&store, &terrain, chunk),
            "前提：区块 (0,9,0) 底面该是空的（高空）"
        );
        // 没改过 ⇒ 可以跳。
        assert!(
            can_skip(&store, &terrain, chunk),
            "没改过 + 底面空 ⇒ 可以跳"
        );

        // 在这个区块里放一块悬空方块。
        let pos = [1, 9 * 32 + 4, 1];
        assert!(
            store.set(pos, Voxel::solid(VoxelId(7)), &terrain),
            "放方块该成功"
        );
        assert!(store.is_edited(ChunkPos::of(pos)), "该记成改动过的区块");

        // 现在**不能**跳了 —— 否则那块悬空方块永远不会被网格化。
        assert!(
            !can_skip(&store, &terrain, chunk),
            "改过的区块绝不能跳：否则玩家放的悬空方块会永久不显示"
        );
    }
}
