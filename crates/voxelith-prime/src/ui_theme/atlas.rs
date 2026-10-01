//! **UI 图集**：Kenney `ui-pack-adventure` 的图块位置。
//!
//! ## 与旧做法的区别（这是"简化"的核心）
//!
//! 旧做法：用 `tools/transcode-kenney-ui.py` 把 11 个图块**裁出来转成 Rust 源码**
//! （2500 行字节数组），每个图块一张独立的 `Image`。
//!
//! 现在：**整张图集作为一个素材加载**，每个图块只是"图集里的一个矩形"。
//! `ImageNode` 用 `rect` 选区域。所以：
//!
//! - **没有转码脚本**，也没有生成的字节数组；
//! - **换素材 = 换文件**（把新图集放进 `assets/ui/`）；
//! - 加一个图块 = 在 [`UiTile`] 加一个变体 + 在 [`tile_rect`] 加一行坐标。
//!
//! ## 坐标从哪来
//!
//! Kenney 的包里带一份 XML（`spritesheet-default.xml`），里面是每个元素的矩形。
//! 那份 XML 也放进 `assets/ui/` —— 它是**素材自带的元数据**，
//! 保留它意味着"图块坐标"这件事的**事实来源仍在素材侧**，
//! 而不是我抄进代码里的一串魔法数字。
//!
//! 抄进代码的那一份在 [`tile_rect`] 里，并由测试对着 XML 核对
//! （见 `tests`：**抄错了会失败**）。

use bevy::prelude::*;

/// 图集素材的路径（相对 Bevy 的资产根）。
pub const UI_ATLAS_PATH: &str = "ui/kenney-adventure.png";

/// 图集自带的 XML 元数据（**只用于测试核对坐标**）。
#[cfg(test)]
pub const UI_ATLAS_XML: &str = include_str!("../../../../assets/ui/kenney-adventure.xml");

/// 用到的 UI 图块。
///
/// 只列 HUD 真正用到的那些；图集里有 128 个元素，不需要全列。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UiTile {
    /// 棕色面板（左上状态面板）。
    PanelBrown,
    /// 灰色面板（左下技能面板）。
    PanelGrey,
    /// 进度条填充（绿 / 蓝 / 红）。
    ProgressGreen,
    /// 绿色进度条的外框。
    ProgressGreenFrame,
    /// 蓝色进度条填充。
    ProgressBlue,
    /// 蓝色进度条外框。
    ProgressBlueFrame,
    /// 红色进度条填充。
    ProgressRed,
    /// 红色进度条外框。
    ProgressRedFrame,
    /// 棕色按钮。
    ButtonBrown,
    /// 灰色按钮。
    ButtonGrey,
    /// 红色按钮。
    ButtonRed,
}

impl UiTile {
    /// 在 Kenney 的 XML 里对应的名字。
    ///
    /// `img` 后缀是 Kenney 打包时加的，不是笔误。
    pub fn asset_name(self) -> &'static str {
        match self {
            Self::PanelBrown => "panel_brownimg.png",
            Self::PanelGrey => "panel_greyimg.png",
            Self::ProgressGreen => "progress_greenimg.png",
            Self::ProgressGreenFrame => "progress_green_borderimg.png",
            Self::ProgressBlue => "progress_blueimg.png",
            Self::ProgressBlueFrame => "progress_blue_borderimg.png",
            Self::ProgressRed => "progress_redimg.png",
            Self::ProgressRedFrame => "progress_red_borderimg.png",
            Self::ButtonBrown => "button_brownimg.png",
            Self::ButtonGrey => "button_greyimg.png",
            Self::ButtonRed => "button_redimg.png",
        }
    }

    /// 图集里的像素矩形 `(x, y, 宽, 高)`。
    ///
    /// **这份数据是从素材自带的 XML 抄来的**，并由测试逐条核对。
    pub fn rect(self) -> (u32, u32, u32, u32) {
        match self {
            Self::PanelBrown => (384, 64, 64, 64),
            Self::PanelGrey => (448, 256, 64, 64),
            Self::ProgressGreen => (560, 96, 16, 32),
            Self::ProgressGreenFrame => (560, 128, 16, 32),
            Self::ProgressBlue => (560, 160, 16, 32),
            Self::ProgressBlueFrame => (560, 192, 16, 32),
            Self::ProgressRed => (566, 552, 16, 32),
            Self::ProgressRedFrame => (560, 64, 16, 32),
            Self::ButtonBrown => (144, 576, 48, 24),
            Self::ButtonGrey => (240, 576, 48, 24),
            Self::ButtonRed => (48, 576, 48, 24),
        }
    }

    /// 转成 Bevy 的 `Rect`（`ImageNode.rect` 用它选区域）。
    pub fn bevy_rect(self) -> Rect {
        let (x, y, w, h) = self.rect();
        Rect::new(x as f32, y as f32, (x + w) as f32, (y + h) as f32)
    }
}

/// 已加载的 UI 图集（句柄 + 尺寸）。
#[derive(Resource, Debug, Clone)]
pub struct UiAtlas {
    /// 整张图集的贴图句柄。
    pub image: Handle<Image>,
    /// 图集像素尺寸（`ImageNode.rect` 不必归一化，但尺寸对调试有用）。
    pub width: u32,
    /// 图集像素高度。
    pub height: u32,
}

/// 发起图集加载（**异步**：只拿到句柄，像素几帧后才到）。
///
/// `AssetServer` 用 `Option`：集成测试（`tests/hud.rs`）只装 `PresentationPlugin`，
/// **没有资源设施**（`AssetPlugin` 要整个渲染后端），必填参数会让那些测试
/// 在系统参数校验阶段就 panic —— 而那与"HUD 长什么样"这个被测对象无关。
/// 缺了就跳过：图集由调用方预置一份空句柄的（见 `tests/hud.rs`）。
pub fn load_ui_atlas(mut commands: Commands, assets: Option<Res<AssetServer>>) {
    let Some(assets) = assets else {
        return;
    };
    commands.insert_resource(UiAtlas {
        image: assets.load(UI_ATLAS_PATH),
        // 尺寸要等加载完才知道；`ImageNode.rect` 用的是像素坐标，
        // 与这里记的尺寸无关，所以先填 0，由 `log_loaded_atlas` 更新。
        width: 0,
        height: 0,
    });
}

/// 图集像素到位后补上尺寸并打印一行（**只成功一次**）。
pub fn log_loaded_atlas(
    mut atlas: Option<ResMut<UiAtlas>>,
    images: Option<Res<Assets<Image>>>,
    mut done: Local<bool>,
) {
    if *done {
        return;
    }
    let (Some(atlas), Some(images)) = (atlas.as_mut(), images) else {
        return;
    };
    let Some(image) = images.get(&atlas.image) else {
        return;
    };
    atlas.width = image.width();
    atlas.height = image.height();
    info!(
        "UI 图集：{}×{}（{UI_ATLAS_PATH}），{} 个图块",
        atlas.width,
        atlas.height,
        UiTile::ALL.len()
    );
    *done = true;
}

impl UiTile {
    /// 全部图块（遍历用）。
    pub const ALL: [UiTile; 11] = [
        UiTile::PanelBrown,
        UiTile::PanelGrey,
        UiTile::ProgressGreen,
        UiTile::ProgressGreenFrame,
        UiTile::ProgressBlue,
        UiTile::ProgressBlueFrame,
        UiTile::ProgressRed,
        UiTile::ProgressRedFrame,
        UiTile::ButtonBrown,
        UiTile::ButtonGrey,
        UiTile::ButtonRed,
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 从 Kenney 的 XML 里取出某个名字的矩形。
    fn xml_rect(name: &str) -> Option<(u32, u32, u32, u32)> {
        // XML 很小，直接按行找，不引 XML 库。
        for line in UI_ATLAS_XML.lines() {
            let line = line.trim();
            if !line.starts_with("<SubTexture") || !line.contains(name) {
                continue;
            }
            let get = |key: &str| -> Option<u32> {
                let start = line.find(&format!("{key}=\""))? + key.len() + 2;
                let end = line[start..].find('"')? + start;
                line[start..end].parse().ok()
            };
            return Some((get("x")?, get("y")?, get("width")?, get("height")?));
        }
        None
    }

    /// **代码里抄的坐标必须与素材自带的 XML 一致。**
    ///
    /// 这是"把坐标抄进代码"这个做法的代价所在 —— 抄错了，
    /// 画面上会显示**图集里另一个元素**（比如按钮画成半个面板），而且不报错。
    /// 所以逐条核对。
    #[test]
    fn every_tile_matches_the_atlas_metadata() {
        for tile in UiTile::ALL {
            let name = tile.asset_name();
            let from_xml =
                xml_rect(name).unwrap_or_else(|| panic!("图集 XML 里没有 {name}（素材换了？）"));
            assert_eq!(
                tile.rect(),
                from_xml,
                "`UiTile::{tile:?}` 的坐标与素材 XML 对不上（{name}）"
            );
        }
    }

    /// 图块不能超出图集范围（超了会采到图外，显示成透明或别的元素）。
    #[test]
    fn every_tile_fits_inside_the_atlas() {
        // Kenney 的这张图集是 592×600。
        const W: u32 = 592;
        const H: u32 = 600;
        for tile in UiTile::ALL {
            let (x, y, w, h) = tile.rect();
            assert!(
                x + w <= W && y + h <= H,
                "`{tile:?}` 的矩形 ({x},{y},{w},{h}) 超出图集 {W}×{H}"
            );
        }
    }

    /// 每个图块的名字都不同（复制粘贴时最容易写重复）。
    #[test]
    fn tile_names_are_unique() {
        let mut names: Vec<&str> = UiTile::ALL.iter().map(|t| t.asset_name()).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(before, names.len(), "有图块用了同一个素材名");
    }
}
