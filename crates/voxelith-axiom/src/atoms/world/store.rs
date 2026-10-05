//! 区块存储：已加载的区块 + 查询 / 修改的唯一入口。
//!
//! ## 查询顺序（**R66**）
//!
//! ```text
//! get_voxel(pos)
//!   1. 区块已加载 → 直接读数组
//!   2. 没加载      → 回退程序化函数（纯计算，不改世界）
//! ```
//!
//! `set_voxel` 会**先按需加载**目标区块（否则"改一格"会把整个区块的生成结果丢掉），
//! 改完发 [`ChunkDirtyMessage`]（**R70**）——这是数据层通知渲染层的**唯一**通道。
//!
//! ## 为什么"加载"不是"读文件"
//!
//! 现在的"持久化层"就是内存里的区块表：先生成、再改。真要落盘时，只需要在这里
//! 多一步"从磁盘读回改动"，`get_voxel` / `set_voxel` 的契约不变。

use std::collections::{HashMap, HashSet};

use bevy_ecs::prelude::*;
use bevy_reflect::Reflect;

use super::chunk::{Chunk, ChunkPos, split};
use super::generate::TerrainParams;
use super::voxel::Voxel;

/// 某个区块变了（**Message**，R33 / R70）。
///
/// 渲染层监听它重建网格。数据层不认识网格，所以消息里只有坐标。
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkDirtyMessage {
    /// 哪个区块。
    pub chunk: ChunkPos,
}

/// 已加载的区块表（**Resource**）。
#[derive(Resource, Debug, Clone, Default, Reflect)]
#[reflect(Resource)]
pub struct VoxelStore {
    /// 区块表。**反射时跳过**：一个区块 32768 个体素，序列化它没有意义
    /// （而且 BRP 会因此把响应撑爆）。
    #[reflect(ignore)]
    chunks: HashMap<ChunkPos, Chunk>,
    /// **被玩家改过的区块**（`set` 成功改动的都记在这里）。
    ///
    /// ## 为什么必须记这个
    ///
    /// 有三处需要知道"这个区块是不是原生的"：
    ///
    /// 1. **存档**：只存改动，不存地形（地形是确定性的推导结果，
    ///    见 `voxelith-prime::save` 的说明）。
    /// 2. **网格化的跳过优化**：底面为空**不等于**区块为空 ——
    ///    玩家可能在悬空处放方块。所以只有"没被改过"的区块才敢用便宜判断跳过。
    ///    （踩过：没有这个记录时，`can_skip` 只能一律返回 `false`。）
    /// 3. **将来做撤销 / 重放**时的依据。
    ///
    /// 只记**区块坐标**而不是每个体素的旧值：区块是 32³ = 32768 个体素，
    /// 记全部旧值会让内存随"走过的地方"线性增长。真要做撤销时
    /// 在**编辑系统**里另记一份有上限的历史。
    edited: HashSet<ChunkPos>,
}

impl VoxelStore {
    /// 读一格。
    ///
    /// 区块没加载时**不加载**（读不该有副作用），直接回退程序化函数。
    pub fn get(&self, pos: [i32; 3], terrain: &TerrainParams) -> Voxel {
        let (chunk_pos, local) = split(pos);
        match self.chunks.get(&chunk_pos) {
            Some(chunk) => chunk.get_local(local),
            None => terrain.voxel_at(pos),
        }
    }

    /// 这一格是不是实心。
    pub fn is_solid(&self, pos: [i32; 3], terrain: &TerrainParams) -> bool {
        self.get(pos, terrain).is_solid()
    }

    /// 改一格，返回"真的改了吗"。
    ///
    /// 返回 `true` 时调用方应当发 [`ChunkDirtyMessage`]。
    /// 只读场景（网格化）用 [`Self::get`]；这个方法会**按需加载区块**。
    pub fn set(&mut self, pos: [i32; 3], voxel: Voxel, terrain: &TerrainParams) -> bool {
        let (chunk_pos, local) = split(pos);
        let chunk = self
            .chunks
            .entry(chunk_pos)
            .or_insert_with(|| Chunk::generate(chunk_pos, |world| terrain.voxel_at(world)));
        let changed = chunk.set_local(local, voxel);
        if changed {
            // 只有**真的变了**才记：set_local 对"写同样的值"返回 alse，
            // 这样反复写同一格不会伪造出"改过"的记录。
            self.edited.insert(chunk_pos);
        }
        changed
    }

    /// 这个区块**被玩家改过**吗（见 dited 字段的说明）。
    pub fn is_edited(&self, chunk: ChunkPos) -> bool {
        self.edited.contains(&chunk)
    }

    /// 被改过的区块坐标（存档与调试用）。
    pub fn edited(&self) -> impl Iterator<Item = ChunkPos> + '_ {
        self.edited.iter().copied()
    }

    /// 被改过的区块数量。
    pub fn edited_count(&self) -> usize {
        self.edited.len()
    }

    /// **射线投射**（只读借用的入口）。
    ///
    /// 单独开这个方法，是为了让调用方能在**不必把 `store` 既借成 `&` 又借成 `&mut`**
    /// 的前提下"先看准了再改"——见 [`Self::set_and_notify`]。
    pub fn raycast(
        &self,
        terrain: &TerrainParams,
        origin: [f32; 3],
        direction: [f32; 3],
        max_distance: f32,
    ) -> Option<super::raycast::VoxelHit> {
        super::raycast::raycast_voxels(self, terrain, origin, direction, max_distance)
    }

    /// 改一格并发消息（系统里用的便捷入口）。
    pub fn set_and_notify(
        &mut self,
        pos: [i32; 3],
        voxel: Voxel,
        terrain: &TerrainParams,
        dirty: &mut MessageWriter<ChunkDirtyMessage>,
    ) -> bool {
        if !self.set(pos, voxel, terrain) {
            return false;
        }
        dirty.write(ChunkDirtyMessage {
            chunk: ChunkPos::of(pos),
        });
        true
    }

    /// 区块是否已加载。
    pub fn is_loaded(&self, chunk: ChunkPos) -> bool {
        self.chunks.contains_key(&chunk)
    }

    /// 主动加载一个区块（渲染层要网格化它时用）。
    ///
    /// 返回 `true` 表示"这次真的加载了"（之前不在表里）。
    pub fn load(&mut self, chunk: ChunkPos, terrain: &TerrainParams) -> bool {
        if self.chunks.contains_key(&chunk) {
            return false;
        }
        self.chunks.insert(
            chunk,
            Chunk::generate(chunk, |world| terrain.voxel_at(world)),
        );
        true
    }

    /// 借出一个区块（网格化用）。
    pub fn chunk(&self, chunk: ChunkPos) -> Option<&Chunk> {
        self.chunks.get(&chunk)
    }

    /// 已加载的区块坐标（**顺序不定**；要稳定顺序就自己排）。
    pub fn loaded(&self) -> impl Iterator<Item = ChunkPos> + '_ {
        self.chunks.keys().copied()
    }

    /// 已加载几个区块。
    pub fn loaded_count(&self) -> usize {
        self.chunks.len()
    }

    /// 卸载一个区块（丢弃它的改动）。
    pub fn unload(&mut self, chunk: ChunkPos) -> bool {
        self.chunks.remove(&chunk).is_some()
    }

    /// 丢掉所有已加载区块（下次查询回退程序化结果）。
    pub fn clear(&mut self) {
        self.chunks.clear();
    }
}

/// 按需要把一片区域的区块加载进来（战场开局：一次把战场范围内的地表准备好）。
///
/// 返回"新加载了几个"。`radius` 是**区块**半径。
pub fn load_around(
    store: &mut VoxelStore,
    terrain: &TerrainParams,
    center: ChunkPos,
    radius: i32,
) -> usize {
    let mut loaded = 0;
    for x in -radius..=radius {
        for z in -radius..=radius {
            let chunk = ChunkPos::new(center.x + x, center.y, center.z + z);
            if store.load(chunk, terrain) {
                loaded += 1;
            }
        }
    }
    loaded
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atoms::world::chunk::CHUNK_SIZE;
    use crate::atoms::world::voxel::VoxelId;

    fn terrain() -> TerrainParams {
        TerrainParams {
            surface: VoxelId(1),
            soil: VoxelId(2),
            deep: VoxelId(3),
            ..Default::default()
        }
    }

    #[test]
    fn reading_unloaded_chunks_falls_back_to_the_generator() {
        let store = VoxelStore::default();
        let terrain = terrain();
        let pos = [5, 0, 5];
        let expected = terrain.voxel_at(pos);
        assert_eq!(
            store.get(pos, &terrain),
            expected,
            "未加载也能读到程序化结果"
        );
        assert_eq!(store.loaded_count(), 0, "读**没有副作用**：不该顺手加载");
    }

    #[test]
    fn writing_loads_the_chunk_first() {
        // 直接往空表里写：不能把生成结果丢掉，否则改一格会得到一个"只有这一格"的世界。
        let mut store = VoxelStore::default();
        let terrain = terrain();
        let (x, z) = (40, -7);
        let surface = terrain.surface_height(x, z);
        let above = [x, surface + 1, z];
        assert!(store.get(above, &terrain).is_air());

        assert!(store.set(above, Voxel::solid(VoxelId(9)), &terrain));
        assert_eq!(store.get(above, &terrain).id, VoxelId(9), "改动生效");
        assert_eq!(
            store.get([x, surface, z], &terrain).id,
            terrain.surface,
            "同一区块里**其它**格子还是生成结果"
        );
        assert_eq!(store.loaded_count(), 1);
    }

    #[test]
    fn writing_the_same_value_reports_no_change() {
        let mut store = VoxelStore::default();
        let terrain = terrain();
        let pos = [1, 0, 1];
        let current = terrain.voxel_at(pos);
        assert!(
            !store.set(pos, current, &terrain),
            "写同样的值不该算改动（否则渲染层每帧白重建）"
        );
    }

    #[test]
    fn edits_are_kept_per_chunk_and_survive_other_edits() {
        let mut store = VoxelStore::default();
        let terrain = terrain();
        let a = [0, 0, 0];
        let b = [CHUNK_SIZE + 1, 0, 0];
        store.set(a, Voxel::solid(VoxelId(7)), &terrain);
        store.set(b, Voxel::solid(VoxelId(8)), &terrain);
        assert_eq!(store.get(a, &terrain).id, VoxelId(7));
        assert_eq!(store.get(b, &terrain).id, VoxelId(8));
        assert_eq!(store.loaded_count(), 2, "两个不同区块");
    }

    #[test]
    fn load_around_covers_a_square() {
        let mut store = VoxelStore::default();
        let terrain = terrain();
        let loaded = load_around(&mut store, &terrain, ChunkPos::new(0, 0, 0), 1);
        assert_eq!(loaded, 9, "半径 1 = 3×3");
        assert_eq!(
            load_around(&mut store, &terrain, ChunkPos::new(0, 0, 0), 1),
            0,
            "重复加载返回 0"
        );
    }

    /// **只有真的改了才算"改过"**。
    ///
    /// 这条撑起三件事（见 VoxelStore::edited 的说明）：存档的"存什么"、
    /// 网格化跳过优化是否安全、将来的撤销。
    /// 若"写同样的值"也记一笔，那么"读一遍再写回"就会伪造出改动记录，
    /// 存档会存下一堆其实没变的地形，跳过优化也会失效。
    #[test]
    fn writing_the_same_value_does_not_count_as_an_edit() {
        let terrain = terrain();
        let mut store = VoxelStore::default();
        let pos = [1, 0, 1];
        let current = store.get(pos, &terrain);

        assert_eq!(store.edited_count(), 0, "前提：一开始没有改动");
        // 写回同一个值 ⇒ 不算改动。
        assert!(!store.set(pos, current, &terrain), "写同样的值该返回 false");
        assert_eq!(store.edited_count(), 0, "写同样的值不该记成改动");
        assert!(!store.is_edited(ChunkPos::of(pos)));

        // 写一个**不同**的值 ⇒ 算改动。
        let other = if current.is_air() {
            Voxel::solid(VoxelId(1))
        } else {
            Voxel::default() // 默认就是空气
        };
        assert!(store.set(pos, other, &terrain), "写不同的值该返回 true");
        assert_eq!(store.edited_count(), 1, "真的改了就该记一笔");
        assert!(store.is_edited(ChunkPos::of(pos)));
    }

    /// 改动记录**按区块**记，不是按体素。
    #[test]
    fn edits_are_tracked_per_chunk() {
        let terrain = terrain();
        let mut store = VoxelStore::default();
        let a = [1, 0, 1];
        // 同一个区块里的另一个位置（区块边长 32）。
        let b = [2, 0, 2];
        // 隔壁区块。
        let c = [33, 0, 1];

        for (pos, target) in [(a, true), (b, true), (c, true)] {
            let current = store.get(pos, &terrain);
            let other = if current.is_air() {
                Voxel::solid(VoxelId(1))
            } else {
                Voxel::default()
            };
            assert!(store.set(pos, other, &terrain), "改动 {target} 该成功");
        }

        // a 与 b 在同一个区块 ⇒ 只记 1 个区块；c 在隔壁 ⇒ 共 2 个。
        assert_eq!(
            store.edited_count(),
            2,
            "同区块的多次改动该只记一个区块，隔壁区块另记一个"
        );
        assert!(store.is_edited(ChunkPos::of(a)));
        assert!(store.is_edited(ChunkPos::of(c)));
    }
}
