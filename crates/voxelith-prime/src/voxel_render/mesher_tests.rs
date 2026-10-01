//! `mesher` 的单元测试（平铺成**一个**模块，不再嵌套 `mod`）。
//!
//! ## 为什么平铺
//!
//! 用 `#[path]` 挂在 `mesher` 模块下时，这个文件**本身就是** `mesher::tests`。
//! 如果再在里面写 `mod tests { use super::*; }`，`super` 就变成了 `mesher::tests`
//! 而不是 `mesher` —— 所有内部项的路径都要写成 `super::super::`，第一版就是这么
//! 搞错的（拆了两次都没拆干净）。
//!
//! 平铺之后 `super` 直接指向 `mesher`，路径写法与内联时完全一致。
//!
//! 单独成文件的原因是测试体量大于被测代码，塞在一起会越过 500 行上限（**R26**）。

use bevy::prelude::*;

use super::super::atlas::{AtlasImage, BlockTexture, FaceTexture, Pattern};
use super::*;
use voxelith_axiom::world::{TerrainParams, VoxelAppearance, VoxelId, VoxelStore};

/// **完全平**的地形：地表严格在 `y = 0`。
fn flat_terrain() -> TerrainParams {
    TerrainParams {
        base_height: 0,
        amplitude: 0.0,
        soil_depth: 2,
        surface: VoxelId(1),
        soil: VoxelId(2),
        deep: VoxelId(3),
        floor_y: -8,
        ..Default::default()
    }
}

/// 三张同色贴图 + 三个方块（测试只要"能查到格子"，不关心颜色）。
fn setup() -> (VoxelStore, VoxelPalette, AtlasImage) {
    let mut palette = VoxelPalette::default();
    for _ in 0..3 {
        palette.push(VoxelAppearance {
            atlas_index: 0,
            opaque: true,
        });
    }
    let mut images = Assets::<Image>::default();
    let blocks: Vec<BlockTexture> = (0..3)
        .map(|_| BlockTexture {
            side: FaceTexture {
                base: [128, 128, 128],
                pattern: Pattern::Solid,
            },
            opaque: true,
            ..Default::default()
        })
        .collect();
    (
        VoxelStore::default(),
        palette,
        AtlasImage::build(&mut images, &blocks),
    )
}

#[test]
fn a_flat_surface_merges_into_one_quad() {
    // **贪婪合并的核心断言**：完全平的地表，32×32 的顶面该压成 **1 个** quad。
    // 若这条挂了，说明合并被什么东西打断了（贴图不一致 / 掩码被切碎）。
    let terrain = flat_terrain();
    let (store, palette, atlas) = setup();
    let mesh = greedy_mesh(
        ChunkPos::new(0, 0, 0),
        |pos| store.get(pos, &terrain),
        &palette,
        &atlas,
    );
    assert_eq!(
        mesh.quads, 1,
        "完全平的地表该合并成 1 个顶面 quad，实际 {}",
        mesh.quads
    );
    assert_eq!(mesh.vertex_count(), 4);
    assert_eq!(mesh.triangle_count(), 2);
    for uv in &mesh.uvs {
        assert!(
            (0.0..=1.0).contains(&uv[0]) && (0.0..=1.0).contains(&uv[1]),
            "UV 越界：{uv:?}"
        );
    }
}

#[test]
fn a_flat_surface_tiles_without_gaps() {
    // **这条抓的是"缝"**：完全平的地表，顶面必须同高且铺满 32×32。
    // 连续表面 ⇒ 顶面顶点全部同高，且四个边界都被顶到。
    let terrain = flat_terrain();
    let (store, palette, atlas) = setup();
    let mesh = greedy_mesh(
        ChunkPos::new(0, 0, 0),
        |pos| store.get(pos, &terrain),
        &palette,
        &atlas,
    );

    let top_verts: Vec<[f32; 3]> = mesh
        .normals
        .iter()
        .enumerate()
        .filter(|(_, normal)| normal[1] > 0.5)
        .map(|(index, _)| mesh.positions[index])
        .collect();
    assert!(!top_verts.is_empty(), "平地表必须有 +Y 顶面");

    let heights: std::collections::BTreeSet<i32> = top_verts.iter().map(|p| p[1] as i32).collect();
    assert_eq!(
        heights.len(),
        1,
        "平整地表的顶面该在同一个高度，实际有这些高度：{heights:?}"
    );

    let xs: std::collections::BTreeSet<i32> = top_verts.iter().map(|p| p[0] as i32).collect();
    let zs: std::collections::BTreeSet<i32> = top_verts.iter().map(|p| p[2] as i32).collect();
    assert_eq!(
        (xs.iter().min(), xs.iter().max()),
        (Some(&0), Some(&CHUNK_SIZE)),
        "顶面该铺满整个区块的 x 范围"
    );
    assert_eq!(
        (zs.iter().min(), zs.iter().max()),
        (Some(&0), Some(&CHUNK_SIZE)),
        "顶面该铺满整个区块的 z 范围"
    );
}

#[test]
fn normals_point_away_from_the_solid_side() {
    let terrain = flat_terrain();
    let (store, palette, atlas) = setup();
    let mesh = greedy_mesh(
        ChunkPos::new(0, 0, 0),
        |pos| store.get(pos, &terrain),
        &palette,
        &atlas,
    );
    // 地表之上是空气 ⇒ 唯一能看见的是 **+Y 顶面**，法线必须朝上。
    assert!(
        mesh.normals.iter().any(|n| n == &[0.0, 1.0, 0.0]),
        "该有朝上的顶面法线：{:?}",
        &mesh.normals[..mesh.normals.len().min(4)]
    );
}

#[test]
fn every_visible_face_winds_outward() {
    // **绕序回归测试**：对每个三角形，几何法线（按索引手算）必须与顶点法线同向。
    // 曾经写死一套绕序，导致顶面被背面剔除、地形整片消失。
    let terrain = flat_terrain();
    let (store, palette, atlas) = setup();
    let mesh = greedy_mesh(
        ChunkPos::new(0, 0, 0),
        |pos| store.get(pos, &terrain),
        &palette,
        &atlas,
    );
    assert!(!mesh.indices.is_empty(), "该有三角形");
    for triangle in mesh.indices.chunks(3) {
        let [a, b, c] = [
            mesh.positions[triangle[0] as usize],
            mesh.positions[triangle[1] as usize],
            mesh.positions[triangle[2] as usize],
        ];
        let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let cross = [
            ab[1] * ac[2] - ab[2] * ac[1],
            ab[2] * ac[0] - ab[0] * ac[2],
            ab[0] * ac[1] - ab[1] * ac[0],
        ];
        let declared = mesh.normals[triangle[0] as usize];
        let dot = cross[0] * declared[0] + cross[1] * declared[1] + cross[2] * declared[2];
        assert!(
            dot > 0.0,
            "三角形绕序朝里（会被背面剔除）：几何法线 {cross:?} vs 顶点法线 {declared:?}"
        );
    }
}

#[test]
fn a_fully_buried_chunk_has_no_visible_faces() {
    // 反面对照：地面以下全实心 ⇒ 一个可见面都没有，调用方**不该建实体**。
    let terrain = flat_terrain();
    let (store, palette, atlas) = setup();
    let mesh = greedy_mesh(
        ChunkPos::new(0, -1, 0),
        |pos| store.get(pos, &terrain),
        &palette,
        &atlas,
    );
    assert!(mesh.is_empty(), "全埋住的区块不该有可见面");
}

/// **单变量测试床**：全场空气，只有**原点那一格是实心**。
///
/// 这样六个方向的面**全部可见**（周围都是空气），可以逐个核对绕序。
/// 之前只测过平地表（只有顶面），侧面绕序从没被覆盖过 —— 于是"台阶的立面
/// 朝里被背面剔除"这种错误能一路活到画面上，表现为地形上一片片黑洞。
fn single_voxel() -> (VoxelStore, VoxelPalette, AtlasImage) {
    let terrain = TerrainParams {
        base_height: -40,
        amplitude: 0.0,
        soil_depth: 1,
        surface: VoxelId(1),
        soil: VoxelId(2),
        deep: VoxelId(3),
        floor_y: -64,
        ..Default::default()
    };
    let mut store = VoxelStore::default();
    // 只放一格：周围自然是空气（生成出来的地表在 y = -40）。
    store.set([0, 0, 0], Voxel::solid(VoxelId(1)), &terrain);

    let mut palette = VoxelPalette::default();
    for _ in 0..3 {
        palette.push(VoxelAppearance {
            atlas_index: 0,
            opaque: true,
        });
    }
    let mut images = Assets::<Image>::default();
    let blocks: Vec<BlockTexture> = (0..3)
        .map(|_| BlockTexture {
            side: FaceTexture {
                base: [128, 128, 128],
                pattern: Pattern::Solid,
            },
            opaque: true,
            ..Default::default()
        })
        .collect();
    (store, palette, AtlasImage::build(&mut images, &blocks))
}

#[test]
fn all_six_faces_are_generated_for_an_isolated_voxel() {
    let (store, palette, atlas) = single_voxel();
    let terrain = TerrainParams {
        base_height: -40,
        amplitude: 0.0,
        soil_depth: 1,
        surface: VoxelId(1),
        soil: VoxelId(2),
        deep: VoxelId(3),
        floor_y: -64,
        ..Default::default()
    };
    let mesh = greedy_mesh(
        ChunkPos::new(0, 0, 0),
        |pos| store.get(pos, &terrain),
        &palette,
        &atlas,
    );
    // 孤立方块六个面全可见 ⇒ 6 个 quad。
    assert_eq!(
        mesh.quads, 6,
        "孤立方块该出 6 个面，实际 {}（少于 6 说明有面被静默剔掉）",
        mesh.quads
    );
    let mut normals: Vec<[f32; 3]> = Vec::new();
    for n in &mesh.normals {
        if !normals.contains(n) {
            normals.push(*n);
        }
    }
    assert_eq!(normals.len(), 6, "六个朝向都该出现：{normals:?}");
}

#[test]
fn every_face_winds_outward_not_inward() {
    let (store, palette, atlas) = single_voxel();
    let terrain = TerrainParams {
        base_height: -40,
        amplitude: 0.0,
        soil_depth: 1,
        surface: VoxelId(1),
        soil: VoxelId(2),
        deep: VoxelId(3),
        floor_y: -64,
        ..Default::default()
    };
    let mesh = greedy_mesh(
        ChunkPos::new(0, 0, 0),
        |pos| store.get(pos, &terrain),
        &palette,
        &atlas,
    );
    for triangle in mesh.indices.chunks(3) {
        let [a, b, c] = [
            mesh.positions[triangle[0] as usize],
            mesh.positions[triangle[1] as usize],
            mesh.positions[triangle[2] as usize],
        ];
        let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let cross = [
            ab[1] * ac[2] - ab[2] * ac[1],
            ab[2] * ac[0] - ab[0] * ac[2],
            ab[0] * ac[1] - ab[1] * ac[0],
        ];
        let declared = mesh.normals[triangle[0] as usize];
        let dot = cross[0] * declared[0] + cross[1] * declared[1] + cross[2] * declared[2];
        assert!(
            dot > 0.0,
            "三角形绕序朝里（会被背面剔除）：几何法线 {cross:?} vs 顶点法线 {declared:?}"
        );
    }
}

fn palette_and_atlas() -> (VoxelPalette, AtlasImage) {
    let mut palette = VoxelPalette::default();
    for _ in 0..3 {
        palette.push(VoxelAppearance {
            atlas_index: 0,
            opaque: true,
        });
    }
    let mut images = Assets::<Image>::default();
    let blocks: Vec<BlockTexture> = (0..3)
        .map(|_| BlockTexture {
            side: FaceTexture {
                base: [128, 128, 128],
                pattern: Pattern::Solid,
            },
            opaque: true,
            ..Default::default()
        })
        .collect();
    (palette, AtlasImage::build(&mut images, &blocks))
}

/// **解析化楼梯**：`surface_height = x`（每往 +x 走一格就升一格）。
///
/// 这样每个台阶的**顶面**与**立面**都必然存在，且位置可以算出来。
/// 之前只测过平地表，立面从来没被覆盖 —— 而画面上有洞的正是台阶地形。
fn staircase(chunk: ChunkPos) -> (VoxelStore, TerrainParams) {
    let terrain = TerrainParams {
        base_height: -40,
        amplitude: 0.0,
        soil_depth: 1,
        surface: VoxelId(1),
        soil: VoxelId(2),
        deep: VoxelId(3),
        floor_y: -64,
        ..Default::default()
    };
    let mut store = VoxelStore::default();
    // 直接往 store 里摆：一格宽的楼梯，只占 x 从 0 到 4。
    let origin = chunk.origin();
    for dx in 0..5 {
        for dz in 0..4 {
            let x = origin[0] + dx;
            let z = origin[2] + dz;
            // 从 -40 一直填到 y = x，形成逐级升高的台阶。
            for y in -40..=x {
                store.set([x, y, z], Voxel::solid(VoxelId(1)), &terrain);
            }
        }
    }
    (store, terrain)
}

#[test]
fn a_staircase_has_no_missing_faces() {
    let chunk = ChunkPos::new(0, 0, 0);
    let (store, terrain) = staircase(chunk);
    let (palette, atlas) = palette_and_atlas();
    let mesh = greedy_mesh(chunk, |pos| store.get(pos, &terrain), &palette, &atlas);

    // 每一个台阶（x = 1..4）在自己那一格上该有：
    //   - 一个顶面（法线 +Y）
    //   - 一个立面（法线 +X，因为右边比它高……不，右边更高时立面朝 -X）
    //
    // 这里断言的是"每个 x 台阶的水平顶面都在"，以及"台阶之间的立面没有丢"。
    let top_faces: Vec<[f32; 3]> = mesh
        .normals
        .iter()
        .enumerate()
        .filter(|(_, n)| n[1] > 0.5)
        .map(|(i, _)| mesh.positions[i])
        .collect();
    assert!(!top_faces.is_empty(), "楼梯必须顶面");

    // 顶面高度必须是**逐级变化**的：至少出现 5 个不同的 y。
    let heights: std::collections::BTreeSet<i32> = top_faces.iter().map(|p| p[1] as i32).collect();
    assert!(
        heights.len() >= 5,
        "楼梯的顶面该有 5 个不同高度，实际 {heights:?}（少了说明台阶没生成）"
    );

    // 立面：法线沿 ±X，且该出现在**台阶交界**上。
    let risers: Vec<[f32; 3]> = mesh
        .normals
        .iter()
        .enumerate()
        .filter(|(_, n)| n[0].abs() > 0.5)
        .map(|(i, _)| mesh.positions[i])
        .collect();
    assert!(
        !risers.is_empty(),
        "楼梯必须有立面（法线沿 ±X）—— 一个都没有说明立面被静默剔掉了"
    );

    // 每个三角形都必须朝外（这一条在**有立面**的地形上才有意义）。
    for triangle in mesh.indices.chunks(3) {
        let [a, b, c] = [
            mesh.positions[triangle[0] as usize],
            mesh.positions[triangle[1] as usize],
            mesh.positions[triangle[2] as usize],
        ];
        let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let cross = [
            ab[1] * ac[2] - ab[2] * ac[1],
            ab[2] * ac[0] - ab[0] * ac[2],
            ab[0] * ac[1] - ab[1] * ac[0],
        ];
        let declared = mesh.normals[triangle[0] as usize];
        let dot = cross[0] * declared[0] + cross[1] * declared[1] + cross[2] * declared[2];
        assert!(
            dot > 0.0,
            "楼梯里有三角形绕序朝里：几何法线 {cross:?} vs 顶点法线 {declared:?}"
        );
    }
}
