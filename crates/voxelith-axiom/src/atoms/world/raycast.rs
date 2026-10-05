//! 体素射线检测：**DDA 逐格步进**（**R67**）。
//!
//! 为什么不用"细步长采样"：步长大了会**穿过**薄墙（漏格），小了则同一格被反复问、
//! 开销与"步数"而不是"跨过的格数"成正比。DDA 每次精确跨过一个格边界——
//! 无漏格、无重复，开销正比于射线经过的格数。
//!
//! 算法（Amanatides & Woo）：对每个轴记录"下一次跨界的参数 `tMax`"与"每格对应的参数增量
//! `tDelta`"，每步推进 `tMax` 最小的那个轴，并记下跨过的是哪个面（法线）——
//! 法线是**放置方块**的依据（**R69**：放在 `pos + normal`，不是玩家位置）。

use super::generate::TerrainParams;
use super::store::VoxelStore;
use bevy_reflect::Reflect;

/// 射线命中结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect)]
pub struct VoxelHit {
    /// 命中的体素（世界坐标，整数格）。
    pub pos: [i32; 3],
    /// 命中面的**朝外法线**（单位向量，只有一个分量是 ±1）。
    ///
    /// 放置方块用 `pos + normal`。
    pub normal: [i32; 3],
}

/// 从 `origin` 沿 `direction` 找第一个实心体素。
///
/// - `origin` 是**世界坐标**（可以是小数，比如相机或角色眼睛的位置）。
/// - `direction` 不必归一化；`max_distance` 是沿**归一化方向**的最大距离（格）。
/// - 起点所在的格**也会被检查**（否则贴着地面往上打会漏掉脚下那格）。
pub fn raycast_voxels(
    store: &VoxelStore,
    terrain: &TerrainParams,
    origin: [f32; 3],
    direction: [f32; 3],
    max_distance: f32,
) -> Option<VoxelHit> {
    let length =
        (direction[0] * direction[0] + direction[1] * direction[1] + direction[2] * direction[2])
            .sqrt();
    if length == 0.0 || max_distance <= 0.0 {
        return None;
    }
    let dir = [
        direction[0] / length,
        direction[1] / length,
        direction[2] / length,
    ];

    // 当前格 + 前进方向。
    let mut voxel = [
        origin[0].floor() as i32,
        origin[1].floor() as i32,
        origin[2].floor() as i32,
    ];
    let step = [
        dir[0].signum() as i32,
        dir[1].signum() as i32,
        dir[2].signum() as i32,
    ];

    // `t_max[axis]`：沿射线走到该轴下一个格边界所需的参数（以方向长度为 1 计）。
    // `t_delta[axis]`：跨过一整格需要的参数。
    let mut t_max = [f32::INFINITY; 3];
    let mut t_delta = [f32::INFINITY; 3];
    for axis in 0..3 {
        if dir[axis] == 0.0 {
            continue;
        }
        let delta = (1.0 / dir[axis]).abs();
        t_delta[axis] = delta;
        // 到该轴下一个边界的距离。
        let boundary = if step[axis] > 0 {
            (voxel[axis] + 1) as f32
        } else {
            voxel[axis] as f32
        };
        t_max[axis] = (boundary - origin[axis]) / dir[axis];
    }

    // 起点就在实心格里 → 直接命中，法线取"从哪边来的"（贴着面打的情况）。
    let start = store.get(voxel, terrain);
    if start.is_solid() {
        return Some(VoxelHit {
            pos: voxel,
            normal: entry_normal(dir),
        });
    }

    // 最多走 `max_distance` 格（多走一格无害，但别让它无限跑）。
    let steps = max_distance.ceil().max(1.0) as i32 + 1;
    for _ in 0..steps {
        // 推进到 `t_max` 最小的那个轴。
        let axis = if t_max[0] < t_max[1] && t_max[0] < t_max[2] {
            0
        } else if t_max[1] < t_max[2] {
            1
        } else {
            2
        };
        if t_max[axis] > max_distance {
            return None;
        }
        voxel[axis] += step[axis];
        t_max[axis] += t_delta[axis];

        // 跨过的是 `axis` 轴上、**反向**的那个面。
        let mut normal = [0, 0, 0];
        normal[axis] = -step[axis];

        if store.get(voxel, terrain).is_solid() {
            return Some(VoxelHit { pos: voxel, normal });
        }
    }
    None
}

/// 起点已经在实心格里时，给一个"从射线来向推出来的"法线。
fn entry_normal(dir: [f32; 3]) -> [i32; 3] {
    // 取分量绝对值最大的轴，法线朝**射线来向的反面**。
    let mut axis = 0;
    for candidate in 1..3 {
        if dir[candidate].abs() > dir[axis].abs() {
            axis = candidate;
        }
    }
    let mut normal = [0, 0, 0];
    // 射线沿 +axis 前进 → 它"撞上"的是该轴的负向面。
    normal[axis] = if dir[axis] > 0.0 { -1 } else { 1 };
    normal
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atoms::world::voxel::{Voxel, VoxelId};

    /// 一个全实心的测试地形（`surface` 极高 → 到处都是地表方块）。
    fn solid_terrain() -> TerrainParams {
        TerrainParams {
            base_height: 64,
            amplitude: 0.0,
            surface: VoxelId(1),
            soil: VoxelId(2),
            deep: VoxelId(3),
            floor_y: -64,
            ..Default::default()
        }
    }

    /// 一片空地形（地表很低 → 上面全是空气），配合 `set` 精确摆方块。
    fn empty_terrain() -> TerrainParams {
        TerrainParams {
            base_height: -32,
            amplitude: 0.0,
            soil_depth: 1,
            surface: VoxelId(1),
            soil: VoxelId(2),
            deep: VoxelId(3),
            floor_y: -64,
            ..Default::default()
        }
    }

    #[test]
    fn hits_the_solid_ground_straight_below() {
        let store = VoxelStore::default();
        let terrain = solid_terrain();
        let hit = raycast_voxels(&store, &terrain, [0.5, 70.0, 0.5], [0.0, -1.0, 0.0], 20.0)
            .expect("应该打到地面");
        assert_eq!(hit.pos, [0, 64, 0], "第一个实心格是地表那格");
        assert_eq!(hit.normal, [0, 1, 0], "从上方打下来 → 命中上表面");
    }

    #[test]
    fn placing_at_pos_plus_normal_lands_on_the_hit_face() {
        // R69 的依据：命中面的法线决定放置位置。
        let store = VoxelStore::default();
        let terrain = solid_terrain();
        let hit =
            raycast_voxels(&store, &terrain, [0.5, 70.0, 0.5], [0.0, -1.0, 0.0], 20.0).unwrap();
        let place_at = [
            hit.pos[0] + hit.normal[0],
            hit.pos[1] + hit.normal[1],
            hit.pos[2] + hit.normal[2],
        ];
        assert_eq!(place_at, [0, 65, 0]);
        assert_eq!(
            store.get(place_at, &terrain),
            Voxel::AIR,
            "待放置的位置该是空气"
        );
    }

    #[test]
    fn does_not_leak_through_a_thin_wall() {
        // 这一条正是"细步长采样"会失败的场景：薄墙必须挡住射线。
        let mut store = VoxelStore::default();
        let terrain = empty_terrain();
        // 在 x = 10 摆一面只有一格厚的墙。
        for y in -4..4 {
            for z in -4..4 {
                store.set([10, y, z], Voxel::solid(VoxelId(5)), &terrain);
            }
        }
        let hit = raycast_voxels(&store, &terrain, [0.5, 0.5, 0.5], [1.0, 0.0, 0.0], 30.0)
            .expect("薄墙必须挡住");
        assert_eq!(hit.pos, [10, 0, 0]);
        assert_eq!(hit.normal, [-1, 0, 0], "命中墙的朝向来面");
    }

    #[test]
    fn respects_the_distance_limit() {
        let store = VoxelStore::default();
        let terrain = solid_terrain();
        assert!(
            raycast_voxels(&store, &terrain, [0.5, 200.0, 0.5], [0.0, -1.0, 0.0], 5.0).is_none(),
            "距离不够就什么都不该命中"
        );
    }

    #[test]
    fn sees_the_voxel_the_origin_stands_in() {
        let store = VoxelStore::default();
        let terrain = solid_terrain();
        let hit = raycast_voxels(&store, &terrain, [0.5, 64.5, 0.5], [0.0, 1.0, 0.0], 5.0)
            .expect("站在实心格里要能命中脚下那格，否则贴地往上打会漏");
        assert_eq!(hit.pos, [0, 64, 0]);
    }

    #[test]
    fn axis_aligned_ray_hits_a_voxel_on_its_own_line() {
        // 沿 +x 的射线**始终在 z = 0 那一列**上（dir.z == 0 ⇒ z 永不跨界），
        // 所以靶子必须摆在同一条线上。
        let mut store = VoxelStore::default();
        let terrain = empty_terrain();
        store.set([5, 0, 0], Voxel::solid(VoxelId(5)), &terrain);

        let hit = raycast_voxels(&store, &terrain, [0.5, 0.5, 0.5], [1.0, 0.0, 0.0], 20.0)
            .expect("同一条线上该命中");
        assert_eq!(hit.pos, [5, 0, 0]);
        assert_eq!(hit.normal, [-1, 0, 0]);
    }

    #[test]
    fn axis_aligned_ray_ignores_voxels_off_its_line() {
        // 反向对照：摆在旁边（z = 5）的方块不该被沿 +x 的射线碰到。
        let mut store = VoxelStore::default();
        let terrain = empty_terrain();
        store.set([5, 0, 5], Voxel::solid(VoxelId(5)), &terrain);
        assert!(
            raycast_voxels(&store, &terrain, [0.5, 0.5, 0.5], [1.0, 0.0, 0.0], 20.0).is_none(),
            "射线在自己的那一列上走，不该拐到 z = 5"
        );
    }

    #[test]
    fn diagonal_ray_walks_both_axes() {
        // 45° 斜射：x 与 z 交替跨界，靶子摆在对角线上。
        let mut store = VoxelStore::default();
        let terrain = empty_terrain();
        store.set([3, 0, 3], Voxel::solid(VoxelId(5)), &terrain);

        let hit = raycast_voxels(&store, &terrain, [0.5, 0.5, 0.5], [1.0, 0.0, 1.0], 20.0)
            .expect("斜射该命中对角线上的方块");
        assert_eq!(hit.pos, [3, 0, 3]);
    }

    #[test]
    fn diagonal_ray_does_not_skip_corners() {
        // 45° 射线恰好穿过格棱：DDA 必须逐格走，不能"跳格"漏掉转角处的方块。
        let mut store = VoxelStore::default();
        let terrain = empty_terrain();
        store.set([2, 0, 2], Voxel::solid(VoxelId(5)), &terrain);
        let hit = raycast_voxels(&store, &terrain, [0.5, 0.5, 0.5], [1.0, 0.0, 1.0], 20.0)
            .expect("转角上的方块也要命中");
        assert_eq!(hit.pos, [2, 0, 2]);
    }

    #[test]
    fn degenerate_input_returns_none() {
        let store = VoxelStore::default();
        let terrain = solid_terrain();
        assert!(raycast_voxels(&store, &terrain, [0.0; 3], [0.0; 3], 10.0).is_none());
        assert!(raycast_voxels(&store, &terrain, [0.0; 3], [0.0, -1.0, 0.0], 0.0).is_none());
    }

    #[test]
    fn a_boundary_start_inside_a_solid_cell_reports_the_entry_face() {
        // **这条抓的是 	_max / origin.floor() 的边界约定 bug。**
        //
        // 起点 x = 2.0 **正好落在格 1 与格 2 的交界**上，方向 -x。
        // 射线一出发就已经在**格 1** 里（x = 2.0 是格 1 的右边界）。
        //
        // 于是：**若格 1 是实心的，射线一开始就在实心介质里** ——
        // 正确的答案是"命中格 1，从它的 **+x** 面进入"。
        //
        // 实现的 bug：loor(2.0) = 2，所以它把起点格当成格 2（空的），
        // 然后 	_max = 0 一步跨到格 1，再从"跨过 +x 轴"推出法线 **-x**。
        // ⇒ **命中了，但法线反了**。
        //
        // 法线是放置方块的依据（**R69**：放在 pos + normal），
        // 反了就会把方块放到实体**内部**。这种错很难在画面上看出来。
        let mut store = VoxelStore::default();
        let terrain = empty_terrain();
        store.set([1, 0, 0], Voxel::solid(VoxelId(5)), &terrain);

        let hit = raycast_voxels(&store, &terrain, [2.0, 0.5, 0.5], [-1.0, 0.0, 0.0], 10.0)
            .expect("射线起点已在格 1 内，必须命中它");
        assert_eq!(hit.pos, [1, 0, 0]);
        assert_eq!(
            hit.normal,
            [1, 0, 0],
            "射线在 x=2.0 处已位于格 1（它的 +x 边界），所以是从 +x 面进入的；\
             若报成 -x，说明起点格被算成了格 2（loor 的边界约定错了）"
        );
    }

    #[test]
    fn a_boundary_start_going_positive_keeps_the_entry_face() {
        // 正向对照：起点 x = 1.0 朝 +x，格 1 是实心的。
        // 射线在 x = 1.0 处位于格 1（它的 -x 边界）⇒ 从 **-x** 面进入。
        // 这条在修之前**就是对的**（loor(1.0)=1 恰好一致），
        // 留着当回归网：修负方向时不能把正方向弄坏。
        let mut store = VoxelStore::default();
        let terrain = empty_terrain();
        store.set([1, 0, 0], Voxel::solid(VoxelId(5)), &terrain);

        let hit = raycast_voxels(&store, &terrain, [1.0, 0.5, 0.5], [1.0, 0.0, 0.0], 10.0)
            .expect("起点已在格 1 内");
        assert_eq!(hit.pos, [1, 0, 0]);
        assert_eq!(hit.normal, [-1, 0, 0], "从 -x 面进入");
    }
}
