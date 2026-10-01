//! 角色精灵表：**真实素材 + 数据驱动的网格布局**。
//!
//! ## 素材来源与许可
//!
//! `hero_walk_rgba.rs` 由 OpenGameArt 的
//! [`Base character spritesheet 16x16`](https://opengameart.org/content/base-character-spritesheet-16x16)
//! 转码而来：**CC0，无需署名**（作者原话 "Use it however you want, no credit needed."）。
//!
//! ## 为什么把 PNG 转成 Rust 源码
//!
//! 原始 RGBA 字节用 `Image::new` 就能建纹理，**不需要 `image` 解码依赖**。
//! 项目已有先例：CJK 字体走 `include_bytes!` 烘焙进二进制。
//! 代价是换素材要重新转码——对"内容用 `.ron`、素材烘焙"这个取舍是划算的。
//!
//! ## 布局是**数据**，不是写死的常量
//!
//! [`SheetLayout`] 描述"这张表怎么切"：格子边长、行数、每行几帧、每帧多久。
//! 换一张网格不同的素材只改这里的数字，**不改任何系统逻辑**
//! （`ActorVisual` 按 `row`/`frame` 查矩形，不关心表长什么样）。
//!
//! ## 行 → 动作的映射
//!
//! 素材的 3 行分别是"正面 / 背面 / 侧面"的走路循环。映射写在 [`SheetLayout::rows`] 里：
//!
//! | 行 | 动作 | 用途 |
//! |---|---|---|
//! | 0 | 正面 | 默认待机 / 朝南 |
//! | 1 | 背面 | 朝北 |
//! | 2 | 侧面 | 朝东 / 朝西（朝西时水平翻转） |

use bevy::image::Image;
use bevy::prelude::*;

/// 一格精灵的边长（像素）。
pub const FRAME_SIZE: u32 = 16;

/// 一个动作在表里的**一行**。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SheetRow {
    /// 行号（0 起）。
    pub row: u32,
    /// 这一行有几帧（走路循环）。
    pub frames: u32,
    /// 每秒播几帧。
    pub frames_per_second: f32,
    /// 播放时要不要水平翻转（同一行复用给"朝西"）。
    pub flip_x: bool,
}

impl SheetRow {
    /// 造一行。
    pub const fn new(row: u32, frames: u32, fps: f32) -> Self {
        Self {
            row,
            frames,
            frames_per_second: fps,
            flip_x: false,
        }
    }

    /// 加上"水平翻转"。
    pub const fn flipped(mut self) -> Self {
        self.flip_x = true;
        self
    }
}

/// 精灵表在世界里的**摆放方式**（数据）。
///
/// ## `world_size` **是**世界单位（这条以前写错了，已改正）
///
/// 早先这里写着"`world_size` 不是世界单位，实测约 2.4 像素/单位，
/// 与算术差了 8.6 倍，**没有找到解释**"。
///
/// **那个结论是错的**：`SpriteMesh::custom_size` 的单位**就是世界单位**
/// （`bevy_sprite` 的源码里 `custom_size` 优先于 `rect.size()`）。
/// 当时的"2.4 像素/单位"是把**两套不同的量**混在一起算出来的：
/// 一边用截图像素、一边用世界单位。
///
/// 现在的换算是干净的：
///
/// ```text
/// 屏幕像素 = world_size × (窗口宽度像素 / view_width)
/// ```
///
/// 实测核对（`view_width = 64`、1280 窗口 ⇒ 20 像素/单位）：
/// `world_size = 26` ⇒ 角色约 **20×28 像素**，约 1 格宽。
///
/// ## 改这个数之前先想清楚"和哪个取景配对"
///
/// `world_size` 与 `CameraConfig::view_width` 是**一对**：
/// 只改一个，角色就会相对格子变得过大或过小。
/// 参照系（Elins）：地面格 64 宽 × 32 高，角色精灵 32×48
/// （即角色约占一格的 **0.5 宽 × 1.5 高**）。
///
/// 标定方法与实测记录在 `work/TODO.md`。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SheetPlacement {
    /// 精灵的世界尺寸（**世界单位**，1 单位 = 1 个体素）。
    ///
    /// 直接喂给 `SpriteMesh::custom_size`。
    pub world_size: f32,
    /// 精灵中心离地高度（世界单位）。
    pub height: f32,
}

impl Default for SheetPlacement {
    fn default() -> Self {
        Self {
            // 见类型文档：实测标定值，对应约 130 像素高。
            world_size: 115.0,
            height: 1.6,
        }
    }
}

/// 精灵表的**网格布局**（数据）。
///
/// 换素材只改这里：格子多大、几行、每行几帧、多快。
#[derive(Debug, Clone, PartialEq)]
pub struct SheetLayout {
    /// 每格边长（像素）。
    pub frame_size: u32,
    /// 表有几列。
    pub columns: u32,
    /// 行 → 动作。
    pub rows: Vec<SheetRow>,
    /// 在世界里的摆放方式。
    pub placement: SheetPlacement,
}

impl Default for SheetLayout {
    /// 默认 = 内置的 `hero_walk_rgba`：16×16 的格子，3 行各 3 帧。
    fn default() -> Self {
        Self {
            frame_size: FRAME_SIZE,
            columns: 8,
            rows: vec![
                SheetRow::new(0, 3, 6.0),
                SheetRow::new(1, 3, 6.0),
                SheetRow::new(2, 3, 6.0),
            ],
            placement: SheetPlacement::default(),
        }
    }
}

impl SheetLayout {
    /// 某一行的描述（行号越界就回退到第 0 行，**不 panic**：渲染不该因为配置越界而崩）。
    pub fn row(&self, row: u32) -> SheetRow {
        self.rows
            .get(row as usize)
            .copied()
            .or_else(|| self.rows.first().copied())
            .unwrap_or(SheetRow::new(0, 1, 0.0))
    }

    /// 行数。
    pub fn row_count(&self) -> u32 {
        self.rows.len() as u32
    }

    /// 按行号 + 帧号算出**源图里的像素矩形**（`Sprite::rect` 用）。
    ///
    /// 帧号对帧数取模：调用方不必自己保证不越界。
    pub fn frame_rect(&self, row: u32, frame: u32) -> Rect {
        let row = self.row(row);
        let frames = row.frames.max(1);
        let column = frame % frames;
        let size = self.frame_size as f32;
        let x = column as f32 * size;
        let y = row.row as f32 * size;
        Rect::new(x, y, x + size, y + size)
    }
}

/// 已建好的精灵表（**Resource**）。
#[derive(Resource, Debug, Clone)]
pub struct ActorSheet {
    /// 贴图句柄（内置素材转成的纹理）。
    pub image: Handle<Image>,
    /// 网格布局。
    pub layout: SheetLayout,
    /// 源图宽度（像素）——测试与自检用。
    pub width: u32,
    /// 源图高度（像素）。
    pub height: u32,
}

impl ActorSheet {
    /// 在世界里的摆放方式（从内容来）。
    ///
    /// 做成方法而不是公开字段：`layout` 已经是公开的，再暴露一个字段会让
    /// "摆放参数从哪来"有两条路。这里只留一条。
    pub fn placement(&self) -> SheetPlacement {
        self.layout.placement
    }

    /// 某一帧的像素矩形。
    pub fn frame_rect(&self, row: u32, frame: u32) -> Rect {
        self.layout.frame_rect(row, frame)
    }

    /// 某一帧要不要水平翻转。
    pub fn flip_x(&self, row: u32) -> bool {
        self.layout.row(row).flip_x
    }

    /// 某一行的帧率。
    pub fn frames_per_second(&self, row: u32) -> f32 {
        self.layout.row(row).frames_per_second
    }

    /// 某一行的帧数。
    pub fn frames(&self, row: u32) -> u32 {
        self.layout.row(row).frames.max(1)
    }
}

/// 精灵表素材的路径（**相对 Bevy 的资产根目录**，即工作区根的 `assets/`）。
///
/// ## 为什么是路径常量而不是烘焙进源码的字节
///
/// 之前这里走的是"转码成 `.rs` 里的 `&[u8]`"（947 行的字节数组）。
/// 那样做的唯一理由是"不想开 `png` 特性"，而代价是：
/// **换素材要重跑脚本再重编译**，而且没人能直接看出图里是什么。
///
/// 现在用 Bevy 的标准做法：素材放 `assets/`，`AssetServer` 按路径加载。
/// 换图 = 换文件，不用动代码。
pub const HERO_SHEET_PATH: &str = "sprites/hero.png";

/// 加载精灵表贴图。
///
/// **异步**：返回句柄立即可用，但像素要等加载完。
/// 调用方（[`build_actor_sheet`]）负责等 `Assets<Image>` 里出现它再建表。
///
/// [`build_actor_sheet`]: super::build_actor_sheet
pub fn load_sheet_image(assets: &AssetServer) -> Handle<Image> {
    assets.load(HERO_SHEET_PATH)
}
#[cfg(test)]
mod tests {
    use super::*;

    /// **素材文件的尺寸必须容得下布局**。
    ///
    /// 从"检查烘焙字节数"改成"检查素材文件"：换素材时最容易出的错就是
    /// **新图的格子尺寸/列数不一样**，那样 `frame_rect` 会切到别的地方 ——
    /// 画面上是"角色缺一块"或者"角色是别人的头"，而且**不报错**。
    ///
    /// 这条读磁盘上的真文件（`include_bytes!` 只为拿尺寸而解析 PNG 头，
    /// 不建纹理）—— 所以它同时守住"文件确实在"。
    #[test]
    fn the_shipped_sheet_file_is_large_enough_for_the_layout() {
        // 直接读文件头拿尺寸：不必启用 PNG 解码，也不会依赖 `AssetServer`。
        const BYTES: &[u8] = include_bytes!("../../../../assets/sprites/hero.png");
        let (w, h) = png_dimensions(BYTES).expect("hero.png 该是个合法 PNG");

        let layout = SheetLayout::default();
        assert!(
            layout.columns * layout.frame_size <= w,
            "布局要 {} 列 × {} 像素 = {} 像素宽，而图只有 {w} 宽",
            layout.columns,
            layout.frame_size,
            layout.columns * layout.frame_size
        );
        assert!(
            layout.row_count() * layout.frame_size <= h,
            "布局要 {} 行 × {} 像素 = {} 像素高，而图只有 {h} 高",
            layout.row_count(),
            layout.frame_size,
            layout.row_count() * layout.frame_size
        );

        // 素材不该是空文件（空图会让角色完全不可见，且不报错）。
        assert!(w > 0 && h > 0, "精灵表尺寸不能是 0");
    }

    /// 从 PNG 的 IHDR 块读出宽高。
    ///
    /// 只为测试用：不想为了"读个尺寸"而拉进整套解码。
    fn png_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
        // 8 字节签名 + 4 长度 + 4 类型 "IHDR" + 4 宽 + 4 高
        const SIG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        if bytes.len() < 24 || &bytes[0..8] != SIG || &bytes[12..16] != b"IHDR" {
            return None;
        }
        let read =
            |i: usize| u32::from_be_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
        Some((read(16), read(20)))
    }
}
