//! 程序化地形：**确定性**噪声生成（同样参数 ⇒ 同样世界）。
//!
//! 这一层是"程序化层"（**R65**）：它**不存储**任何东西，`voxel_at(pos)` 是纯函数。
//! 玩家改过的体素存在 [`VoxelStore`](super::VoxelStore) 的已加载区块里，查询时优先。
//!
//! ## 为什么自己写噪声而不是引依赖
//!
//! 只要一个"输入整数坐标 → `[-1, 1]`、看起来像地形"的函数：value noise 加两层八度就够，
//! 三十行、零依赖、可测（同样的种子必须给出同样的世界——存档与"同种子复现"都指望它）。
//! 引 `noise` / `fastnoise-lite` 会多一条**登记制依赖**（R5），收益不抵成本。
//!
//! ## 可配置
//!
//! [`TerrainParams`] 是**数据**：层高、起伏、噪声频率、各层用什么方块全在里面。
//! 战场用"薄而平"的默认值（俯视角要能一眼看完地形），但改成山也没人拦着。

use super::voxel::{Voxel, VoxelId};
use bevy_reflect::Reflect;

/// 贴图图案（**内容可配**）：`world.ron` 里就写它。
///
/// "每种方块长什么样"用"颜色 + 图案"描述，而不是让内容作者去画 16×16 PNG——
/// 加一种方块仍然只改 `.ron`。真美术资源到位时，这里加一个 `Image(path)` 变体即可，
/// 下游（网格顶点只认"图集格号"）不受影响。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TexturePattern {
    /// 纯色。
    Solid,
    /// 细碎噪点（泥土 / 石头）。
    Speckle {
        /// 明暗波动幅度（0–255）。
        amount: u8,
    },
    /// 水平条纹（木头）。
    Stripes {
        /// 条纹间距。
        spacing: u32,
        /// 明暗波动幅度。
        amount: u8,
    },
    /// 顶部一层亮色（草皮）。
    Topped {
        /// 顶部几行用亮色。
        rows: u32,
        /// 亮多少。
        amount: u8,
    },
}

impl Default for TexturePattern {
    fn default() -> Self {
        Self::Solid
    }
}

/// 一个面的贴图定义。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceTexture {
    /// 基色（sRGB 0–255）。
    pub base: [u8; 3],
    /// 图案。
    pub pattern: TexturePattern,
}

impl Default for FaceTexture {
    fn default() -> Self {
        Self {
            base: [255, 0, 255],
            pattern: TexturePattern::Solid,
        }
    }
}

/// 一种方块的完整外观定义（`world.ron` 的一条）。
#[derive(Debug, Clone, PartialEq)]
pub struct BlockDef {
    /// 稳定名字（`grass`）：运行时只留 ID。
    pub id: String,
    /// 侧面贴图。
    pub side: FaceTexture,
    /// 顶面（缺省用侧面）。
    pub top: Option<FaceTexture>,
    /// 底面（缺省用侧面）。
    pub bottom: Option<FaceTexture>,
    /// 是否遮挡邻面（透明的方块不该把邻面藏掉）。
    pub opaque: bool,
}

/// 噪声参数（频率 / 八度 / 种子）。
#[derive(Debug, Clone, Copy, PartialEq, Reflect)]
pub struct NoiseParams {
    /// 基础频率：`0.05` ≈ 每 20 格一个起伏。
    pub frequency: f32,
    /// 叠几层。（`1` = 只有大起伏，`3` = 加中/细节）
    pub octaves: u32,
    /// 每层频率翻几倍。
    pub lacunarity: f32,
    /// 每层振幅乘几倍。
    pub persistence: f32,
    /// 种子：同种子 = 同世界。
    pub seed: u32,
}

impl Default for NoiseParams {
    fn default() -> Self {
        Self {
            frequency: 0.045,
            octaves: 3,
            lacunarity: 2.0,
            persistence: 0.5,
            seed: 0x5EED_1234,
        }
    }
}

/// 每个**面朝向**的固定明暗系数（**内容可配**）。
///
/// ## 为什么需要它（而不是靠光照）
///
/// 立体感来自"同一个方块的不同面亮度不同"。靠真实光照去制造这个差异有两个问题：
///
/// 1. **难调**：环境光一高，各面就趋同。实测过——顶面与侧面的亮度只差 12%，
///    画面上就是一片平色块，完全没有体积感。
/// 2. **不可控**：光源方向一改，所有方块的明暗关系就全变了，
///    而这恰恰是"美术风格"（MC 风格是**约定俗成**的：顶面最亮、侧面中等、底面最暗）。
///
/// 所以这里用**固定系数**：与光源无关、与相机无关，内容说了算。
/// 系数是乘在贴图颜色上的，`1.0` = 原色。
///
/// 默认值照 MC 的惯例：顶面 `1.0`、侧面 `0.62`、底面 `0.45`。
#[derive(Debug, Clone, Copy, PartialEq, Reflect)]
pub struct FaceShade {
    /// 侧面（四个水平朝向）。
    pub side: f32,
    /// 顶面（`+Y`）。
    pub top: f32,
    /// 底面（`-Y`）。
    pub bottom: f32,
}

impl Default for FaceShade {
    fn default() -> Self {
        Self::MC_STYLE
    }
}

impl FaceShade {
    /// 同一类别（侧面 / 顶面 / 底面）的零配置默认值。
    ///
    /// MC 风格的惯例值，`voxel_render/shade.rs` 里有一条契约测试守着
    /// "顶面/侧面的亮度比 ≥ 1.4"（低于它就分不出体积，画面成一片平色块）。
    ///
    /// **试过把它调亮到 `0.78`**（怀疑等距下侧面占面积大、暗面塌成黑块），
    /// 实测**画面上那几块黑斑完全没变** —— 说明它们不是明暗造成的。
    /// 而调亮会破坏上面那条对比度契约（`1.0/0.78 = 1.28`），所以撤回。
    pub const MC_STYLE: Self = Self {
        side: 0.62,
        top: 1.0,
        bottom: 0.45,
    };

    /// 按方向轴与符号取系数（零配置版本，不依赖资源）。
    ///
    /// - `axis == 1`（竖直轴）：`+1` → 顶面、`-1` → 底面
    /// - 其它轴：侧面
    pub fn mc_style(axis: usize, sign: i32) -> f32 {
        Self::MC_STYLE.pick(axis, sign)
    }

    /// 用给定配置取系数。
    ///
    /// - `axis == 1`（竖直轴）：`+1` → 顶面、`-1` → 底面
    /// - 其它轴：侧面
    pub fn pick(&self, axis: usize, sign: i32) -> f32 {
        if axis != 1 {
            return self.side;
        }
        if sign > 0 { self.top } else { self.bottom }
    }
}

/// 地形参数（**数值内容**，可由 `world.ron` 覆盖）。
#[derive(Debug, Clone, Copy, PartialEq, Reflect)]
pub struct TerrainParams {
    /// 地表基准高度（世界 Y）。
    ///
    /// 默认 0：地表在 `y = 0` 上下小幅起伏，**俯视角相机正好从上方看它**。
    pub base_height: i32,
    /// 起伏幅度（格）。地表高度落在 `base_height ± amplitude`。
    pub amplitude: f32,
    /// 高度噪声。
    pub height: NoiseParams,
    /// 地表之下多少格是"次表层"（泥土），再往下是"深层"（石头）。
    pub soil_depth: i32,
    /// 地表方块。
    pub surface: VoxelId,
    /// 次表层方块。
    pub soil: VoxelId,
    /// 深层方块。
    pub deep: VoxelId,
    /// 世界底部（更低的格子一律实心，免得从下面看穿）。
    pub floor_y: i32,
}

impl Default for TerrainParams {
    fn default() -> Self {
        Self {
            base_height: 0,
            amplitude: 1.6,
            height: NoiseParams::default(),
            soil_depth: 2,
            // ID 由内容层注册决定；这里 0 是"未配置"的占位，`WorldPlugin` 之前没人用它生成。
            surface: VoxelId(0),
            soil: VoxelId(0),
            deep: VoxelId(0),
            floor_y: -4,
        }
    }
}

impl TerrainParams {
    /// 地表高度（世界 Y）。
    pub fn surface_height(&self, x: i32, z: i32) -> i32 {
        let n = fractal_noise(&self.height, x as f32, z as f32);
        self.base_height + (n * self.amplitude).round() as i32
    }

    /// 某格是什么方块（**程序化层的唯一入口**）。
    pub fn voxel_at(&self, pos: [i32; 3]) -> Voxel {
        let [x, y, z] = pos;
        if y < self.floor_y {
            return Voxel::solid(self.deep);
        }
        let surface = self.surface_height(x, z);
        if y > surface {
            return Voxel::AIR;
        }
        if y == surface {
            return Voxel::solid(self.surface);
        }
        if y > surface - self.soil_depth {
            return Voxel::solid(self.soil);
        }
        Voxel::solid(self.deep)
    }
}

// ------------------------------------------------------------------ 噪声

/// 分形 value noise：把 `octaves` 层不同频率 / 振幅的单层噪声加起来，归一到 `[-1, 1]`。
fn fractal_noise(params: &NoiseParams, x: f32, z: f32) -> f32 {
    let mut total = 0.0;
    let mut amplitude = 1.0;
    let mut frequency = params.frequency;
    let mut norm = 0.0;
    for octave in 0..params.octaves.max(1) {
        // 每层换一个种子，否则各层在整数格上会同时过零、叠出网格状纹路。
        let layer_seed = params.seed.wrapping_add(octave.wrapping_mul(0x9E37_79B9));
        total += value_noise(layer_seed, x * frequency, z * frequency) * amplitude;
        norm += amplitude;
        frequency *= params.lacunarity;
        amplitude *= params.persistence;
    }
    if norm == 0.0 { 0.0 } else { total / norm }
}

/// 单层 value noise：格点取伪随机值，格内用 smoothstep 插值。
fn value_noise(seed: u32, x: f32, z: f32) -> f32 {
    let x0 = x.floor();
    let z0 = z.floor();
    let tx = smoothstep(x - x0);
    let tz = smoothstep(z - z0);
    let (xi, zi) = (x0 as i32, z0 as i32);

    let c00 = lattice(seed, xi, zi);
    let c10 = lattice(seed, xi + 1, zi);
    let c01 = lattice(seed, xi, zi + 1);
    let c11 = lattice(seed, xi + 1, zi + 1);

    let top = lerp(c00, c10, tx);
    let bottom = lerp(c01, c11, tx);
    lerp(top, bottom, tz)
}

/// 格点上的伪随机值，落在 `[-1, 1]`。
///
/// **必须是纯函数**：世界靠它复现，用 `rand` 或时间做种会让"同种子同世界"失效。
fn lattice(seed: u32, x: i32, z: i32) -> f32 {
    let mut h = seed;
    h = h.wrapping_mul(0x9E37_79B9) ^ (x as u32).wrapping_mul(0x85EB_CA6B);
    h = h.wrapping_mul(0x9E37_79B9) ^ (z as u32).wrapping_mul(0xC2B2_AE35);
    // 雪崩混合，避免相邻格点线性相关（那会看起来像条纹）。
    h ^= h >> 15;
    h = h.wrapping_mul(0x2545_F491);
    h ^= h >> 13;
    // 取高位映射到 [-1, 1]。
    let unit = (h >> 8) as f32 / (1 << 23) as f32; // 0..2
    unit - 1.0
}

/// 平滑插值权重（3t² − 2t³）：让格点处导数为 0，不然地形会有明显折角。
fn smoothstep(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> TerrainParams {
        TerrainParams {
            surface: VoxelId(1),
            soil: VoxelId(2),
            deep: VoxelId(3),
            ..Default::default()
        }
    }

    #[test]
    fn same_seed_gives_the_same_world() {
        // 存档与"同种子复现"都指望这条。
        let a = params();
        let b = params();
        for x in -20..20 {
            for z in -20..20 {
                assert_eq!(a.surface_height(x, z), b.surface_height(x, z));
            }
        }
    }

    #[test]
    fn different_seed_gives_a_different_world() {
        let mut a = params();
        let mut b = params();
        a.height.seed = 1;
        b.height.seed = 2;
        let differences = (-40..40)
            .flat_map(|x| (-40..40).map(move |z| (x, z)))
            .filter(|&(x, z)| a.surface_height(x, z) != b.surface_height(x, z))
            .count();
        assert!(
            differences > 100,
            "换种子该换地形（实际差异 {differences} 格）"
        );
    }

    #[test]
    fn height_stays_within_the_configured_amplitude() {
        let p = params();
        for x in -100..100 {
            for z in -100..100 {
                let height = p.surface_height(x, z);
                assert!(
                    (height - p.base_height).abs() <= p.amplitude.ceil() as i32,
                    "({x},{z}) 高度 {height} 超出 ±{}",
                    p.amplitude
                );
            }
        }
    }

    #[test]
    fn layers_are_stacked_surface_soil_deep() {
        let p = params();
        let (x, z) = (3, 7);
        let surface = p.surface_height(x, z);

        assert_eq!(
            p.voxel_at([x, surface, z]).id,
            p.surface,
            "最上面是地表方块"
        );
        assert_eq!(
            p.voxel_at([x, surface + 1, z]),
            Voxel::AIR,
            "地表之上是空气"
        );
        assert_eq!(p.voxel_at([x, surface - 1, z]).id, p.soil, "往下是次表层");
        assert_eq!(
            p.voxel_at([x, surface - p.soil_depth - 1, z]).id,
            p.deep,
            "再往下是深层"
        );
        assert_eq!(p.voxel_at([x, p.floor_y - 1, z]).id, p.deep, "底部兜底");
    }

    #[test]
    fn terrain_is_continuous_no_cliffs_between_neighbours() {
        // 相邻格高度差不能跳变：value noise + smoothstep 的意义就在这里。
        let p = params();
        for x in -30..30 {
            for z in -30..30 {
                let here = p.surface_height(x, z);
                let right = p.surface_height(x + 1, z);
                assert!(
                    (here - right).abs() <= 2,
                    "({x},{z}) 相邻高度差 {} 太大（像断崖）",
                    (here - right).abs()
                );
            }
        }
    }

    #[test]
    fn surface_is_usually_the_topmost_solid_voxel() {
        let p = params();
        let (x, z) = (-11, 5);
        let surface = p.surface_height(x, z);
        assert!(p.voxel_at([x, surface, z]).is_solid());
        assert!(p.voxel_at([x, surface + 3, z]).is_air());
    }
}
