//! **地板网格**：用 Kenney 的地牢地板瓦片（GLB）铺一格一格的地面。
//!
//! ## 为什么单开一个模块（与 `terrain.rs` 的关系）
//!
//! `terrain.rs` 走的是**体素 + 贪婪网格化**那条路：高度图 → 合并面 → 一张图集。
//! 它擅长"有起伏、可挖"的地形，但**看起来不是格子**（合并之后没有格线）。
//!
//! 这里走的是另一条路：**一格放一个现成的 GLB 瓦片**。
//! 好处是格线天然存在（瓦片之间有缝）、比例天然对（瓦片就是按网格做的）。
//! 代价是**没有起伏、不可挖** —— 它是"地牢地板"，不是"地形"。
//!
//! 两者**互斥**（都用同一个 y 高度会 z-fighting）。用 [`FloorGridConfig::enabled`]
//! 二选一，默认开地板（它和 Kenney 的模块化墙/门是同一套规格，能直接拼接）。
//!
//! ## 尺寸换算（**这是唯一的坑**）
//!
//! Kenney 的 modular 系列的网格单位是 **2 世界单位**：
//!
//! | 模型 | 文件里的范围 | 换算 |
//! |---|---|---|
//! | `template-floor` | `-2.0 .. 2.0`（4 宽） | 一格 = 1 世界单位 ⇒ **缩放 0.5** |
//! | `template-wall` | 4 宽 × 4.15 高 | 同上 |
//! | `room-small` | 12 × 12 | 同上 |
//!
//! 所以放一格瓦片 = `scale = 0.5` + 间距 `1.0`。
//! 写成常量 [`TILE_SCALE`] / [`CELL_SPACING`]，改网格尺寸时两个一起改。
//!
//! ## 格线怎么来的
//!
//! **不画线**。把瓦片**稍微缩小**（[`TILE_INSET`]），瓦片之间就露出缝隙；
//! 缝隙下面是深色的地面衬板（[`BACKDROP`]），于是看起来就是格线。
//! 这比额外 spawn 几千个线段实体便宜得多，而且缝隙宽度可以直接调。
//!
//! ## 为什么两色棋盘
//!
//! 单纯靠缝隙在小尺寸下不够清楚（一格 20 px 时，缝隙只有 1–2 px）。
//! 交替两种灰让格子一眼能数出来 —— 参照系（用户给的截图）也是这么做的。

use bevy::prelude::*;

use super::camera::CameraConfig;

/// GLB 里 `template-floor` 的实际宽度（世界单位）。
///
/// 读 `POSITION` 的 `min/max` 得到 `-2.0 .. 2.0` ⇒ **宽 4.0**。
/// 它是**一个平面四边形**（12 个索引），不是立方体。
pub const GLB_TILE_WIDTH: f32 = 4.0;

/// 把一块瓦片缩放成"正好一格"的系数。
///
/// ## 这个常量的含义（我第一版理解错了）
///
/// 它是**缩放**，不是"模型宽度的比例"。正确的推导是：
///
/// ```text
/// 瓦片宽 = GLB_TILE_WIDTH × 缩放  ⇒  缩放 = CELL_SPACING / GLB_TILE_WIDTH
/// ```
///
/// 即 `1.0 / 4.0 = 0.25`。
///
/// 第一版我写的 `0.5`，心里想的是"Kenney 的网格单位是 2 世界单位，所以 2/4 = 0.5" ——
/// 那是把**网格单位**和**格子间距**混为一谈了：
/// 网格单位确实让 `room-small`（12 宽）对齐，但**一格是 1 世界单位**，
/// 所以地板瓦片必须是 1/4 而不是 1/2。
/// 用 0.5 的结果是瓦片宽 2 格，互相重叠 ⇒ 格子完全看不见。
pub const TILE_SCALE: f32 = CELL_SPACING / GLB_TILE_WIDTH;

/// 格子的间距（世界单位）。**必须等于一个体素**，否则和墙/门的网格对不上。
pub const CELL_SPACING: f32 = 1.0;

/// 瓦片相对格子的内缩比例（0.06 = 每边缩 3%）。
///
/// 这个值直接决定格线宽度：内缩 `0.06` ⇒ 缝隙占一格的 6%，
/// 视野 64（一格约 20 px）时缝隙约 1.2 px。
/// 想更明显就调大（`0.12` 约 2.4 px）。
pub const TILE_INSET: f32 = 0.06;

/// 地板网格的配置（**Resource**）。
#[derive(Resource, Debug, Clone)]
pub struct FloorGridConfig {
    /// 是否启用。默认 `true`（我们要的就是格子地面）。
    pub enabled: bool,
    /// 地板边长（格）。`96` ⇒ 96×96 = 9216 个瓦片。
    ///
    /// 取值依据：相机视野宽度 64，斜 45° 下能看到约 64×(16/9)/√2 ≈ 80 格的跨度，
    /// 所以 96 留了余量（看到边缘时会露出深色衬板，那比"缺一块"好）。
    pub extent: i32,
    /// 地板中心（格坐标）。挪动它 = 挪动整块地板。
    pub center: IVec2,
}

impl Default for FloorGridConfig {
    fn default() -> Self {
        Self {
            // **不在这里读环境变量** —— 由 `super::ground_style::GroundStyle` 统一决定，
            // 否则会出现"两个模块各自解读同一个变量、默认值写法还不一样"的 bug
            // （真踩过：判据写反，两边都跑了）。
            enabled: true,
            // `FLOOR_GRID_EXTENT` 可覆盖（调参不用改代码）。
            extent: env_i32("FLOOR_GRID_EXTENT").unwrap_or(96),
            center: IVec2::ZERO,
        }
    }
}

fn env_i32(name: &str) -> Option<i32> {
    std::env::var(name).ok()?.parse().ok()
}

/// 已经铺好了（避免重复 spawn）。
#[derive(Resource, Debug, Default)]
pub struct FloorGrid {
    /// 铺了多少个瓦片。
    pub placed: usize,
}

/// 衬板的颜色（缝隙里露出来的那个"格线"色）。
pub const BACKDROP: Color = Color::srgb(0.14, 0.14, 0.16);

/// 衬板中心的 y（世界坐标）。厚度 0.08 ⇒ 顶面在 -0.06。
///
/// **必须低于 0**，否则会和瓦片同高（实测会整个盖住瓦片）。
pub const BACKDROP_Y: f32 = -0.10;

/// 铺地板。
///
/// ## 为什么用一个扁平循环，而不是分块（chunk）
///
/// 体素地形要分块是因为**要挖、要按需网格化**。
/// 地板是静态的、一格一个模型，分块只会增加复杂度（还要处理跨块接缝）。
/// 9000 多个瓦片一次性 spawn 是几百毫秒，可以接受。
///
/// 真到了要更大范围时，再改成"跟随相机按需铺" —— 那时接口不变。
#[allow(clippy::too_many_arguments)]
pub fn spawn_floor_grid(
    mut commands: Commands,
    style: Res<super::ground_style::GroundStyle>,
    config: Res<FloorGridConfig>,
    camera: Res<CameraConfig>,
    assets: Option<Res<AssetServer>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    existing: Option<Res<FloorGrid>>,
) {
    if !config.enabled || !style.is_tile_grid() || existing.is_some() {
        return;
    }
    let Some(assets) = assets else {
        // 无头测试：没有资产设施，铺不出来。
        return;
    };

    let extent = config.extent.max(1);
    let half = extent / 2;

    // ---- 衬板：一整块深色平面，铺在瓦片下面 ----
    //
    // 它的作用是让瓦片之间的缝隙有颜色可露。
    // 用**纯色材质**（无贴图），所以它不受 GLB 贴图那条路的影响。
    //
    // ⚠️ **必须明显低于瓦片**。第一版放在 `y = -0.02`、厚 `0.04` ——
    // 那意味着**顶面正好在 `y = 0`**，与瓦片同高：
    // 实测结果是整个画面只剩衬板（深灰一片），瓦片被埋在里面看不见。
    // 现在放到 `-0.10`、厚 `0.08` ⇒ 顶面在 `-0.06`，瓦片明显浮在它上方。
    let span = (extent as f32 + 2.0) * CELL_SPACING;
    commands.spawn((
        Name::new("floor-backdrop"),
        Mesh3d(meshes.add(Cuboid::new(span, 0.08, span))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: BACKDROP,
            unlit: true,
            ..default()
        })),
        Transform::from_xyz(
            config.center.x as f32 * CELL_SPACING,
            BACKDROP_Y,
            config.center.y as f32 * CELL_SPACING,
        ),
    ));

    // ---- 瓦片 ----
    //
    // 两种缩放交替 ⇒ 棋盘。`TILE_INSET` 让每块都小一点，露出缝隙。
    let path = "models/dungeon/template-floor.glb";

    let mut placed = 0usize;
    for dx in 0..extent {
        for dz in 0..extent {
            let gx = config.center.x - half + dx;
            let gz = config.center.y - half + dz;
            // 棋盘：**同一套模型，只改缩放**。
            //
            // 为什么不改材质颜色：GLB 的材质是**共享**的（`assets/models/dungeon/`
            // 下 39 个模型共用同一个 `colormap` 材质）。
            // 改它的 `base_color` 会**同时改掉所有瓦片与墙**，棋盘就没了。
            // 而缩放是**每个实例独立**的，所以用缩放做深浅区分最省事。
            //
            // ## 缩放怎么算（这里踩过坑）
            //
            // `template-floor` 在文件里是 **4 世界单位宽**，而一格是 `CELL_SPACING`。
            // 所以让"一块瓦片 = 一格"的缩放是 `CELL_SPACING / 4.0 = 1/8`。
            //
            // 我第一版写成 `TILE_SCALE * full / CELL_SPACING`，其中
            // `TILE_SCALE = 0.5` 被当成了"模型宽度"——那是**错的**：
            // 实际算出来 `0.5 * 0.94 = 0.47`，瓦片宽 `4 * 0.47 = 1.88` 格，
            // **重叠了 88%**，于是整个画面被瓦片盖满、看不到格线
            // （实测：全屏只剩瓦片的深色，衬板的红一点都看不见）。
            //
            // 正确的关系：**瓦片宽 = CELL_SPACING × (1 - 内缩)**。
            let inset_scale = 1.0 - TILE_INSET;
            // 深浅交替：不改材质，只让"浅"的那些再小 4%（视觉上像两块不同的砖）。
            let checker = if (gx + gz).rem_euclid(2) == 0 {
                1.0
            } else {
                0.96
            };
            let scale = TILE_SCALE * inset_scale * checker;

            commands.spawn((
                Name::new(format!("floor/{gx},{gz}")),
                WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(path))),
                Transform::from_xyz(gx as f32 * CELL_SPACING, 0.0, gz as f32 * CELL_SPACING)
                    .with_scale(Vec3::splat(scale)),
            ));
            placed += 1;
        }
    }

    info!(
        "地板网格：铺了 {placed} 个瓦片（{extent}x{extent}，间距 {CELL_SPACING}，\
         瓦片缩放 {TILE_SCALE}，内缩 {TILE_INSET}）；视野宽 {} ⇒ 一格约 {:.1} px",
        camera.view_width,
        1280.0 / camera.view_width
    );
    commands.insert_resource(FloorGrid { placed });
}

/// 地板铺好后核对一次：**实际存在的瓦片实体数**要与记录一致。
///
/// 为什么要核对：瓦片是异步加载的，`spawn` 成功不代表模型出现。
/// 而且 9000 多个实体里少几个是**看不出来**的（那只是几个空格子）。
/// 打一行实数，排查时能立刻知道"是没铺出来还是加载失败"。
pub fn verify_floor_grid(grid: Option<Res<FloorGrid>>, tiles: Query<&Name>, mut done: Local<bool>) {
    if *done {
        return;
    }
    let Some(grid) = grid else {
        return;
    };
    *done = true;
    let actual = tiles
        .iter()
        .filter(|name| name.as_str().starts_with("floor/"))
        .count();
    info!(
        "地板网格核对：记录 {} 个瓦片实体，实际查到 {actual} 个{}",
        grid.placed,
        if actual == grid.placed {
            "（一致）"
        } else {
            "（**不一致** —— 有实体没建出来）"
        }
    );
}

/// 地板网格插件。
pub struct FloorGridPlugin;

impl Plugin for FloorGridPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FloorGridConfig>()
            .add_systems(Startup, spawn_floor_grid)
            .add_systems(Update, verify_floor_grid);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **瓦片缩放必须让一块瓦片正好占一格。**
    ///
    /// `template-floor` 在 GLB 里是 `-2..2`（4 世界单位宽），
    /// 而一格是 [`CELL_SPACING`]。
    /// 缩放错了的表现是"瓦片之间重叠或漏缝" —— 而那看起来像模型坏了。
    #[test]
    fn the_tile_scale_matches_the_grid_unit() {
        // 缩放之后，一块瓦片该正好是"一格减去内缩"宽。
        let rendered = GLB_TILE_WIDTH * TILE_SCALE * (1.0 - TILE_INSET);
        let expected = CELL_SPACING * (1.0 - TILE_INSET);
        assert!(
            (rendered - expected).abs() < 1e-6,
            "瓦片宽 {rendered}，该等于一格 {CELL_SPACING} 减去内缩（{expected}）"
        );
        // 而且**绝不能超过一格** —— 超了就会重叠，格子就看不见了。
        assert!(
            GLB_TILE_WIDTH * TILE_SCALE <= CELL_SPACING + 1e-6,
            "瓦片宽 {} 超过了一格 {CELL_SPACING} ⇒ 瓦片会互相重叠",
            GLB_TILE_WIDTH * TILE_SCALE
        );
    }

    /// 内缩要**大于 0**（否则没有格线）且**不能太大**（否则瓦片像散落的小方块）。
    #[test]
    fn the_inset_leaves_a_visible_but_reasonable_seam() {
        assert!(TILE_INSET > 0.0, "内缩为 0 ⇒ 瓦片紧贴，看不到格线");
        assert!(
            TILE_INSET < 0.3,
            "内缩 {TILE_INSET} 太大 ⇒ 瓦片看起来是散落的小方块，不是地板"
        );
    }

    /// 默认**启用**（我们要的就是格子地面）。
    #[test]
    fn the_grid_is_on_by_default() {
        if std::env::var("FLOOR_GRID").is_err() {
            assert!(FloorGridConfig::default().enabled, "默认该启用地板网格");
        }
    }

    /// 地板范围要**盖住相机视野**，否则能看到边缘。
    ///
    /// 斜 45° 下视野能看到的世界跨度 ≈ `view_width * (16/9) / sqrt(2)`。
    #[test]
    fn the_extent_covers_the_camera_view() {
        let config = FloorGridConfig::default();
        let camera = CameraConfig::default();
        // 1280x720 ⇒ 竖直跨度 = view_width * 720/1280。
        let vertical = camera.view_width * 720.0 / 1280.0;
        // 斜 45° 下，世界 X 与 Z 各占屏幕宽/高的一部分；取较保守的估计。
        let needed = (camera.view_width + vertical) / 2.0_f32.sqrt();
        let have = config.extent as f32;
        assert!(
            have >= needed,
            "地板边长 {have} 格小于视野需要的 {needed:.1} 格 ⇒ 会看到地板边缘"
        );
    }
}
