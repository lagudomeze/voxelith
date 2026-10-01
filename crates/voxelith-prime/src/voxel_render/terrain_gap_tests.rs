//! **逐列顶面**测试：真实 `world.ron` 高度图下，每一列最高实心块的上表面
//! 必须真的被网格覆盖。
//!
//! ## 为什么需要这条
//!
//! 第 5 轮用"按面朝向上三色"诊断出：地形上有约 **7.5%** 的画面是**纯清屏色**
//! （顶面占 87.8%、侧面 2.3%、底面 0.3%，三色之和 90.4%）。
//!
//! 已排除的：世界边缘（半径 3→5 反而更差）、近平面裁剪（`near = 0.1`）、
//! 区块边界（不规则、不沿 32 格网格对齐）。
//!
//! 而地形起伏只有 **±2 格** —— 所以那些缺口**不可能是"向下的洞"**
//! （最大落差 2 格，看不到底）。只剩一个解释：**某些顶面没被生成**。
//!
//! 现有的网格化测试都只断言"给定一块地形，面数对不对"，
//! **没有一条拿真实高度图逐列核对过**。这条补上那个洞。

use bevy::prelude::*;

use super::atlas::{AtlasImage, BlockTexture, FaceTexture, Pattern};
use super::mesher::{ChunkMesh, greedy_mesh};
use voxelith_axiom::world::{
    CHUNK_SIZE, ChunkPos, TerrainParams, VoxelAppearance, VoxelId, VoxelNames, VoxelPalette,
    VoxelStore,
};

/// 从真实的 `world.ron` 造一套配置（**与游戏同源**）。
fn real_setup() -> (TerrainParams, VoxelPalette, AtlasImage, VoxelStore) {
    let raw = crate::content::parse_raw().expect("world.ron 该配好");
    let mut names = VoxelNames::default();
    let mut palette = VoxelPalette::default();
    let mut textures = Vec::new();
    for block in &raw.world.blocks {
        names.register(&block.id);
        palette.push(VoxelAppearance {
            atlas_index: 0,
            opaque: block.opaque,
        });
        textures.push(BlockTexture {
            side: FaceTexture {
                base: block.side.base,
                pattern: Pattern::Solid,
            },
            top: block.top.map(|t| FaceTexture {
                base: t.base,
                pattern: Pattern::Solid,
            }),
            bottom: None,
            opaque: block.opaque,
        });
    }
    let t = &raw.world.terrain;
    let lookup = |name: &str| names.id(name).unwrap_or(VoxelId(0));
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
    let mut images = Assets::<Image>::default();
    let atlas = AtlasImage::build(&mut images, &textures);
    let mut store = VoxelStore::default();
    voxelith_axiom::world::load_around(&mut store, &terrain, ChunkPos::default(), 2);
    (terrain, palette, atlas, store)
}

/// 网格里所有**朝上的四边形**，表示为 `(y, min_x, max_x, min_z, max_z)`（格坐标）。
///
/// ⚠️ **不能查"顶点是否落在格中心"**：贪心网格会把同一层同色的面**合并**成
/// 一个大四边形，只记 4 个**角**顶点。第一版就是这么写的，结果 **1024 列全部
/// 被判为"缺失"**——典型的假阴性。正确做法是查**点是否落在四边形内部**。
///
/// 这些四边形都是轴对齐的（网格化只在 `(u, v)` 平面里铺），所以包围盒就够。
fn top_quads(mesh: &ChunkMesh) -> Vec<(i32, i32, i32, i32, i32)> {
    let mut out = Vec::new();
    // `push_quad` 一次推 4 个顶点，所以每 4 个一组就是一个四边形。
    for (index, quad) in mesh.positions.chunks(4).enumerate() {
        if quad.len() < 4 {
            continue;
        }
        // 法线取自这一组的第 0 个（同一 quad 共享法线）。
        if mesh.normals[index * 4][1] < 0.5 {
            continue;
        }
        let mut min_x = i32::MAX;
        let mut max_x = i32::MIN;
        let mut min_z = i32::MAX;
        let mut max_z = i32::MIN;
        for p in quad {
            min_x = min_x.min(p[0].round() as i32);
            max_x = max_x.max(p[0].round() as i32);
            min_z = min_z.min(p[2].round() as i32);
            max_z = max_z.max(p[2].round() as i32);
        }
        let y = quad[0][1].round() as i32;
        out.push((y, min_x, max_x, min_z, max_z));
    }
    out
}

/// 某个 `(x, z)` 是否被"高度为 `y` 的朝上四边形"盖住。
///
/// 用**格中心** `(x + 0.5, z + 0.5)` 判定：格中心一定落在覆盖它的四边形内部
/// （合并出来的四边形边界都在整数格上）。用 2 倍坐标避免浮点。
fn covered(quads: &[(i32, i32, i32, i32, i32)], x: i32, y: i32, z: i32) -> bool {
    let cx = 2 * x + 1;
    let cz = 2 * z + 1;
    quads.iter().any(|&(qy, min_x, max_x, min_z, max_z)| {
        qy == y && 2 * min_x <= cx && cx <= 2 * max_x && 2 * min_z <= cz && cz <= 2 * max_z
    })
}

/// **核心断言**：真实高度图下，每一列的最高实心块的上表面都必须被网格覆盖。
/// **相机视野覆盖的那批区块**：逐列核对。
///
/// 单个区块（32×32 格）远小于相机视野（约 62×35 世界单位），
/// 所以只测中心那个区块是不够的 —— 视野跨的是 3×3。
///
/// 用参数化的方式跑 9 个区块，**全部**都要通过。
/// **完整检查**：每一个"上方是空气"的实心格，它的 `+Y` 面都必须在网格里。
///
/// 比 [`every_chunk_in_the_camera_view_has_all_its_surface_faces`] 严格得多：
/// 那条只查"每列最高的那一格"（= 地表）。如果地形生成出了**坑或洞**，
/// 坑底的格子上方也是空气，也该有面 —— 但那条测试看不见它们。
///
/// 这条把所有高度的这种格子都算上，能抓到"坑里没有几何"。
#[test]
fn every_upward_facing_voxel_has_its_face() {
    let (terrain, palette, atlas, store) = real_setup();

    let mut exposed = 0usize;
    let mut problems = Vec::new();
    for cx in -1..=1 {
        for cz in -1..=1 {
            let chunk = ChunkPos::new(cx, 0, cz);
            let mesh = greedy_mesh(chunk, |pos| store.get(pos, &terrain), &palette, &atlas);
            let quads = top_quads(&mesh);
            let origin = chunk.origin();
            for dx in 0..CHUNK_SIZE {
                for dz in 0..CHUNK_SIZE {
                    let x = origin[0] + dx as i32;
                    let z = origin[2] + dz as i32;
                    // 该区块覆盖的全部高度。
                    for y in origin[1]..origin[1] + CHUNK_SIZE as i32 {
                        if !store.get([x, y, z], &terrain).is_solid() {
                            continue;
                        }
                        // 上方是空气 ⇒ 这个面可见 ⇒ 必须在网格里。
                        if store.get([x, y + 1, z], &terrain).is_solid() {
                            continue;
                        }
                        exposed += 1;
                        if !covered(&quads, x, y, z) {
                            problems.push((x, y, z));
                        }
                    }
                }
            }
        }
    }

    assert!(exposed > 2000, "该有足够多的暴露面，实际 {exposed}");
    assert!(
        problems.is_empty(),
        "{exposed} 个上方是空气的实心格里有 {} 个缺面（前 10 个：{:?}）",
        problems.len(),
        &problems[..problems.len().min(10)]
    );
}
/// **侧面**检查：每一个"侧面邻居是空气"的实心格，它的那个侧向面都必须在网格里。
///
/// ## 为什么必须单独查侧面
///
/// 前面的测试只查了 `+Y`（朝上）的面。但等距视角下**侧面同样大面积可见**——
/// 而且如果侧面的朝向算错（绕序写成朝里），它会**被背面剔除**，
/// 于是凸起的方块周围就露出一圈天空（清屏色），看起来像"地形有洞"。
///
/// 这一条按四个水平方向逐面核对：网格里必须存在一个**法线朝外**的四边形，
/// 覆盖该格那个方向的整片面积。
#[test]
fn every_exposed_side_face_exists_and_points_outward() {
    let (terrain, palette, atlas, store) = real_setup();

    // 四个水平方向：`(法线, u 轴, v 轴)`。
    let dirs: [(i32, i32, i32); 4] = [(1, 0, 0), (-1, 0, 0), (0, 0, 1), (0, 0, -1)];

    let mut exposed = 0usize;
    let mut problems: Vec<([i32; 4], i32)> = Vec::new();
    for cx in -1..=1 {
        for cz in -1..=1 {
            let chunk = ChunkPos::new(cx, 0, cz);
            let mesh = greedy_mesh(chunk, |pos| store.get(pos, &terrain), &palette, &atlas);
            let origin = chunk.origin();
            for dx in 0..CHUNK_SIZE {
                for dz in 0..CHUNK_SIZE {
                    let x = origin[0] + dx as i32;
                    let z = origin[2] + dz as i32;
                    for y in origin[1]..origin[1] + CHUNK_SIZE as i32 {
                        if !store.get([x, y, z], &terrain).is_solid() {
                            continue;
                        }
                        for (nx, ny, nz) in dirs {
                            let neighbour = [x + nx, y + ny, z + nz];
                            if store.get(neighbour, &terrain).is_solid() {
                                continue;
                            }
                            exposed += 1;
                            // 这个方向上该有一个法线朝 `(nx,0,nz)` 的四边形。
                            // 用"顶点法线"判定朝向，再核对它覆盖这一格。
                            let found = side_quad_covers(&mesh, (x, y, z), (nx, ny, nz));
                            if !found {
                                problems.push(([x, y, z, nx + nz * 2], y));
                            }
                        }
                    }
                }
            }
        }
    }

    // 阈值别拍脑袋定太高：地形起伏只有 ±2 格，暴露侧面本来就不多。
    // 第一版写 2000，结果**测试因为自己的阈值失败**（实际 1077），
    // 真正的 problems 断言根本没跑到。
    assert!(exposed > 500, "该有足够多的暴露侧面，实际 {exposed}");
    assert!(
        problems.is_empty(),
        "{exposed} 个暴露侧面里有 {} 个在网格里找不到（前 10：{:?}）",
        problems.len(),
        &problems[..problems.len().min(10)]
    );
}

/// 网格里有没有一个**法线朝 `normal`** 的面，盖住格子 `(x, y, z)` 在该方向上的那个面。
///
/// ## 判据：**逐三角形**判断"这个面的中心"是否落在它上面
///
/// 不再自己拼包围盒（那样写过两版、两次都把自己坑了），而是直接把
/// 已经被顶面测试验证过的思路搬过来：
///
/// 1. 算出这个面**在格坐标系下的中心**；
/// 2. 遍历每个三角形，用**重心坐标**判断那个点是否落在三角形内。
///
/// 三角形是网格里唯一不会骗人的东西 —— 合并 / 绕序 / 朝向都不会影响
/// "这个点是否在三角形里"。
fn side_quad_covers(mesh: &ChunkMesh, voxel: (i32, i32, i32), normal: (i32, i32, i32)) -> bool {
    let (x, y, z) = voxel;
    // 面所在的那个平面：法线正方向取格坐标 +1，负方向取格坐标本身。
    let along = if normal.0 + normal.1 + normal.2 > 0 {
        1.0
    } else {
        0.0
    };
    let point = Vec3::new(
        x as f32 + 0.5 + normal.0 as f32 * along,
        y as f32 + 0.5 + normal.1 as f32 * along,
        z as f32 + 0.5 + normal.2 as f32 * along,
    );

    for triangle in mesh.indices.chunks(3) {
        if triangle.len() < 3 {
            continue;
        }
        let idx = [
            triangle[0] as usize,
            triangle[1] as usize,
            triangle[2] as usize,
        ];
        // 朝向必须对：取这个三角形的顶点法线。
        let n = mesh.normals[idx[0]];
        if n[0].round() as i32 != normal.0
            || n[1].round() as i32 != normal.1
            || n[2].round() as i32 != normal.2
        {
            continue;
        }
        let a = Vec3::from(mesh.positions[idx[0]]);
        let b = Vec3::from(mesh.positions[idx[1]]);
        let c = Vec3::from(mesh.positions[idx[2]]);
        if point_in_triangle(point, a, b, c) {
            return true;
        }
    }
    false
}

/// 点是否落在三角形内（**含边界**，用重心坐标 + 容差）。
///
/// 点在三角形**所在平面**上时才有意义；调用方已经保证点在该平面上。
fn point_in_triangle(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> bool {
    let v0 = c - a;
    let v1 = b - a;
    let v2 = p - a;
    let dot00 = v0.dot(v0);
    let dot01 = v0.dot(v1);
    let dot02 = v0.dot(v2);
    let dot11 = v1.dot(v1);
    let dot12 = v1.dot(v2);
    let denom = dot00 * dot11 - dot01 * dot01;
    if denom.abs() < 1e-9 {
        return false;
    }
    let u = (dot11 * dot02 - dot01 * dot12) / denom;
    let v = (dot00 * dot12 - dot01 * dot02) / denom;
    const EPS: f32 = 1e-3;
    u >= -EPS && v >= -EPS && u + v <= 1.0 + EPS
}
