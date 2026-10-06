//! **格子坐标**：逻辑位置的唯一表示（L0，零依赖）。
//!
//! 逻辑位置是**格子**，不是浮点世界坐标：视觉位置由表现层从格子算出来
//! （`f(now)` 纯函数），所以 L0 只认一对 `i32`，**不引 `glam` / `bevy_math`**。
//!
//! ```text
//! L0/L1  CellPos(i32, i32)     判定、寻路、距离、目标选择都只看它
//! L2     CellPos → 世界坐标     由渲染层用纯函数换算（可插值，但插值结果不是真相）
//! ```
//!
//! 两个距离口径都留着，因为它们对应两套移动规则，**用错会让"斜着走"的代价算错**：
//!
//! | 口径 | 适用于 | 斜向一步 |
//! |---|---|---|
//! | [`CellPos::chebyshev`] | 八向移动（含斜走） | 算 1 |
//! | [`CellPos::manhattan`] | 四向移动（只有直走） | 算 2 |
//!
//! 用哪一个是**玩法决定**，不是数学细节：选 chebyshev 就是在说"斜走不额外花钱"。

use bevy_ecs::prelude::*;

/// 逻辑位置：格坐标（不是世界坐标）。
///
/// `x` / `y` 对应体素世界的两个水平轴；高度由地形查询决定，不在这里存
/// （单层世界下它是常数，多层时它是 `f(x, y)`）。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct CellPos {
    /// 格坐标 X。
    pub x: i32,
    /// 格坐标 Y（体素世界的另一个水平轴）。
    pub y: i32,
}

impl CellPos {
    /// 原点。
    pub const ZERO: Self = Self { x: 0, y: 0 };

    /// 构造。
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    /// 拆成数组（给"按分量算"的地方用）。
    pub const fn to_array(self) -> [i32; 2] {
        [self.x, self.y]
    }

    /// 平移。
    pub const fn offset(self, dx: i32, dy: i32) -> Self {
        Self {
            x: self.x + dx,
            y: self.y + dy,
        }
    }

    /// **切比雪夫距离**：八向移动下的步数（斜走算 1）。
    pub fn chebyshev(self, other: Self) -> i32 {
        (self.x - other.x).abs().max((self.y - other.y).abs())
    }

    /// **曼哈顿距离**：四向移动下的步数（斜走算 2）。
    pub fn manhattan(self, other: Self) -> i32 {
        (self.x - other.x).abs() + (self.y - other.y).abs()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_move_one_cell_at_a_time() {
        let origin = CellPos::new(3, -2);
        assert_eq!(origin.offset(1, 0), CellPos::new(4, -2));
        assert_eq!(origin.offset(0, -5), CellPos::new(3, -7));
        assert_eq!(origin.offset(0, 0), origin);
    }

    #[test]
    fn the_two_distance_metrics_disagree_exactly_on_diagonals() {
        let origin = CellPos::ZERO;
        let diagonal = CellPos::new(3, 3);
        assert_eq!(origin.chebyshev(diagonal), 3, "斜走算 1");
        assert_eq!(origin.manhattan(diagonal), 6, "斜走要绕，算 2");

        let straight = CellPos::new(0, 4);
        assert_eq!(origin.chebyshev(straight), 4);
        assert_eq!(origin.manhattan(straight), 4, "直走两种口径一致");
    }

    #[test]
    fn distance_is_symmetric_and_zero_on_self() {
        let a = CellPos::new(-7, 2);
        let b = CellPos::new(5, -9);
        assert_eq!(a.chebyshev(b), b.chebyshev(a));
        assert_eq!(a.manhattan(b), b.manhattan(a));
        assert_eq!(a.chebyshev(a), 0);
        assert_eq!(a.manhattan(a), 0);
    }
}
