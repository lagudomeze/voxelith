//! 格坐标。
//!
//! 逻辑位置**永远是离散格**（`GP-12`：任何时刻都不能停在格与格之间 ✓），所以位置一律用
//! [`TilePos`]，不裸用 `IVec3`——六方向等距、环面环绕这些是**格坐标自己的事**（`WD-01`），
//! 以后要往上加方法（邻接、距离、环绕），裸向量会和别处任何三维整数混在一起 ✗
//!
//! 现在只放最小的一套：构造、取分量、相等、哈希、与 [`IVec3`] 互转。
//! **环绕需要世界尺寸**，那是世界层的属性，不在这里 ✓

use bevy_ecs::prelude::*;
use bevy_math::IVec3;

/// 体素世界里的一个格。
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct TilePos(pub IVec3);

impl TilePos {
    /// 原点。
    pub const ZERO: Self = Self(IVec3::ZERO);

    /// 造一个格坐标。
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self(IVec3::new(x, y, z))
    }

    /// 从一个整数向量来。
    pub const fn of(value: IVec3) -> Self {
        Self(value)
    }

    /// 取里面的整数向量。
    pub const fn ivec3(self) -> IVec3 {
        self.0
    }
}

impl From<IVec3> for TilePos {
    fn from(value: IVec3) -> Self {
        Self(value)
    }
}

impl From<TilePos> for IVec3 {
    fn from(value: TilePos) -> Self {
        value.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn construction_and_access() {
        let p = TilePos::new(1, -2, 3);

        assert_eq!(p.ivec3(), IVec3::new(1, -2, 3));
        assert_eq!(p, TilePos::of(IVec3::new(1, -2, 3)));
        assert_eq!(TilePos::ZERO, TilePos::new(0, 0, 0));
    }

    #[test]
    fn converts_both_ways() {
        let v = IVec3::new(4, 5, 6);

        assert_eq!(TilePos::from(v).ivec3(), v);
        assert_eq!(IVec3::from(TilePos::of(v)), v);
    }

    /// 能当哈希键用（世界层要按格索引）。
    #[test]
    fn is_a_usable_key() {
        let mut map = std::collections::HashMap::new();
        map.insert(TilePos::new(1, 2, 3), "石头");
        map.insert(TilePos::new(1, 2, 3), "泥土");

        assert_eq!(map.len(), 1);
        assert_eq!(map[&TilePos::new(1, 2, 3)], "泥土");
    }
}
