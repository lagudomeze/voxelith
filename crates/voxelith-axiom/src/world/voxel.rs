//! 体素本身：ID、空气约定、渲染属性表。
//!
//! **ID 是运行时的全部**：地形只存 `u16`，名字只在加载期出现（与内容层的词汇表同一套路）。
//! 渲染属性（哪个方块透明、贴图用哪一张）由 L2 注册进 [`VoxelPalette`]，
//! 数据层**不认识**纹理（**R75、R103**）。

use bevy_reflect::Reflect;
/// 体素标识（`0` 恒为空气，见 [`Voxel::AIR`]）。
///
/// `Default` 是 `0` = 空气：这样 `Voxel::default()` 天然是空，
/// 新数组 / 新结构体不需要"全部填成空气"的初始化。
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Default,
    Reflect,
    serde::Serialize,
    serde::Deserialize,
)]
pub struct VoxelId(pub u16);

/// 一个体素。
///
/// 现在只有 ID（对齐"类似 MC 的体素系统"的最小必要信息）。
/// 将来要存"朝向 / 含水量"这类每格状态时，加在这里，而不是在渲染层另开一份。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Reflect)]
pub struct Voxel {
    /// 方块 ID（`0` = 空气）。
    pub id: VoxelId,
}

impl Voxel {
    /// 空气。**约定 ID `0` 恒为空气**：这样 `Voxel::default()` 就是空，
    /// 新数组天然是空的，不需要"全部填成空气"的初始化。
    pub const AIR: Self = Self { id: VoxelId(0) };

    /// 造一个实心体素。
    pub const fn solid(id: VoxelId) -> Self {
        Self { id }
    }

    /// 是不是空气。
    pub fn is_air(self) -> bool {
        self.id == VoxelId(0)
    }

    /// 是不是实心（现在只要不是空气就算实心；将来若要"水/草"这类非实心方块，
    /// 判据应改成查 [`VoxelPalette`]，不要在这里写死）。
    pub fn is_solid(self) -> bool {
        !self.is_air()
    }
}

/// 一个方块的**渲染属性**（由 L2 在加载内容时注册）。
///
/// 数据层只把 ID 当 ID；"这个方块长什么样"是表现层的事。放到这里是因为它是
/// **每种方块一份**的静态数据，放在 L2 会让网格化任务每次都要跨层查询。
#[derive(Debug, Clone, PartialEq, Reflect)]
pub struct VoxelAppearance {
    /// 贴图在图集里的下标（L2 解释）。
    pub atlas_index: u16,
    /// 是否遮挡邻面（贪婪网格化用：透明的方块不该把邻面藏掉）。
    pub opaque: bool,
}

impl Default for VoxelAppearance {
    fn default() -> Self {
        Self {
            atlas_index: 0,
            opaque: true,
        }
    }
}

/// 方块外观表（**Resource**）：ID → 外观。
///
/// **ID `0` 保留给空气**（[`Voxel::AIR`] 的判据就是 `id == 0`），所以：
///
/// - [`push`](Self::push) 返回的 ID **从 `1` 开始**；
/// - 表里第 `n` 项对应 ID `n + 1`；
/// - 查 ID `0` 恒返回 `None`。
///
/// 这与 [`VoxelNames`](super::VoxelNames) 的编号**必须一致**——两个表错位一格会让
/// "名字解析出的 ID"与"外观查到的贴图"对不上，表现是**贴图串味**（比画不出来更难查）。
#[derive(bevy_ecs::prelude::Resource, Debug, Clone, Default, Reflect)]
pub struct VoxelPalette {
    entries: Vec<VoxelAppearance>,
}

impl VoxelPalette {
    /// 造一张表：`entries[0]` 是 **ID 1**，依此类推（ID 0 是空气）。
    pub fn new(entries: Vec<VoxelAppearance>) -> Self {
        Self { entries }
    }

    /// 登记一个外观，返回它的 ID（**从 `1` 开始**；`0` 留给空气）。
    pub fn push(&mut self, appearance: VoxelAppearance) -> VoxelId {
        self.entries.push(appearance);
        VoxelId(self.entries.len() as u16)
    }

    /// 查外观；**空气或没登记过的 ID 返回 `None`**（不 panic：网格化不该因为内容漏配而崩）。
    pub fn get(&self, id: VoxelId) -> Option<&VoxelAppearance> {
        if id == VoxelId(0) {
            return None;
        }
        self.entries.get(id.0 as usize - 1)
    }

    /// 这个方块是否遮挡邻面。
    ///
    /// ## 两条判据，顺序不能反（**这里出过一次"整片地形全黑"**）
    ///
    /// 1. **空气先判**：`VoxelId(0)` 永远返回 `false`。
    ///    空气不挡光——这是网格化"看得见地表"的前提。
    /// 2. 没登记过的 ID 按**实心**处理：宁可多画几面，也不要因为内容漏配而漏面。
    ///
    /// ⚠️ 曾经写成"没登记过 ⇒ 实心"而没有先判空气，于是**空气被当成实心**：
    /// 贪婪网格化认为每个面都被邻格挡住，**所有区块都产出空网格**，
    /// 而表现就是"地形完全画不出来、且不报任何错"。
    /// 教训：**"安全默认值"要按"最常见的输入"来选**——世界里最多的方块是空气，
    /// 它绝不该落到"未知"分支里。
    pub fn is_opaque(&self, id: VoxelId) -> bool {
        if id == VoxelId(0) {
            return false;
        }
        self.get(id).is_none_or(|appearance| appearance.opaque)
    }

    /// 登记了几种。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空表？
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn air_is_id_zero_and_default() {
        assert!(Voxel::default().is_air());
        assert!(Voxel::AIR.is_air());
        assert!(!Voxel::solid(VoxelId(3)).is_air());
    }

    #[test]
    fn palette_indexes_by_id_and_treats_air_as_absent() {
        let mut palette = VoxelPalette::default();
        let stone = palette.push(VoxelAppearance {
            atlas_index: 1,
            opaque: true,
        });
        let water = palette.push(VoxelAppearance {
            atlas_index: 2,
            opaque: false,
        });
        assert_eq!(
            stone,
            VoxelId(1),
            "第一个登记的方块是 ID 1 —— **ID 0 保留给空气**"
        );
        assert_eq!(palette.get(water).unwrap().atlas_index, 2);
        assert!(!palette.is_opaque(water));

        // **空气永远不挡光**：这是网格化"看得见地表"的前提。
        // 曾经因为把它归到"没登记过 ⇒ 实心"里，导致整片地形一个面都画不出来。
        assert!(
            !palette.is_opaque(VoxelId(0)),
            "空气必须不遮挡邻面，否则网格化会把所有面都剔掉"
        );
        assert!(palette.get(VoxelId(99)).is_none(), "没登记过就是 None");
        assert!(
            palette.is_opaque(VoxelId(99)),
            "没登记过按实心处理：宁可多画面也不要漏面"
        );
    }
}
