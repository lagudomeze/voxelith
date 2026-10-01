//! 体素世界的 RON 描述结构（`world.ron`）。
//!
//! 与 [`super::descriptor`] 分开是因为"战斗内容"与"地形内容"是两套互不引用的描述，
//! 放一个文件里只会让它越过 500 行上限（**R26**）而没有任何好处。
//!
//! 名字在这里只出现一次：加载期翻成 `u16` ID，运行时不再碰字符串。

// ------------------------------------------------------------------ 体素世界

/// 贴图图案（RON 形态，与运行时的 [`TexturePattern`] 同名同形）。
#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize)]
pub enum PatternRon {
    /// 纯色。
    Solid,
    /// 细碎噪点。
    Speckle {
        /// 明暗波动幅度（0–255）。
        amount: u8,
    },
    /// 水平条纹。
    Stripes {
        /// 条纹间距。
        spacing: u32,
        /// 明暗波动幅度。
        amount: u8,
    },
    /// 顶部一层亮色。
    Topped {
        /// 顶部几行用亮色。
        rows: u32,
        /// 亮多少。
        amount: u8,
    },
}

impl Default for PatternRon {
    fn default() -> Self {
        Self::Solid
    }
}

/// 一个面的贴图（RON 形态）。
#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize)]
#[serde(default)]
pub struct FaceTextureRon {
    /// 基色（sRGB 0–255）。
    pub base: [u8; 3],
    /// 图案。
    pub pattern: PatternRon,
}

impl Default for FaceTextureRon {
    fn default() -> Self {
        Self {
            base: [255, 0, 255],
            pattern: PatternRon::Solid,
        }
    }
}

/// 一种方块（RON 形态）。
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(default)]
pub struct BlockRon {
    /// 稳定名字（`grass`）：运行时只留 ID。
    pub id: String,
    /// 侧面贴图。
    pub side: FaceTextureRon,
    /// 顶面（缺省用侧面）。
    pub top: Option<FaceTextureRon>,
    /// 底面（缺省用侧面）。
    pub bottom: Option<FaceTextureRon>,
    /// 是否遮挡邻面。
    pub opaque: bool,
}

impl Default for BlockRon {
    fn default() -> Self {
        Self {
            id: String::new(),
            side: FaceTextureRon::default(),
            top: None,
            bottom: None,
            opaque: true,
        }
    }
}

/// 高度噪声（RON 形态）。
#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize)]
#[serde(default)]
pub struct NoiseRon {
    /// 基础频率。
    pub frequency: f32,
    /// 叠几层。
    pub octaves: u32,
    /// 每层频率翻几倍。
    pub lacunarity: f32,
    /// 每层振幅乘几倍。
    pub persistence: f32,
    /// 种子。
    pub seed: u32,
}

impl Default for NoiseRon {
    fn default() -> Self {
        let defaults = crate::world::NoiseParams::default();
        Self {
            frequency: defaults.frequency,
            octaves: defaults.octaves,
            lacunarity: defaults.lacunarity,
            persistence: defaults.persistence,
            seed: defaults.seed,
        }
    }
}

/// 地形参数（RON 形态）：方块写成名字，加载期翻成 ID。
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(default)]
pub struct TerrainRon {
    /// 地表基准高度。
    pub base_height: i32,
    /// 起伏幅度。
    pub amplitude: f32,
    /// 高度噪声。
    pub height: NoiseRon,
    /// 次表层厚度。
    pub soil_depth: i32,
    /// 地表方块名。
    pub surface: String,
    /// 次表层方块名。
    pub soil: String,
    /// 深层方块名。
    pub deep: String,
    /// 世界底部。
    pub floor_y: i32,
}

impl Default for TerrainRon {
    fn default() -> Self {
        Self {
            base_height: 0,
            amplitude: 1.6,
            height: NoiseRon::default(),
            soil_depth: 2,
            surface: "grass".into(),
            soil: "dirt".into(),
            deep: "stone".into(),
            floor_y: -4,
        }
    }
}

/// 体素世界定义（`world.ron`）。
#[derive(Debug, Clone, Default, PartialEq, serde::Deserialize)]
#[serde(default)]
pub struct WorldRon {
    /// 地形参数。
    pub terrain: TerrainRon,
    /// 方块表（**顺序即 ID**；`0` 留给空气，所以空气不要写进来）。
    pub blocks: Vec<BlockRon>,
    /// 纸片人（角色精灵）的摆放参数。
    #[serde(default)]
    pub sprite: SpriteRon,
}

/// 纸片人的摆放参数（RON 形态）。
///
/// 放在 `world.ron` 里而不是代码里，因为它是**内容**：
/// 换一套精灵素材、或者换一个相机取景，这个值就要跟着重标定。
#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize)]
#[serde(default)]
pub struct SpriteRon {
    /// 喂给 `custom_size` 的尺寸。
    ///
    /// ⚠️ **不是世界单位**。实测约 `2.4 像素 / 每单位`，
    /// 所以 `115` 对应约 130 像素高的角色。改之前请先用截图标定
    /// （方法见 `work/TODO.md`）。
    pub world_size: f32,
    /// 精灵中心离地高度（**这个**是世界单位）。
    pub height: f32,
}

impl Default for SpriteRon {
    fn default() -> Self {
        Self {
            world_size: 115.0,
            height: 1.6,
        }
    }
}
