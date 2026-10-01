//! 区块网格化：**贪婪合并**（**R71**）——同一面上相邻同种方块合成一个四边形。
//!
//! ## 为什么值得做
//!
//! 32³ 区块最多有 `32 × 32 × 6 ≈ 6k` 个可见面。整块实心的地面若逐面出四边形就是 6k 个 quad；
//! 贪婪合并后地表塌成 **1 个** quad。顶点数与 draw call 都降一个数量级。
//!
//! ## 算法（每轴两方向，共 6 趟）
//!
//! 对每个方向 `d`，沿 `d` 轴逐层切片，在切片上建一张 `CHUNK_SIZE²` 掩码：
//! 掩码记录"这一格要不要出**朝 d 的面**、用哪张图集格"。
//! 然后在掩码上找**同图集格的最大矩形**，出一个 quad 并把矩形里的格子划掉。
//!
//! ## 与数据层的关系
//!
//! 本模块**只读**体素（通过 `sample` 闭包），不碰 `AssetServer`、不建实体（**R72**）：
//! 纯计算 ⇒ 可以丢进 `AsyncComputeTaskPool`，也可以在测试里同步跑。
//! 世界坐标换算一律走 `world` 暴露的 `ChunkPos::origin`（**R107**），这里不自己乘 32。

use voxelith_axiom::world::{CHUNK_SIZE, ChunkPos, Voxel, VoxelPalette};

use super::atlas::{AtlasImage, BlockFace};

/// 六个面方向：`(轴, 符号)`。顺序即 `direction` 下标。
pub const DIRECTIONS: [(usize, i32); 6] = [
    (0, 1),  // +X
    (0, -1), // -X
    (1, 1),  // +Y（顶）
    (1, -1), // -Y（底）
    (2, 1),  // +Z
    (2, -1), // -Z
];

/// 面朝向 → 单位法线。
pub fn direction_normal(direction: usize) -> [i32; 3] {
    let (axis, sign) = DIRECTIONS[direction];
    let mut normal = [0; 3];
    normal[axis] = sign;
    normal
}

/// 一个方向轴之外的另两个轴（切片平面内的 `u` / `v`）。
fn slice_axes(axis: usize) -> (usize, usize) {
    match axis {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    }
}

/// 一个可见面：用哪张图集格。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Face {
    tile: u16,
}

/// 网格化产物：可直接塞进 `Mesh` 的三角形数据（**世界坐标**）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChunkMesh {
    /// 顶点位置（世界坐标）。
    pub positions: Vec<[f32; 3]>,
    /// 顶点法线。
    pub normals: Vec<[f32; 3]>,
    /// 顶点 UV（图集 UV，`0..1`）。
    pub uvs: Vec<[f32; 2]>,
    ///
    /// 立体感靠它而不是靠光照 —— MC 风格的"顶面最亮、侧面中等、底面最暗"
    /// 是**约定俗成**的美术规则，用固定系数才不会因为调光源而走样。
    /// 三角形索引。
    pub indices: Vec<u32>,
    /// 合并出了几个四边形。
    pub quads: usize,
}

impl ChunkMesh {
    /// 顶点数。
    pub fn vertex_count(&self) -> usize {
        self.positions.len()
    }

    /// 三角形数。
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// 什么都没有（全空气、或全被埋住的区块）。
    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }
}

/// 网格化一个区块。
///
/// - `chunk`：要网格化的区块坐标。
/// - `sample`：读**世界坐标**处的体素（调用方负责跨区块查邻居，接缝才不会出双面）。
/// - `palette`：哪个方块挡邻面。
/// - `atlas`：图集格号 → UV。
pub fn greedy_mesh(
    chunk: ChunkPos,
    mut sample: impl FnMut([i32; 3]) -> Voxel,
    palette: &VoxelPalette,
    atlas: &AtlasImage,
) -> ChunkMesh {
    let origin = chunk.origin();
    let mut mesh = ChunkMesh::default();

    for (direction, &(axis, sign)) in DIRECTIONS.iter().enumerate() {
        let (u_axis, v_axis) = slice_axes(axis);
        let mut normal = [0; 3];
        normal[axis] = sign;

        for slice in 0..CHUNK_SIZE {
            // 这一层有没有格子？没有就整层跳过（空气占了绝大部分）。
            let mut grid = vec![None::<Face>; (CHUNK_SIZE * CHUNK_SIZE) as usize];
            let mut any = false;
            for v in 0..CHUNK_SIZE {
                for u in 0..CHUNK_SIZE {
                    let mut local = [0; 3];
                    local[axis] = slice;
                    local[u_axis] = u;
                    local[v_axis] = v;
                    if let Some(visible) =
                        visible_face(&mut sample, palette, atlas, origin, local, normal)
                    {
                        grid[(v * CHUNK_SIZE + u) as usize] = Some(visible);
                        any = true;
                    }
                }
            }
            if !any {
                continue;
            }
            merge_layer(&mut mesh, &mut grid, direction, slice, axis, origin, atlas);
        }
    }
    mesh
}

/// 一格在世界坐标下的位置。
fn world_pos(origin: [i32; 3], local: [i32; 3]) -> [i32; 3] {
    [
        origin[0] + local[0],
        origin[1] + local[1],
        origin[2] + local[2],
    ]
}

/// 这一格在 `normal` 方向上要不要出面、用哪张贴图。
fn visible_face(
    sample: &mut impl FnMut([i32; 3]) -> Voxel,
    palette: &VoxelPalette,
    atlas: &AtlasImage,
    origin: [i32; 3],
    local: [i32; 3],
    normal: [i32; 3],
) -> Option<Face> {
    let here = sample(world_pos(origin, local));
    if here.is_air() {
        return None;
    }
    let neighbour = sample(world_pos(
        origin,
        [
            local[0] + normal[0],
            local[1] + normal[1],
            local[2] + normal[2],
        ],
    ));
    // 邻格挡光 ⇒ 这个面看不见。跨区块的邻居也走这里（由 `sample` 负责）。
    if palette.is_opaque(neighbour.id) {
        return None;
    }
    // **走 `tile_index_for`（接受 1-based `VoxelId`）**，不要手写 `id - 1`：
    // ID 与下标的换算只允许在 `atlas` 里出现一次。
    let tile = atlas.tile_index_for(here.id, BlockFace::from_normal(normal))?;
    Some(Face { tile })
}

/// 在一层掩码上做贪婪合并，把结果追加进 `mesh`。
fn merge_layer(
    mesh: &mut ChunkMesh,
    grid: &mut [Option<Face>],
    direction: usize,
    slice: i32,
    axis: usize,
    origin: [i32; 3],
    atlas: &AtlasImage,
) {
    let (u_axis, v_axis) = slice_axes(axis);
    let mut v = 0;
    while v < CHUNK_SIZE {
        let mut u = 0;
        while u < CHUNK_SIZE {
            let Some(face) = grid[(v * CHUNK_SIZE + u) as usize] else {
                u += 1;
                continue;
            };
            // 先往 +u 扩。
            let mut du = 1;
            while u + du < CHUNK_SIZE && grid[(v * CHUNK_SIZE + u + du) as usize] == Some(face) {
                du += 1;
            }
            // 再整行往 +v 扩（要求整行同贴图）。
            let mut dv = 1;
            'grow: while v + dv < CHUNK_SIZE {
                for offset in 0..du {
                    if grid[((v + dv) * CHUNK_SIZE + u + offset) as usize] != Some(face) {
                        break 'grow;
                    }
                }
                dv += 1;
            }
            // 划掉已合并的格子。
            for row in 0..dv {
                for column in 0..du {
                    grid[((v + row) * CHUNK_SIZE + u + column) as usize] = None;
                }
            }
            push_quad(
                mesh, face.tile, direction, slice, axis, u_axis, v_axis, u, v, du, dv, origin,
                atlas,
            );
            u += du;
        }
        v += 1;
    }
}

/// 一个矩形 → 4 顶点 + 2 三角形。
#[allow(clippy::too_many_arguments)]
fn push_quad(
    mesh: &mut ChunkMesh,
    tile: u16,
    direction: usize,
    slice: i32,
    axis: usize,
    u_axis: usize,
    v_axis: usize,
    u0: i32,
    v0: i32,
    du: i32,
    dv: i32,
    origin: [i32; 3],
    atlas: &AtlasImage,
) {
    let corner = |du_offset: i32, dv_offset: i32| -> [i32; 3] {
        let mut local = [0; 3];
        local[axis] = slice;
        local[u_axis] = u0 + du_offset;
        local[v_axis] = v0 + dv_offset;
        local
    };
    let corners = [corner(0, 0), corner(du, 0), corner(du, dv), corner(0, dv)];

    let normal = direction_normal(direction);
    // **用 `tile_uv`（已内缩半个纹素）**，不要用 `tile_rect` 的边界。
    //
    // 边界矩形的右/下边**正好落在隔壁格的第一个像素上**，`nearest` 采样会把邻居
    // 的颜色混进来。表现是"草方块顶面被旁边的泥土染成棕色"，而图集、网格化、
    // 绕序**全都是对的** —— 极难查。详见 `AtlasImage::tile_uv` 的说明。
    let rect = atlas.tile_uv(tile);
    // `y` 轴翻过来：UV 原点在左下，而图集的行是从上往下数的。
    let uvs = [
        [rect.min.x, rect.max.y],
        [rect.max.x, rect.max.y],
        [rect.max.x, rect.min.y],
        [rect.min.x, rect.min.y],
    ];

    let base = mesh.positions.len() as u32;
    for (index, local) in corners.iter().enumerate() {
        let world = world_pos(origin, *local);
        mesh.positions
            .push([world[0] as f32, world[1] as f32, world[2] as f32]);
        mesh.normals
            .push([normal[0] as f32, normal[1] as f32, normal[2] as f32]);
        mesh.uvs.push(uvs[index]);
    }

    // ---- 三角形绕序：**必须让几何法线朝外**，否则面会被背面剔除 ----
    //
    // 四个角的排列在 `(u, v)` 平面里是**逆时针**的。但世界空间的 `u × v` 方向取决于
    // 两个轴的手性：`(1,2) = Y×Z = +X` 是右手系，而 `(0,2) = X×Z = -Y` 是左手系。
    // 所以"一套绕序通吃六个方向"是不可能的——**一半的面会朝里**。
    //
    // 这里直接推导：算一遍 `e_u × e_v`，看它是不是等于该面的朝外法线；
    // 不等于就把三角形反过来。这样不依赖"人肉记住哪个轴对是右手系"。
    //
    // 踩过的坑：曾经写死 `[0,1,2, 0,2,3]`，结果**顶面（+Y）全部被剔掉**——
    // 表现是"地形完全不见"，而把 `cull_mode` 关掉又整个冒出来。
    let mut u_edge = [0_i32; 3];
    u_edge[u_axis] = 1;
    let mut v_edge = [0_i32; 3];
    v_edge[v_axis] = 1;
    let cross = [
        u_edge[1] * v_edge[2] - u_edge[2] * v_edge[1],
        u_edge[2] * v_edge[0] - u_edge[0] * v_edge[2],
        u_edge[0] * v_edge[1] - u_edge[1] * v_edge[0],
    ];
    let indices = if cross == normal {
        [base, base + 1, base + 2, base, base + 2, base + 3]
    } else {
        [base, base + 2, base + 1, base, base + 3, base + 2]
    };
    mesh.indices.extend_from_slice(&indices);
    mesh.quads += 1;
}

// ------------------------------------------------------------------ 测试
// ------------------------------------------------------------------ 测试

/// 单元测试（体量大于被测代码，单独成文件；见 `mesher_tests.rs`）。
#[cfg(test)]
#[path = "mesher_tests.rs"]
mod mesher_tests;
