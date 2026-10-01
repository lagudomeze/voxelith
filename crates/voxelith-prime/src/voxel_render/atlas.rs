//! 方块贴图集：**运行时按配置程序化生成**，不需要美术资源。
//!
//! ## 为什么程序化
//!
//! 每格 16×16 的 MC 风格贴图本身就该由"颜色 + 图案"决定。写成
//!
//! ```ron
//! (name: "grass", face: "top", base: (r: 96, g: 148, b: 62), pattern: Speckle(amount: 22))
//! ```
//!
//! 比让内容作者去画 16×16 PNG 更省事，也让"加一种方块"仍然只改 `.ron`。
//! 真美术资源到位时，把 [`AtlasImage::build`] 换成 `AssetServer` 加载图集即可——
//! 网格顶点只认"图集格号"，两边一致就不影响下游。
//!
//! ## 一个方块最多三种面
//!
//! 侧面 / 顶面 / 底面可以各不相同（草方块就是典型：顶绿、侧棕、底棕）。
//! 网格化按面的朝向取对应贴图，索引计算见 [`AtlasImage::tile_index`]。

use bevy::asset::RenderAssetUsages;
use bevy::image::Image;
use bevy::prelude::*;
use voxelith_axiom::world::{
    BlockDef, TexturePattern, VoxelAppearance, VoxelId, VoxelNames, VoxelPalette,
};

use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

/// 每格贴图的边长（像素）。
pub const TILE: u32 = 16;

/// 图集网格的列数（行数按需增长）。
const COLUMNS: u32 = 4;

/// 一个方块用到的三种面。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BlockFace {
    /// 侧面。
    #[default]
    Side,
    /// 顶面（`+Y`）。
    Top,
    /// 底面（`-Y`）。
    Bottom,
}

impl BlockFace {
    /// 三种面的下标（贴图索引数组里用）。
    pub const ALL: [BlockFace; 3] = [BlockFace::Side, BlockFace::Top, BlockFace::Bottom];

    /// 在"一个方块三种面"里的下标。
    pub fn index(self) -> usize {
        match self {
            BlockFace::Side => 0,
            BlockFace::Top => 1,
            BlockFace::Bottom => 2,
        }
    }

    /// 从法线推"这是哪个面"（网格化用）。
    ///
    /// 只有 `±Y` 才区分顶/底；四个侧面共用一张贴图。
    pub fn from_normal(normal: [i32; 3]) -> Self {
        match normal[1] {
            1 => BlockFace::Top,
            -1 => BlockFace::Bottom,
            _ => BlockFace::Side,
        }
    }
}

/// 贴图图案（**配置项**）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Pattern {
    /// 纯色。
    Solid,
    /// 细碎噪点（泥土 / 石头）。
    Speckle {
        /// 明暗波动幅度（0–255）。
        amount: u8,
    },
    /// 水平条纹（木头 / 草叶）。
    Stripes {
        /// 条纹间距。
        spacing: u32,
        /// 明暗波动幅度。
        amount: u8,
    },
    /// 一层横线（草地的顶部边缘感）。
    Topped {
        /// 顶部几行用亮色。
        rows: u32,
        /// 亮多少。
        amount: u8,
    },
}

impl Default for Pattern {
    fn default() -> Self {
        Pattern::Solid
    }
}

/// 一个面的贴图定义。
#[derive(Debug, Clone, PartialEq)]
pub struct FaceTexture {
    /// 基色（sRGB 0–255）。
    pub base: [u8; 3],
    /// 图案。
    pub pattern: Pattern,
}

impl Default for FaceTexture {
    fn default() -> Self {
        Self {
            base: [255, 0, 255],
            pattern: Pattern::Solid,
        }
    }
}

/// 一个方块在内容里的外观（三种面 + 是否遮挡邻面）。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BlockTexture {
    /// 侧面。
    pub side: FaceTexture,
    /// 顶面（缺省时用侧面）。
    pub top: Option<FaceTexture>,
    /// 底面（缺省时用侧面）。
    pub bottom: Option<FaceTexture>,
    /// 是否遮挡邻面（透明的方块不该把邻面藏掉）。
    pub opaque: bool,
}

impl BlockTexture {
    /// 取某个面的贴图（没单独配就用侧面）。
    pub fn face(&self, face: BlockFace) -> &FaceTexture {
        match face {
            BlockFace::Side => &self.side,
            BlockFace::Top => self.top.as_ref().unwrap_or(&self.side),
            BlockFace::Bottom => self.bottom.as_ref().unwrap_or(&self.side),
        }
    }
}

/// 已生成的图集：一张贴图 + "哪一格是哪个面"的索引。
///
/// `tiles[block_id][face.index()]` = 图集格号；`u16::MAX` 表示没登记。
#[derive(Resource, Debug, Clone)]
pub struct AtlasImage {
    /// 贴图句柄。
    pub image: Handle<Image>,
    /// 图集列数。
    pub columns: u32,
    /// 图集行数。
    pub rows: u32,
    tiles: Vec<[u16; 3]>,
}

impl AtlasImage {
    /// 图集格号 → 该格左上角在贴图里的像素坐标。
    pub fn tile_origin(&self, tile: u16) -> (f32, f32) {
        let index = tile as u32;
        (
            (index % self.columns) as f32 * TILE as f32,
            (index / self.columns) as f32 * TILE as f32,
        )
    }

    /// 图集格号 → UV 矩形（**像素坐标**，`Sprite` / HUD 用）。
    ///
    /// 这是格的**边界**矩形（`16×16` 整格）。要喂给 `nearest` 采样时应当用
    /// [`Self::tile_uv`]——边界矩形会让右/下边正好落到隔壁格上。
    pub fn tile_rect(&self, tile: u16) -> Rect {
        let (x, y) = self.tile_origin(tile);
        Rect::new(x, y, x + TILE as f32, y + TILE as f32)
    }

    /// 图集格号 → **采样用**的 UV 区间（已归一化到 `0..1`，并内缩半个纹素）。
    ///
    /// ## ⚠️ 为什么要内缩半纹素（踩过一次很贵的坑）
    ///
    /// 一开始直接用格的**边界**算 UV：
    ///
    /// ```text
    /// u0 = 16/64 = 0.25   u1 = 32/64 = 0.5
    /// ```
    ///
    /// 右边界 `u1 = 0.5` 换算成像素正好是 `x = 32` —— **那是隔壁格的第 0 个像素**。
    /// `nearest` 采样在右/下边缘会取到邻居的颜色，于是**每一格都被右邻居与下邻居污染**。
    ///
    /// 表现：草方块顶面（图集格 1）被右边的泥土格（格 2）染成棕色，
    /// 整片地形看着就是"一堆棕色色块"，而**图集本身、网格化、绕序全是对的**。
    /// 这个 bug 极难查，因为"结构"测试全绿而颜色就是不对。
    ///
    /// 修法是经典做法：采样区间取**纹素中心**到**纹素中心**，
    /// 即两端各内缩 `0.5 / 图集边长`。
    pub fn tile_uv(&self, tile: u16) -> Rect {
        let rect = self.tile_rect(tile);
        let half_u = 0.5 / self.pixel_width();
        let half_v = 0.5 / self.pixel_height();
        Rect::new(
            rect.min.x / self.pixel_width() + half_u,
            rect.min.y / self.pixel_height() + half_v,
            rect.max.x / self.pixel_width() - half_u,
            rect.max.y / self.pixel_height() - half_v,
        )
    }

    /// 某个方块的某个面用哪一格。
    ///
    /// `block` 是 **`VoxelId`（1-based，`0` = 空气）**，与 [`VoxelNames`] / [`VoxelPalette`]
    /// 同一套编号。
    ///
    /// ## ⚠️ 这里踩过一次很贵的坑
    ///
    /// `tiles` 是按"`world.ron` 里第几个方块"push 出来的（**0-based 下标**），
    /// 曾经直接把 `VoxelId` 当这个下标用。`VoxelId` 改成 1-based 之后，
    /// **所有方块的贴图整体偏移一格**，而且最后一个方块直接越界。
    ///
    /// 越界返回 `None` 本身不致命，致命的是 [`visible_face`](super::mesher) 里
    /// `atlas.tile_index(..)?` —— **查不到就不出面**。于是整排面被静默剔掉，
    /// 画面出现一片片黑洞，而所有单元测试都是绿的（它们都用自己造的 0-based 图集）。
    ///
    /// 所以：**ID 与下标的换算只允许出现在这一个地方**，别在调用点手写 `- 1`。
    pub fn tile_index_for(&self, block: VoxelId, face: BlockFace) -> Option<u16> {
        if block == VoxelId(0) {
            return None;
        }
        let entry = self.tiles.get(block.0 as usize - 1)?;
        let tile = entry[face.index()];
        (tile != u16::MAX).then_some(tile)
    }

    /// 按 **0-based 下标** 查某个方块的某个面（`tiles` 的原始下标）。
    ///
    /// 只给"造表与自检"用；游戏路径请用 [`Self::tile_index_for`]。
    pub fn tile_index(&self, block: u16, face: BlockFace) -> Option<u16> {
        let entry = self.tiles.get(block as usize)?;
        let tile = entry[face.index()];
        (tile != u16::MAX).then_some(tile)
    }

    /// 图集总格数。
    pub fn tile_count(&self) -> u32 {
        self.columns * self.rows
    }

    /// 图集宽度（像素）。
    pub fn pixel_width(&self) -> f32 {
        (self.columns * TILE) as f32
    }

    /// 图集高度（像素）。
    pub fn pixel_height(&self) -> f32 {
        (self.rows * TILE) as f32
    }

    /// 按"方块顺序"生成图集（`blocks[i]` 的 ID 就是 `i + 1`，`0` 留给空气）。
    ///
    /// 返回图集与每格的像素数据。空列表也能安全处理（生成一张 1 格的空图）。
    pub fn build(images: &mut Assets<Image>, blocks: &[BlockTexture]) -> Self {
        let tile_count = (blocks.len() * BlockFace::ALL.len()).max(1) as u32;
        let rows = tile_count.div_ceil(COLUMNS).max(1);
        let width = COLUMNS * TILE;
        let height = rows * TILE;
        let mut pixels = vec![0_u8; (width * height * 4) as usize];

        let mut tiles: Vec<[u16; 3]> = Vec::with_capacity(blocks.len());
        for (block_index, block) in blocks.iter().enumerate() {
            let mut entry = [u16::MAX; 3];
            for face in BlockFace::ALL {
                let tile = (block_index * 3 + face.index()) as u32;
                entry[face.index()] = tile as u16;
                let origin = ((tile % COLUMNS) * TILE, (tile / COLUMNS) * TILE);
                // **面明暗在这一步烘焙进去**（见 `shade` 模块）：
                // 同一个方块的三张格分别是"顶面亮 / 侧面中 / 底面暗"。
                let shade = super::shade::face_shade(face);
                paint_tile(&mut pixels, width, origin, block.face(face), shade);
            }
            tiles.push(entry);
        }

        let image = Image::new(
            Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            pixels,
            TextureFormat::Rgba8UnormSrgb,
            // MAIN_WORLD：网格化不需要它，但图标 / HUD 会读；留着避免以后再来改。
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
        );

        Self {
            image: images.add(image),
            columns: COLUMNS,
            rows,
            tiles,
        }
    }
}

/// 把一个面的贴图画进图集。
///
/// `face_shade` 是**这个面朝向的固定明暗系数**（见 `shade` 模块），
/// 在生成贴图时乘进去 —— 这样立体感是**烘焙好的**，与光源、环境光都无关。
fn paint_tile(
    pixels: &mut [u8],
    width: u32,
    origin: (u32, u32),
    texture: &FaceTexture,
    face_shade: f32,
) {
    for y in 0..TILE {
        for x in 0..TILE {
            // 两个不同的东西，**名字必须分开**（踩过：重名导致面明暗被完全忽略）：
            // - `pattern_jitter`：图案自己的明暗抖动（每个像素不同）
            // - `face_shade`：这个**面朝向**的固定系数（整张格一样）
            let pattern_jitter = pattern_shade(texture.pattern, x, y);
            let [r, g, b] = texture.base.map(|channel| {
                let value = channel as f32 + pattern_jitter as f32;
                (value * face_shade).round().clamp(0.0, 255.0) as u8
            });
            let px = origin.0 + x;
            let py = origin.1 + y;
            let index = ((py * width + px) * 4) as usize;
            pixels[index] = r;
            pixels[index + 1] = g;
            pixels[index + 2] = b;
            pixels[index + 3] = 255;
        }
    }
}

/// 图案 → 这一像素的明暗偏移（**确定性**：同样的图案每次生成同样的贴图）。
fn pattern_shade(pattern: Pattern, x: u32, y: u32) -> i16 {
    match pattern {
        Pattern::Solid => 0,
        Pattern::Speckle { amount } => {
            // 用坐标哈希做"随机"：不用 RNG，免得每次生成不同。
            let hash = (x.wrapping_mul(73_856_093) ^ y.wrapping_mul(19_349_663)) >> 3;
            let unit = (hash % (2 * amount as u32 + 1)) as i16;
            unit - amount as i16
        }
        Pattern::Stripes { spacing, amount } => {
            let band = (y / spacing.max(1)) % 2;
            let jitter = ((x.wrapping_mul(2_654_435_761)) >> 28) as i16 % 3;
            let base = if band == 0 {
                amount as i16
            } else {
                -(amount as i16)
            };
            base + jitter - 1
        }
        Pattern::Topped { rows, amount } => {
            if y < rows.max(1) {
                amount as i16
            } else {
                let hash = (x.wrapping_mul(73_856_093) ^ y.wrapping_mul(19_349_663)) >> 4;
                (hash % 7) as i16 - 3
            }
        }
    }
}

// ------------------------------------------------------------------ 装配

// `AtlasPlugin` 曾在这里作为一个**空壳 Plugin** 存在（`build` 里只写 `let _ = app;`）。
// 它违反 **R12**（禁止空壳 Plugin），而且会让人以为"图集有插件可以装"——
// 实际上 `build_atlas` 是由 `VoxelRenderPlugin` 的启动链显式排序的（见下）。
// 已删除：想注册图集就去 `VoxelRenderPlugin` 的链里加，不要在这里加间接层。

/// 由内容（`world.ron` 的方块表）生成图集，并把方块表灌进 L0 的 `VoxelNames` / `VoxelPalette`。
///
/// **顺序是契约**：先注册名字与外观（数据层），再画贴图（表现层）——两边用同一套 ID。
/// 三个表的编号必须一致（`0` = 空气，真方块从 `1` 开始），否则"名字解析出的 ID"
/// 与"外观查到的贴图"会对不上。
pub fn build_atlas(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut palette: ResMut<VoxelPalette>,
    mut names: ResMut<VoxelNames>,
    content: Option<Res<crate::content::ContentData>>,
) {
    // 没有内容（无头测试 / 没装 `ContentPlugin`）就建一张空图集，让下游不 panic。
    let blocks: Vec<BlockTexture> = content
        .as_ref()
        .map(|content| content.world.blocks.iter().map(convert_block).collect())
        .unwrap_or_default();

    if let Some(content) = &content {
        for block in &content.world.blocks {
            // 名字与外观**按同一顺序**注册，于是两边算出同一个 ID。
            names.register(&block.id);
            palette.push(VoxelAppearance {
                atlas_index: 0,
                opaque: block.opaque,
            });
        }
    }

    let atlas = AtlasImage::build(&mut images, &blocks);
    info!(
        "体素图集：{} 种方块，{} 格",
        blocks.len(),
        atlas.tile_count()
    );
    commands.insert_resource(atlas);
}

/// 内容里的方块定义 → 表现层的贴图定义。
fn convert_block(block: &BlockDef) -> BlockTexture {
    BlockTexture {
        side: convert_face(block.side),
        top: block.top.map(convert_face),
        bottom: block.bottom.map(convert_face),
        opaque: block.opaque,
    }
}

/// 内容里的面定义 → 表现层的面定义（图案枚举同名，逐项搬过来）。
fn convert_face(face: voxelith_axiom::world::FaceTexture) -> FaceTexture {
    FaceTexture {
        base: face.base,
        pattern: match face.pattern {
            TexturePattern::Solid => Pattern::Solid,
            TexturePattern::Speckle { amount } => Pattern::Speckle { amount },
            TexturePattern::Stripes { spacing, amount } => Pattern::Stripes { spacing, amount },
            TexturePattern::Topped { rows, amount } => Pattern::Topped { rows, amount },
        },
    }
}

// ------------------------------------------------------------------ 测试

/// 单元测试（内容较多，单独成文件；见 `atlas_tests.rs`）。
#[cfg(test)]
#[path = "atlas_tests.rs"]
mod tests;
