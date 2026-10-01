//! `dungeon_layout` 的测试。
//!
//! 单独一个文件是为了 **R26 / R93 的单文件 500 行上限** ——
//! 布局的规则测试（不重叠、边界铺满、走廊连通、旋转轴对齐）加起来很长，
//! 和实现混在一起会超。
//!
//! 用 `#[path]` 挂回 `dungeon_layout`，所以这里 `use super::*` 拿到的
//! 就是布局模块的全部内容。

use super::*;

use super::*;

fn spec(seed: u64) -> DungeonSpec {
    DungeonSpec { seed, ..default() }
}

/// **同一个种子必须产出完全一样的布局。**
///
/// 这条不是形式主义：布局可复现是排查的前提 ——
/// 如果每次跑出来的地牢都不同，"我看到的这张图有问题"就没法复现。
#[test]
fn the_same_seed_produces_the_same_layout() {
    for seed in [1u64, 42, 999, 20_260_101] {
        assert_eq!(
            generate(&spec(seed)),
            generate(&spec(seed)),
            "种子 {seed} 两次生成的结果不一致"
        );
    }
}

/// 不同的种子**应该**产出不同的布局（否则种子没起作用）。
#[test]
fn different_seeds_produce_different_layouts() {
    let a = generate(&spec(1));
    let b = generate(&spec(2));
    assert_ne!(a, b, "换种子该换布局 —— 否则随机数没接上");
}

/// **房间不许重叠。**
///
/// 重叠的话墙会插进别的房间里，看起来像渲染错误 —— 而那是布局的错。
#[test]
fn rooms_never_overlap() {
    for seed in 1..40u64 {
        let s = spec(seed);
        let rooms = place_rooms(&s, &mut Rng::new(s.seed));
        assert!(!rooms.is_empty(), "种子 {seed} 一个房间都没生成");
        for (i, a) in rooms.iter().enumerate() {
            for b in rooms.iter().skip(i + 1) {
                assert!(!a.overlaps(b), "种子 {seed}：房间 {a:?} 与 {b:?} 重叠了");
            }
        }
    }
}

/// 房间必须落在给定范围内。
#[test]
fn rooms_stay_inside_the_bounds() {
    let s = spec(7);
    let (lo, hi) = s.bounds;
    for room in place_rooms(&s, &mut Rng::new(s.seed)) {
        assert!(
            room.min.x >= lo.x && room.min.y >= lo.y,
            "房间 {room:?} 越出下界 {lo:?}"
        );
        assert!(
            room.max.x <= hi.x && room.max.y <= hi.y,
            "房间 {room:?} 越出上界 {hi:?}"
        );
    }
}

/// 每个房间的**内部每一格**都要有地板。
///
/// 少铺一格就是地上一个洞 —— 而在地上很难看出来是"少了一块"。
#[test]
fn every_room_cell_gets_a_floor() {
    let s = spec(3);
    let layout = generate(&s);
    let floors: std::collections::HashSet<IVec2> = layout
        .iter()
        .filter(|p| p.part == DungeonPart::Floor)
        .map(|p| p.cell)
        .collect();
    for room in place_rooms(&s, &mut Rng::new(s.seed)) {
        for x in room.min.x..=room.max.x {
            for y in room.min.y..=room.max.y {
                assert!(
                    floors.contains(&IVec2::new(x, y)),
                    "房间 {room:?} 的格子 ({x},{y}) 没有地板"
                );
            }
        }
    }
}

/// 每个房间的**边界每一格**都要有墙（或门）。
///
/// 漏一格就是墙上一个缺口 —— 视觉上像是"墙没放对"，很难查。
#[test]
fn every_room_boundary_gets_a_wall_or_door() {
    let s = spec(5);
    let layout = generate(&s);
    let walls: std::collections::HashSet<IVec2> = layout
        .iter()
        .filter(|p| {
            matches!(
                p.part,
                DungeonPart::Wall | DungeonPart::WallCorner | DungeonPart::Door
            )
        })
        .map(|p| p.cell)
        .collect();
    for room in place_rooms(&s, &mut Rng::new(s.seed)) {
        for cell in room.boundary() {
            assert!(
                walls.contains(&cell),
                "房间 {room:?} 的边界 {cell:?} 上没有墙也没有门"
            );
        }
    }
}

/// **走廊必须真的连通两个房间**（首尾对上，且相邻格只差一步）。
///
/// L 形路径写错的话会断成一截一截 —— 而那看起来像"走廊没铺完"。
#[test]
fn corridors_connect_their_endpoints_without_gaps() {
    let from = IVec2::new(-5, 3);
    let to = IVec2::new(4, -6);
    let path = l_path(from, to);
    assert_eq!(path.first(), Some(&from), "路径起点不对");
    assert_eq!(path.last(), Some(&to), "路径终点不对");
    for pair in path.windows(2) {
        let step = (pair[1] - pair[0]).abs();
        assert_eq!(
            step.x + step.y,
            1,
            "路径在 {:?} → {:?} 之间断了（必须逐格相邻）",
            pair[0],
            pair[1]
        );
    }
}

/// 所有旋转都是 **90° 的整数倍**（模块化件必须轴对齐）。
#[test]
fn every_rotation_is_axis_aligned() {
    for seed in 1..20u64 {
        for p in generate(&spec(seed)) {
            assert!(
                p.quarter_turns < 4,
                "旋转圈数 {} 越界（该是 0..3）",
                p.quarter_turns
            );
        }
    }
}

/// **南北墙不转、东西墙转 90°。**
///
/// 墙件未旋转时**沿着世界 X 延伸**（GLB 里是 4 宽 × 4.15 高、厚度朝 -Z）。
/// 所以：
/// - 南北墙（贴 `±Z` 边）本来就沿 X ⇒ **不转**；
/// - 东西墙（贴 `±X` 边）要沿 Z ⇒ **转 90°**。
///
/// 我第一版按"贴 +Z 边转 180°"写 —— 那会让南北墙**垂直于边**、插进房间。
/// 症状是"墙横七竖八"而不是"少了一堵墙"，所以值得钉住。
#[test]
fn north_south_walls_do_not_rotate_and_east_west_do() {
    let room = Room {
        min: IVec2::new(0, 0),
        max: IVec2::new(4, 4),
    };
    for x in 1..4 {
        assert_eq!(
            wall_rotation(&room, IVec2::new(x, 0)),
            0,
            "南墙（贴 -Z 边）该不转 —— 它本来就沿 X"
        );
        assert_eq!(
            wall_rotation(&room, IVec2::new(x, 4)),
            0,
            "北墙（贴 +Z 边）该不转"
        );
    }
    for y in 1..4 {
        assert_eq!(
            wall_rotation(&room, IVec2::new(0, y)),
            1,
            "西墙（贴 -X 边）该转 90° —— 要沿 Z"
        );
        assert_eq!(
            wall_rotation(&room, IVec2::new(4, y)),
            1,
            "东墙（贴 +X 边）该转 90°"
        );
    }
}

/// 四个角的墙角件**旋转各不相同**（否则会朝错方向、露出缝隙）。
#[test]
fn the_four_corners_get_four_distinct_rotations() {
    let room = Room {
        min: IVec2::new(0, 0),
        max: IVec2::new(4, 4),
    };
    let corners = [
        IVec2::new(0, 0),
        IVec2::new(4, 0),
        IVec2::new(4, 4),
        IVec2::new(0, 4),
    ];
    let turns: Vec<u8> = corners.iter().map(|c| corner_rotation(&room, *c)).collect();
    let unique: std::collections::HashSet<u8> = turns.iter().copied().collect();
    assert_eq!(unique.len(), 4, "四个角该有四种不同旋转，实际 {turns:?}");
}

/// 生成出来的墙**只用两种旋转**（南北 0°、东西 90°）——
/// 出现 180°/270° 说明又有地方按"朝向"而不是"沿哪条轴"在算了。
#[test]
fn straight_walls_only_use_two_rotations() {
    let turns: std::collections::HashSet<u8> = generate(&spec(11))
        .iter()
        .filter(|p| p.part == DungeonPart::Wall)
        .map(|p| p.quarter_turns)
        .collect();
    for turn in &turns {
        assert!(
            *turn == 0 || *turn == 1,
            "直墙出现了旋转 {turn} —— 只该有 0（南北）和 1（东西）"
        );
    }
}

/// 尺寸与中心算对（这两个被布局与测试都依赖）。
#[test]
fn room_geometry_is_consistent() {
    let room = Room {
        min: IVec2::new(-3, 2),
        max: IVec2::new(1, 8),
    };
    assert_eq!(room.width(), 5);
    assert_eq!(room.height(), 7);
    assert_eq!(room.center(), IVec2::new(-1, 5));
    assert!(room.contains(IVec2::new(-3, 2)), "该含左下角");
    assert!(room.contains(IVec2::new(1, 8)), "该含右上角");
    assert!(!room.contains(IVec2::new(2, 8)), "不该含界外");
}

/// 边界格子里**四角各只出现一次**（不重复）。
#[test]
fn the_boundary_lists_each_corner_once() {
    let room = Room {
        min: IVec2::ZERO,
        max: IVec2::new(3, 3),
    };
    let boundary = room.boundary();
    let unique: std::collections::HashSet<IVec2> = boundary.iter().copied().collect();
    assert_eq!(boundary.len(), unique.len(), "边界里有重复格");
    // 4x4 的框：4*4 - 2*2 = 12 格。
    assert_eq!(unique.len(), 12);
}
