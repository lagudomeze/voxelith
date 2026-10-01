//! **地面表现方式的二选一**：体素地形 还是 GLB 地板网格。
//!
//! ## 为什么需要这个模块（踩过的坑）
//!
//! 两种地面都落在 `y = 0` 附近：
//!
//! - [`super::terrain`]：体素 + 贪婪网格化，有起伏、可挖、看起来**不是格子**；
//! - [`super::floor_grid`]：一格一个 Kenney 地板瓦片，**天然有格线**，但不可挖。
//!
//! 同时开会互相遮挡（实测：地板铺好了，被绿色地形整个盖住）+ z-fighting。
//!
//! ## 为什么不让两边各自读环境变量
//!
//! 我第一版就是这么做的，**结果判据写反了**：
//!
//! | 模块 | 判据 | `FLOOR_GRID` 未设时的结果 |
//! |---|---|---|
//! | `floor_grid` | `map_or(true, \|v\| v != "0")` | **开** ✅ |
//! | `terrain` | `is_ok_and(\|v\| v != "0")` | **也开** ❌ |
//!
//! 两个模块各自解读同一个环境变量，"默认值"的写法还不一样 ——
//! 这种 bug 的表现是"两边都跑了"，而**两边各自的代码看起来都是对的**。
//!
//! 所以把决策收敛到**一个类型**：谁想知道用哪种地面，就问 [`GroundStyle`]。
//! 环境变量只在这一个地方读。

use bevy::prelude::*;

/// 地面用哪种表现。
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GroundStyle {
    /// **GLB 地板网格**（默认）：一格一个 Kenney 瓦片，有格线，不可挖。
    ///
    /// 为什么不挖地形：现在要的是"看清格子与素材比例"，
    /// 而体素地形合并面之后**没有格线**，反而不利于这个目的。
    #[default]
    TileGrid,
    /// 体素地形：有起伏、可挖、走图集。
    Voxels,
}

impl GroundStyle {
    /// 从环境变量决定。**这是唯一读 `FLOOR_GRID` 的地方。**
    ///
    /// `FLOOR_GRID=0` ⇒ 体素地形；其余（含未设）⇒ 地板网格。
    pub fn from_env() -> Self {
        match std::env::var("FLOOR_GRID") {
            Ok(v) if v == "0" => Self::Voxels,
            _ => Self::TileGrid,
        }
    }

    /// 是否铺地板网格。
    pub fn is_tile_grid(self) -> bool {
        self == Self::TileGrid
    }

    /// 是否加载体素地形。
    pub fn is_voxels(self) -> bool {
        self == Self::Voxels
    }
}

/// 注册地面风格。
pub struct GroundStylePlugin;

impl Plugin for GroundStylePlugin {
    fn build(&self, app: &mut App) {
        let style = GroundStyle::from_env();
        info!(
            "地面表现：{}（FLOOR_GRID=0 可切到体素地形）",
            match style {
                GroundStyle::TileGrid => "GLB 地板网格",
                GroundStyle::Voxels => "体素地形",
            }
        );
        app.insert_resource(style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **两种风格必须互斥**（同时开会互相遮挡）。
    #[test]
    fn the_two_styles_are_mutually_exclusive() {
        for style in [GroundStyle::TileGrid, GroundStyle::Voxels] {
            assert_ne!(
                style.is_tile_grid(),
                style.is_voxels(),
                "{style:?} 不能同时是两种风格"
            );
        }
    }

    /// 默认是**地板网格**（现在要的就是格子地面）。
    #[test]
    fn the_default_is_the_tile_grid() {
        assert_eq!(GroundStyle::default(), GroundStyle::TileGrid);
    }

    /// 环境变量映射：只有 `FLOOR_GRID=0` 才切到体素。
    ///
    /// 注意这条测试**改自己进程的环境变量**，所以用 `--test-threads` 隔离
    /// 或者接受它与别的测试串行 —— 这里只在同一个测试内设置并还原。
    #[test]
    fn only_an_explicit_zero_selects_voxels() {
        // 保存现场。
        let saved = std::env::var("FLOOR_GRID").ok();

        // SAFETY: 单线程测试，且本测试自己负责还原。
        unsafe { std::env::set_var("FLOOR_GRID", "0") };
        assert_eq!(GroundStyle::from_env(), GroundStyle::Voxels, "`0` 该切体素");

        unsafe { std::env::set_var("FLOOR_GRID", "1") };
        assert_eq!(GroundStyle::from_env(), GroundStyle::TileGrid);

        unsafe { std::env::remove_var("FLOOR_GRID") };
        assert_eq!(
            GroundStyle::from_env(),
            GroundStyle::TileGrid,
            "未设该用默认（地板网格）"
        );

        // 还原。
        match saved {
            Some(v) => unsafe { std::env::set_var("FLOOR_GRID", v) },
            None => unsafe { std::env::remove_var("FLOOR_GRID") },
        }
    }
}
