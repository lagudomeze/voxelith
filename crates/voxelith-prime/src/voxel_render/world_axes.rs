//! **世界坐标轴指示器**：在原点画 X/Y/Z 三根带箭头的轴。
//!
//! ## 为什么需要（排查工具，不是游戏内容）
//!
//! 等距视图下**方向极易看错**：俯角 45° + 方位角 45° 之后，
//! 屏幕上的"右"既不是 `+X` 也不是 `+Z`，而 `+X` 与 `+Z` 在画面上是对称的两条斜线。
//!
//! 这一条在本项目里**真的咬过人**：调地形、调模型朝向、判断"墙该往哪边加"
//! 时，光看画面分不出 `+X` 与 `+Z` —— 而两者的后果完全不同。
//!
//! ## 用 Bevy 内置的 `Gizmos::axes`
//!
//! 不需要自己搭箭头几何：`bevy_gizmos` 提供了
//! [`Gizmos::axes(transform, base_length)`](bevy::gizmos::prelude::Gizmos::axes)，
//! 它按 `Transform` 画三根轴，颜色是固定的 **X=红 / Y=绿 / Z=蓝**（`RED`/`GREEN`/`BLUE`）。
//!
//! `GizmoPlugin` 与 `GizmoRenderPlugin` 已经包含在 `DefaultPlugins` 里，不用额外注册。
//!
//! ## 怎么用
//!
//! 默认**关闭**（它是排查工具）。打开：
//!
//! ```powershell
//! $env:SHOW_AXES = "1"
//! ```
//!
//! 长度可用 `AXES_LENGTH` 覆盖（默认 8 个世界单位 = 8 格）。

use bevy::prelude::*;

/// 坐标轴的长度（世界单位）。默认 8 格 —— 够看清方向，又不至于糊满屏幕。
pub const DEFAULT_LENGTH: f32 = 8.0;

/// 原点坐标轴的配置。
#[derive(Resource, Debug, Clone, Copy)]
pub struct WorldAxes {
    /// 是否画。
    pub enabled: bool,
    /// 每根轴的长度（世界单位）。
    pub length: f32,
}

impl Default for WorldAxes {
    fn default() -> Self {
        Self {
            // `SHOW_AXES=1` 打开。默认关：它是排查工具，不该出现在正式画面上。
            enabled: std::env::var("SHOW_AXES").is_ok_and(|v| v == "1"),
            length: std::env::var("AXES_LENGTH")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(DEFAULT_LENGTH),
        }
    }
}

/// 在原点画坐标轴。
///
/// ## 为什么用一个单位 `Transform` 而不是直接传 `Vec3`
///
/// `Gizmos::axes` 接受 `impl TransformPoint`。传 `Transform::IDENTITY`
/// 就是"以世界原点为起点、方向与世界轴一致"。
/// 以后想让轴跟着某个物体走（比如选中高亮），把那个实体的 `Transform` 传进来即可 ——
/// **这个函数不用改**。
pub fn draw_world_axes(mut gizmos: Gizmos, axes: Res<WorldAxes>) {
    if !axes.enabled {
        return;
    }
    gizmos.axes(Transform::IDENTITY, axes.length);
}

/// 注册坐标轴指示器。
pub struct WorldAxesPlugin;

impl Plugin for WorldAxesPlugin {
    fn build(&self, app: &mut App) {
        let axes = WorldAxes::default();
        if axes.enabled {
            info!(
                "坐标轴指示器：已开启（长度 {}，X=红 / Y=绿 / Z=蓝）",
                axes.length
            );
        }
        app.insert_resource(axes)
            .add_systems(Update, draw_world_axes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 默认**关闭** —— 它是排查工具，不该出现在正式画面上。
    #[test]
    fn the_axes_are_off_by_default() {
        if std::env::var("SHOW_AXES").is_err() {
            assert!(!WorldAxes::default().enabled, "坐标轴默认该是关的");
        }
    }

    /// 长度必须为正 —— 为 0 或负数时 `Gizmos::axes` 画不出东西，
    /// 而"什么都没画"看起来就像"功能坏了"。
    #[test]
    fn the_length_is_positive() {
        let axes = WorldAxes::default();
        assert!(axes.length > 0.0, "轴长必须为正，实际 {}", axes.length);
    }

    /// **轴长与格子对齐**：默认长度该是整数格，这样一眼能数出几格。
    ///
    /// 一格是 1 世界单位（见 `super::floor_grid::CELL_SPACING`），
    /// 所以"长度是整数"就等于"与格线对齐"。
    #[test]
    fn the_default_length_aligns_with_the_grid() {
        assert_eq!(
            DEFAULT_LENGTH.fract(),
            0.0,
            "默认轴长 {DEFAULT_LENGTH} 不是整数格 ⇒ 箭头尖端对不到格线上"
        );
    }
}
