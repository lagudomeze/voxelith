//! 区块：坐标、局部索引、体素数组。
//!
//! 区块是**存储与网格化的单位**，不是"实体"。它住在
//! [`VoxelStore`](super::VoxelStore) 里，渲染层按 `ChunkDirtyMessage` 拿它去算网格。

use super::voxel::Voxel;
use bevy_reflect::Reflect;

/// 区块边长（**R64**：32³）。
///
/// 为什么是 32 而不是 16：战术战场只有几十格见方，32 让"一个区块装得下一片战场"，
/// 减少区块数与跨区块网格接缝。改动它会影响网格化与存储布局，所以是 `pub const` 而不是配置项。
pub const CHUNK_SIZE: i32 = 32;

/// 一个区块里的体素数。
pub const CHUNK_VOLUME: usize = (CHUNK_SIZE * CHUNK_SIZE * CHUNK_SIZE) as usize;

/// 区块坐标（世界坐标 / [`CHUNK_SIZE`] 的商）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Reflect)]
pub struct ChunkPos {
    /// 区块 X。
    pub x: i32,
    /// 区块 Y。
    pub y: i32,
    /// 区块 Z。
    pub z: i32,
}

impl ChunkPos {
    /// 造一个区块坐标。
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    /// 某个体素属于哪个区块。
    ///
    /// 用 `div_euclid` 而不是 `/`：负坐标下 Rust 的整除向零取整，
    /// `-1 / 32 == 0` 会把 `-1` 归到区块 `0`，于是"世界坐标 → 区块"在原点两侧不对称。
    pub fn of(voxel: [i32; 3]) -> Self {
        Self {
            x: voxel[0].div_euclid(CHUNK_SIZE),
            y: voxel[1].div_euclid(CHUNK_SIZE),
            z: voxel[2].div_euclid(CHUNK_SIZE),
        }
    }

    /// 区块左上角的世界体素坐标。
    pub fn origin(self) -> [i32; 3] {
        [
            self.x * CHUNK_SIZE,
            self.y * CHUNK_SIZE,
            self.z * CHUNK_SIZE,
        ]
    }
}

/// 世界体素坐标 → （区块，区块内局部坐标）。
///
/// 局部坐标恒在 `0..CHUNK_SIZE`（`rem_euclid` 保证）。
pub fn split(voxel: [i32; 3]) -> (ChunkPos, [u8; 3]) {
    let chunk = ChunkPos::of(voxel);
    let local = [
        voxel[0].rem_euclid(CHUNK_SIZE) as u8,
        voxel[1].rem_euclid(CHUNK_SIZE) as u8,
        voxel[2].rem_euclid(CHUNK_SIZE) as u8,
    ];
    (chunk, local)
}

/// 局部坐标 → 数组下标（**x 变化最快**，便于按行遍历）。
pub fn local_index(local: [u8; 3]) -> usize {
    let x = local[0] as usize;
    let y = local[1] as usize;
    let z = local[2] as usize;
    x + y * CHUNK_SIZE as usize + z * (CHUNK_SIZE as usize * CHUNK_SIZE as usize)
}

/// 一个区块的体素。
///
/// 用**数组**而不是 `HashMap`：区块内是稠密的，数组的 `get` 是常数时间且无哈希开销——
/// 网格化每帧要问几十万次"这格是啥、邻居是啥"。
#[derive(Debug, Clone, PartialEq)]
pub struct Chunk {
    /// 区块坐标。
    pub pos: ChunkPos,
    voxels: Box<[Voxel; CHUNK_VOLUME]>,
    /// 有没有被改过（渲染层可据此跳过"生成后没动过"的区块重建）。
    pub modified: bool,
}

impl Chunk {
    /// 造一个全空气的区块。
    pub fn empty(pos: ChunkPos) -> Self {
        Self {
            pos,
            voxels: Box::new([Voxel::AIR; CHUNK_VOLUME]),
            modified: false,
        }
    }

    /// 造一个区块，体素由 `fill` 按**世界坐标**决定。
    pub fn generate(pos: ChunkPos, mut fill: impl FnMut([i32; 3]) -> Voxel) -> Self {
        let mut chunk = Self::empty(pos);
        let origin = pos.origin();
        for z in 0..CHUNK_SIZE {
            for y in 0..CHUNK_SIZE {
                for x in 0..CHUNK_SIZE {
                    let world = [origin[0] + x, origin[1] + y, origin[2] + z];
                    chunk.voxels[local_index([x as u8, y as u8, z as u8])] = fill(world);
                }
            }
        }
        chunk
    }

    /// 读一格（局部坐标越界返回空气）。
    pub fn get_local(&self, local: [u8; 3]) -> Voxel {
        if local.iter().any(|&value| value as i32 >= CHUNK_SIZE) {
            return Voxel::AIR;
        }
        self.voxels[local_index(local)]
    }

    /// 写一格（局部坐标越界**忽略**，返回 `false`）。
    pub fn set_local(&mut self, local: [u8; 3], voxel: Voxel) -> bool {
        if local.iter().any(|&value| value as i32 >= CHUNK_SIZE) {
            return false;
        }
        let index = local_index(local);
        if self.voxels[index] == voxel {
            return false;
        }
        self.voxels[index] = voxel;
        self.modified = true;
        true
    }

    /// 原始数组（网格化用；顺序由 [`local_index`] 定义）。
    pub fn voxels(&self) -> &[Voxel; CHUNK_VOLUME] {
        &self.voxels
    }

    /// 非空气格数。
    pub fn solid_count(&self) -> usize {
        self.voxels.iter().filter(|voxel| voxel.is_solid()).count()
    }
}

#[cfg(test)]
mod tests {
    use super::super::voxel::VoxelId;
    use super::*;

    #[test]
    fn chunk_pos_is_symmetric_around_the_origin() {
        // 用 `div_euclid`：`-1` 属于区块 `-1`，而不是被向零取整归到 `0`。
        assert_eq!(ChunkPos::of([0, 0, 0]), ChunkPos::new(0, 0, 0));
        assert_eq!(ChunkPos::of([31, 0, 0]), ChunkPos::new(0, 0, 0));
        assert_eq!(ChunkPos::of([32, 0, 0]), ChunkPos::new(1, 0, 0));
        assert_eq!(ChunkPos::of([-1, 0, 0]), ChunkPos::new(-1, 0, 0));
        assert_eq!(ChunkPos::of([-32, 0, 0]), ChunkPos::new(-1, 0, 0));
        assert_eq!(ChunkPos::of([-33, -1, -64]), ChunkPos::new(-2, -1, -2));
    }

    #[test]
    fn split_round_trips_for_negative_coordinates() {
        for voxel in [[0, 0, 0], [-1, 5, -33], [100, -7, 63], [-100, 0, -100]] {
            let (chunk, local) = split(voxel);
            let origin = chunk.origin();
            let rebuilt = [
                origin[0] + local[0] as i32,
                origin[1] + local[1] as i32,
                origin[2] + local[2] as i32,
            ];
            assert_eq!(rebuilt, voxel, "{voxel:?} 拆开再拼回去该是原值");
            assert!(local.iter().all(|&value| (value as i32) < CHUNK_SIZE));
        }
    }

    #[test]
    fn local_index_is_injective_and_in_range() {
        let mut seen = std::collections::HashSet::new();
        for z in 0..CHUNK_SIZE as u8 {
            for y in 0..CHUNK_SIZE as u8 {
                for x in 0..CHUNK_SIZE as u8 {
                    let index = local_index([x, y, z]);
                    assert!(index < CHUNK_VOLUME);
                    assert!(seen.insert(index), "下标重复：{:?}", [x, y, z]);
                }
            }
        }
        assert_eq!(seen.len(), CHUNK_VOLUME);
    }

    #[test]
    fn generate_uses_world_coordinates() {
        // `fill` 收到的是**世界坐标**，不是一个区块内的局部坐标。
        let chunk = Chunk::generate(ChunkPos::new(1, 0, 0), |world| {
            if world[0] == 32 && world[1] == 0 && world[2] == 0 {
                Voxel::solid(VoxelId(1))
            } else {
                Voxel::AIR
            }
        });
        assert_eq!(chunk.get_local([0, 0, 0]), Voxel::solid(VoxelId(1)));
        assert_eq!(chunk.get_local([1, 0, 0]), Voxel::AIR);
    }

    #[test]
    fn out_of_range_access_is_safe() {
        let mut chunk = Chunk::empty(ChunkPos::default());
        assert_eq!(chunk.get_local([CHUNK_SIZE as u8, 0, 0]), Voxel::AIR);
        assert!(!chunk.set_local([CHUNK_SIZE as u8, 0, 0], Voxel::solid(VoxelId(1))));
    }

    #[test]
    fn writing_reports_whether_anything_changed() {
        let mut chunk = Chunk::empty(ChunkPos::default());
        assert!(!chunk.modified);
        assert!(chunk.set_local([1, 2, 3], Voxel::solid(VoxelId(1))));
        assert!(chunk.modified);
        assert!(
            !chunk.set_local([1, 2, 3], Voxel::solid(VoxelId(1))),
            "写同一个值不算改动（否则渲染层会白重建网格）"
        );
        assert_eq!(chunk.solid_count(), 1);
    }
}
