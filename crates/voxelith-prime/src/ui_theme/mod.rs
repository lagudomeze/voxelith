//! **UI 主题**：把 Kenney `ui-pack-adventure` 的图块接到 Bevy UI 上。
//!
//! ## 这一层解决什么
//!
//! HUD 之前是**纯色矩形**（`BackgroundColor` + 描边 `Node`），看起来像调试界面。
//! 这里把 Kenney 的九宫格面板 / 进度条 / 按钮接进来，让 HUD 有真正的美术外观。
//!
//! ## 素材怎么进来的（**这是本模块最重要的一段**）
//!
//! **整张图集作为一个素材加载**（`assets/ui/kenney-adventure.png`，592×600），
//! 每个图块只是"图集里的一个矩形"，`ImageNode.rect` 用来选区域。
//!
//! 早先的做法是：写一个 Python 脚本把 11 个图块**裁出来转成 Rust 源码**
//! （`kenney_ui_rgba/` 里 2500 行字节数组），每个图块一张独立的 `Image`。
//! 那套做法的代价是：
//!
//! - **换素材要重跑脚本再重编译**；
//! - 代码库里躺着几千行没人读的字节；
//! - 每个图块一份 GPU 纹理（11 个小纹理，而不是 1 张图集）。
//!
//! 现在没有转码脚本，也没有生成的字节数组：**换素材 = 换文件**。
//! 唯一保留的是 `UiTile` 里的矩形坐标 —— 那是素材 XML 里的数据抄过来的，
//! 由 `atlas.rs` 的测试逐条对着 XML 核对。
//!
//! ## 九宫格（`Sliced`）是这里的关键
//!
//! 面板图块是 64×64 的**圆角带边框**贴图。直接拉伸会把圆角和边框拉变形，
//! 所以用 `NodeImageMode::Sliced(TextureSlicer)`：
//! 四角保持原尺寸、四边单向拉伸、中心双向拉伸。这样**任意面板尺寸都好看**。
//!
//! 边宽取 16px：64×64 的图块，四边各 16、中心 32 —— 与 Kenney 的设计一致
//! （他那套面板的边框正好占 1/4 边长）。

use bevy::prelude::*;
use bevy::sprite::TextureSlicer;

mod atlas;

pub use atlas::{UI_ATLAS_PATH, UiAtlas, UiTile, load_ui_atlas, log_loaded_atlas};

/// 面板图块的九宫格边宽（像素）。
///
/// 64×64 的图块，四边各 16、中心 32。**这个数必须与素材一致** ——
/// 写大了会把边框切掉一块，写小了会把圆角拉变形。
const PANEL_BORDER: f32 = 16.0;

/// 进度条图块是 16×32，左右各留 4px 做圆头，中间拉伸。
const BAR_BORDER: f32 = 4.0;

/// 面板底衬的暗色 tint。
///
/// **为什么需要它（踩过一次）**：Kenney 的面板图块是**不透明**的亮棕 / 亮灰。所以
///
/// - 给它加 `BackgroundColor` 是**没用的** —— 底色被不透明图块整个盖住；
/// - 浅色文字会因为"亮底 + 亮字"而**看不见**（实测：标题与资源名糊成一片）。
///
/// 正确做法是**压在 tint 上**：`ImageNode::color` 与像素相乘。
/// 边框仍在（乘出来的还是边框），底衬变暗，文字就跳出来了。
pub const PANEL_TINT: Color = Color::srgb(0.42, 0.36, 0.30);

/// 进度条不压暗：它本来就该是亮的。
pub const BAR_TINT: Color = Color::WHITE;

/// 一个九宫格的 `ImageNode`，从图集里选一个图块。
///
/// ## 用 Bevy 的 builder 链，而不是手写结构体字面量
///
/// `ImageNode` 提供了 `with_rect` / `with_mode` / `with_color`
/// （见 `bevy_ui::widget::image`），链式写法比 `ImageNode { .. }` 字面量更贴合官方用法，
/// 也免得以后 `ImageNode` 加字段时这里编译不过。
///
/// `TextureSlicer` 本身**没有**便捷构造器，用结构体字面量是官方写法
/// （`bevy_sprite::texture_slice::slicer`）。
///
/// `border` 是九宫格的边宽：面板图块用 [`PANEL_BORDER`]，按钮更小。
/// `tint` 与像素相乘 —— 见 [`PANEL_TINT`] 的说明。
pub fn sliced_node(atlas: &UiAtlas, tile: UiTile, border: f32, tint: Color) -> ImageNode {
    ImageNode::new(atlas.image.clone())
        // **从图集里选区域**：`rect` 是像素坐标，正是 `UiTile` 记的那份。
        .with_rect(tile.bevy_rect())
        .with_mode(NodeImageMode::Sliced(TextureSlicer {
            border: BorderRect::all(border),
            ..default()
        }))
        .with_color(tint)
}

/// 面板节点的便捷构造（九宫格，可给任意尺寸；已压暗到可读）。
pub fn panel(atlas: &UiAtlas, brown: bool) -> ImageNode {
    let tile = if brown {
        UiTile::PanelBrown
    } else {
        UiTile::PanelGrey
    };
    sliced_node(atlas, tile, PANEL_BORDER, PANEL_TINT)
}

/// 资源条的一对图块：`(填充, 外框)`，按序号循环三色。
///
/// 循环而不是写死：HUD 里每种资源一条，**加一种新资源不用改这里的代码**
/// （序号决定颜色），也不会出现两条同色的条挨在一起。
pub fn bar_tiles(index: usize) -> (UiTile, UiTile) {
    match index % 3 {
        0 => (UiTile::ProgressGreen, UiTile::ProgressGreenFrame),
        1 => (UiTile::ProgressBlue, UiTile::ProgressBlueFrame),
        _ => (UiTile::ProgressRed, UiTile::ProgressRedFrame),
    }
}

/// 资源条填充的便捷构造。
pub fn bar_fill(atlas: &UiAtlas, tile: UiTile) -> ImageNode {
    sliced_node(atlas, tile, BAR_BORDER, BAR_TINT)
}

/// 资源条外框的便捷构造。
pub fn bar_frame(atlas: &UiAtlas, tile: UiTile) -> ImageNode {
    sliced_node(atlas, tile, BAR_BORDER, BAR_TINT)
}

/// 注册 UI 主题。
///
/// **本插件不注册系统**：`load_ui_atlas` 由 `presentation` 的启动链显式调用。
/// 这样"加载素材 → 装字体 → 建 HUD"是一条**看得见的链**，
/// 而不是靠插件注册顺序去猜——那猜错过一次。
pub struct UiThemePlugin;

impl Plugin for UiThemePlugin {
    fn build(&self, _app: &mut App) {
        // 有意为空：见类型文档。
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    /// 九宫格边宽必须**小于图块边长的一半**，否则四角会重叠。
    ///
    /// 为什么钉住：边宽写大了 `TextureSlicer` 会把边和角挤在一起 ——
    /// 画面上是"面板边框糊成一团"，而且**不报错**。
    #[test]
    fn the_nine_slice_border_fits_inside_the_tile() {
        for tile in [UiTile::PanelBrown, UiTile::PanelGrey] {
            let (_, _, w, h) = tile.rect();
            assert!(
                PANEL_BORDER * 2.0 < w as f32 && PANEL_BORDER * 2.0 < h as f32,
                "{tile:?} 是 {w}x{h}，边宽 {PANEL_BORDER} 的九宫格放不下（四角会重叠）"
            );
        }
    }

    /// 进度条的边宽同样要放得下（16x32 的图块）。
    #[test]
    fn the_bar_border_fits_inside_the_bar_tile() {
        for tile in [
            UiTile::ProgressGreen,
            UiTile::ProgressGreenFrame,
            UiTile::ProgressBlue,
            UiTile::ProgressBlueFrame,
            UiTile::ProgressRed,
            UiTile::ProgressRedFrame,
        ] {
            let (_, _, w, _) = tile.rect();
            assert!(
                BAR_BORDER * 2.0 < w as f32,
                "{tile:?} 宽 {w}，左右各 {BAR_BORDER} 的边会重叠"
            );
        }
    }

    /// **相邻资源条不同色**：`bar_tiles` 要真的按序号循环。
    ///
    /// 这条是"加一种新资源不用改代码"那个承诺的守卫 ——
    /// 若哪天有人把它写成固定的绿色，前三条资源会变成三条同样的绿条。
    #[test]
    fn neighbouring_bars_get_different_colours() {
        let (a, _) = bar_tiles(0);
        let (b, _) = bar_tiles(1);
        let (c, _) = bar_tiles(2);
        assert_ne!(a, b, "第 1 与第 2 条不该同色");
        assert_ne!(b, c, "第 2 与第 3 条不该同色");
        assert_ne!(a, c, "第 1 与第 3 条不该同色");
        assert_eq!(bar_tiles(3).0, a, "第 4 条该回到第 1 条的颜色（三色循环）");
    }

    /// **面板 tint 必须压暗**，否则浅色文字在亮面板上看不见。
    ///
    /// `ImageNode::color` 与像素相乘，所以 tint 的 RGB 越大越亮。
    /// 全白（1.0）等于没压暗 —— 那正是"文字糊在背景里"的原因。
    #[test]
    fn the_panel_tint_actually_darkens() {
        let tint = PANEL_TINT.to_srgba();
        let brightest = tint.red.max(tint.green).max(tint.blue);
        assert!(
            brightest < 0.8,
            "面板 tint 的最亮通道是 {brightest}，压得不够暗：浅色文字会糊在 Kenney 的亮面板上（实测过）"
        );
        assert!(
            brightest > 0.1,
            "面板 tint 太暗（{brightest}），边框会看不出来，失去美术效果"
        );
    }

    /// 进度条**不该**被压暗（它本来就该是亮的）。
    #[test]
    fn the_bar_tint_does_not_darken() {
        let tint = BAR_TINT.to_srgba();
        assert!(
            tint.red > 0.9 && tint.green > 0.9 && tint.blue > 0.9,
            "进度条 tint 该接近纯白（乘上去不改变颜色）"
        );
    }
}
