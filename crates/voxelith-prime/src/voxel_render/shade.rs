//! 面朝向明暗：**立体感的来源**，烘焙进图集。
//!
//! ## 为什么是"烘焙"而不是"光照"或"顶点色"
//!
//! 立体感来自"同一个方块的不同面亮度不同"。这个项目上**两条路都试过并失败**：
//!
//! | 做法 | 结果 |
//! |---|---|
//! | 真实光照（方向光 + 环境光） | 难调。实测顶面/侧面只差 **12%**，画面是一堆平色块；把主光调大又整片过曝 |
//! | `unlit` + 逐顶点色 | **不可行**：`StandardMaterial` 不读顶点色（`bevy_pbr` 里没有 `ATTRIBUTE_COLOR`），所有面渲染成同一亮度 |
//!
//! 所以改成**烘焙**：图集本来就有"每个方块三张格"（顶 / 侧 / 底），
//! 在生成那三张格时就把明暗乘进去。优点：
//!
//! - **完全确定**：与光源、相机、环境光全都无关；
//! - **逐面精确**：顶面就是比侧面亮 38%，不依赖任何法线计算；
//! - **零运行期开销**：一张贴图搞定，材质可以是 `unlit`。
//!
//! ## 系数从哪来
//!
//! [`FaceShade::MC_STYLE`]（`axiom::world`）：顶面 `1.0`、侧面 `0.62`、底面 `0.45`。
//! "顶亮、侧中、底暗"是 MC 风格**约定俗成**的美术规则，不该随灯光走样。
//! 想换风格就改 `FaceShade`，不必碰光源、也不必碰网格化。

use voxelith_axiom::world::FaceShade;

pub use super::atlas::BlockFace;

/// 某个面朝向的明暗系数（乘在贴图颜色上，`1.0` = 原色）。
pub fn face_shade(face: BlockFace) -> f32 {
    let (axis, sign) = match face {
        // 侧面：`axis` 取非竖直轴即可，`pick` 只看它是不是 1。
        BlockFace::Side => (0, 1),
        BlockFace::Top => (1, 1),
        BlockFace::Bottom => (1, -1),
    };
    FaceShade::mc_style(axis, sign)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_is_brightest_and_bottom_is_darkest() {
        let top = face_shade(BlockFace::Top);
        let side = face_shade(BlockFace::Side);
        let bottom = face_shade(BlockFace::Bottom);
        assert!(top > side, "顶面该比侧面亮：{top} vs {side}");
        assert!(side > bottom, "侧面该比底面亮：{side} vs {bottom}");
    }

    #[test]
    fn contrast_is_strong_enough_to_read_as_volume() {
        // 这条是**从用户反馈反推出来的**：用户说"看不出 3D 感觉，还是一堆色块"。
        // 量化原因就是顶面/侧面只差 12%。守住差距下限，防止以后又被调回去。
        let top = face_shade(BlockFace::Top);
        let side = face_shade(BlockFace::Side);
        let ratio = top / side;
        assert!(
            ratio >= 1.4,
            "顶面/侧面的亮度比只有 {ratio:.2}，差异太小会看不出体积（目标 ≥ 1.4）"
        );
    }
}
