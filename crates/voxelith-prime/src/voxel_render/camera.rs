//! 俯视角战场相机（**可配置**）。
//!
//! ## 视角
//!
//! 正交投影 + **斜俯视**：相机在战场斜上方往下看，`X` 向右、`Z` 向屏幕下方。
//! 不用纯垂直俯视（`-Y` 直视）是因为那样立方体只剩顶面、完全看不出地形高低；
//! 斜一点既保持"看一片战场"，又能看见方块的侧面。
//!
//! ```text
//!        相机 (0, height, distance)
//!           ╲
//!            ╲  俯角 pitch
//!             ╲
//!   ───────────●──────────  战场（y ≈ 0 的平面）
//! ```
//!
//! ## 配置
//!
//! [`CameraConfig`] 是**引擎级参数**（Rust），数字本身放 `world.ron`：
//! 俯角、高度、正交缩放（视野宽度）、相机跟随谁。
//!
//! ## 为什么自己算位置而不是用 `Transform::look_at`
//!
//! 俯角与距离是"给人调的旋钮"，直接由它们算位置最直观：
//! `height = distance * sin(pitch)`、`ground = distance * cos(pitch)`。
//! `look_at` 会引入"目标点"这个中间概念，反而不好调。

use bevy::camera::ScalingMode;
use bevy::prelude::*;
use voxelith_axiom::world::{CHUNK_SIZE, ChunkPos};

/// 俯视角相机参数（**Resource**）。
#[derive(Resource, Debug, Clone, Copy, PartialEq, Reflect)]
#[reflect(Resource)]
pub struct CameraConfig {
    /// 俯角（度）：`90` = 垂直正上方看，`0` = 贴地平视。
    ///
    /// **默认 `45`**：这是"斜 45 度"的经典等距视角。
    /// 选它的理由（不是随手挑的）：
    ///
    /// - **水平与竖直缩水一样多**：`sin45 = cos45`，所以方块的两个可见面
    ///   （顶面与侧面）在屏幕上被压缩的比例相同，格子看起来是"正"的，
    ///   不会一边特别扁。
    /// - **能同时看见顶面和侧面**：`90`（垂直俯视）只剩顶面，地形像平面贴图；
    ///   角度太小（如 `20`）侧面占满画面，远处挤成一条线。
    /// - **俯视战场的惯例**：等距/斜 45 是这一视角的常规选择，玩家一眼就懂。
    ///
    /// 想要更陡的俯视就把这个数往上调（`60`–`70` 更强调地表布局），
    /// 想要更强的立体感就往下调（`30`–`40`）。
    pub pitch_degrees: f32,
    /// 方位角（度）：相机绕竖直轴转多少。
    ///
    /// ## 这一个数是"等距"与"正对网格"的分界
    ///
    /// - `0°` ⇒ 相机正对网格 ⇒ 地面格子看起来是**正正方形**，世界 `+Z` 正好朝屏幕下。
    ///   这是"俯视战场"的直白做法，但方块完全没有立体感（只能看到一个面）。
    /// - **`45°` ⇒ 等距（斜正方形）**：地面格子看起来是**菱形**，世界的**一格对角线**
    ///   朝屏幕下。方块的两个侧面都露出来，这正是经典等距/斜 45 的观感。
    ///
    /// 所以"俯视角"要真的好看，**光有俯角不够，还必须转方位角**——
    /// 只调 `pitch` 永远只能得到正正方形。
    pub azimuth_degrees: f32,
    /// 相机到注视点的距离（世界单位）。
    pub distance: f32,
    /// 正交视野宽度（世界单位）：越小越近。
    ///
    /// 正交投影下"缩放"就是它，与距离无关——这样调距离只改透视感，不改取景范围。
    ///
    /// ## 这个数决定"一格在屏幕上多大"
    ///
    /// 屏幕上一格的像素宽度 ≈ `窗口宽度 / view_width`。
    ///
    /// **实测标定过**（1280×720 窗口）：
    ///
    /// | 值 | 一格 | 观感 |
    /// |---|---|---|
    /// | `62` | 21 px | 用户："格子太大" |
    /// | `150` | 8.5 px | 格子小了，但角色几乎看不见 |
    /// | `20` | 64 px | 太近，只看到 20 格 |
    /// | **`64`** | **20 px** | **当前值**（约 64 格可见） |
    ///
    /// 参照系是 [Elin](https://elin-modding-resources.github.io/Elin.Docs/articles/10_Source%20Sheets/tile)：
    /// 它的地面格是 64 宽 × 32 高、角色精灵 32 × 48。
    ///
    /// **它与 `world.ron` 的 `sprite.world_size` 是一对**：
    /// 只改一个，角色就会相对格子变得过大或过小。
    /// 改完**必须看一眼截图** —— 格子大小没有测试能替你判断。
    pub view_width: f32,
}

impl Default for CameraConfig {
    fn default() -> Self {
        Self {
            // 45° 俯角 + 45° 方位角 = 经典等距。
            pitch_degrees: 45.0,
            azimuth_degrees: 45.0,
            // 斜 45° 下要把相机放得比俯视更远一点，否则近处的地表被裁出画面。
            distance: 88.0,
            // 视野宽度：见字段文档。
            //
            // 实测标定（1280×720 窗口）：
            //   `62`  ⇒ 一格 21 px（用户反馈"格子太大"）
            //   `150` ⇒ 一格约 8.5 px（**当前值**）
            //   `200` ⇒ 一格约 6.5 px（偏小，地形细节糊了）
            view_width: 64.0,
        }
    }
}

/// 跟随目标（**可选**）：有它就让相机跟着走。
#[derive(Component, Debug, Clone, Copy, Default, Reflect)]
#[reflect(Component)]
pub struct CameraFollow;

/// 战场相机标记。
#[derive(Component, Debug, Clone, Copy, Default, Reflect)]
#[reflect(Component)]
pub struct BattlefieldCamera;

impl CameraConfig {
    /// 相机看的是哪个世界点。
    ///
    /// **这是"取景"与"加载范围"共用的唯一真相**：地形按它居中加载，相机围着它摆。
    /// 两者用同一个值时，"**看得见的地方一定有地形**"这个不变量才成立。
    ///
    /// 踩过的坑：地形曾经恒以世界原点为中心加载，而等距相机的方位角一转
    /// （45°），视野覆盖的范围就平移了，画面上于是出现成片"没有网格"的黑块
    /// —— 看着像渲染 bug，其实是取景与加载范围没对齐。
    pub fn focus(&self) -> Vec3 {
        Vec3::ZERO
    }

    /// 注视点所在的区块：地形加载的中心。
    pub fn center_chunk(&self) -> ChunkPos {
        let focus = self.focus();
        let size = CHUNK_SIZE as i32;
        ChunkPos::new(
            (focus.x as i32).div_euclid(size),
            0,
            (focus.z as i32).div_euclid(size),
        )
    }

    /// 由俯角 / 方位角 / 距离算出相机位置（相对注视点）。
    ///
    /// ```text
    /// offset = distance · ( sin(az)·cos(pitch),  sin(pitch),  cos(az)·cos(pitch) )
    /// ```
    ///
    /// 检算：`pitch = 45°, az = 0°` ⇒ `(0, d·0.707, d·0.707)` —— 与改造前一致。
    /// `pitch = 45°, az = 45°` ⇒ `(d·0.5, d·0.707, d·0.5)` —— 相机在 `+X+Z` 象限，
    /// 于是世界的 `+X` 与 `+Z` 在屏幕上都指向**右下 / 左下**，格子成了菱形。
    pub fn offset(&self) -> Vec3 {
        let pitch = self.pitch_degrees.to_radians();
        let azimuth = self.azimuth_degrees.to_radians();
        Vec3::new(
            self.distance * azimuth.sin() * pitch.cos(),
            self.distance * pitch.sin(),
            self.distance * azimuth.cos() * pitch.cos(),
        )
    }
}

/// 起点：造相机。
///
/// 注意**不能**再用 `debug.rs` 里那个"egui 渲染目标相机"来渲染战场——
/// 那个是给检查器用的临时相机。这里给出真正的战场相机并把 `egui` 相机撤掉的话，
/// 检查器就没得画了，所以两者**共存**：检查器用它的，战场用这个。
pub fn spawn_battlefield_camera(mut commands: Commands, config: Res<CameraConfig>) {
    info!(
        "[相机] 建战场相机：俯角 {}°，方位角 {}°，距离 {}，视野宽 {}，位置 {:?}",
        config.pitch_degrees,
        config.azimuth_degrees,
        config.distance,
        config.view_width,
        config.offset()
    );
    commands.spawn((
        Name::new("battlefield-camera"),
        BattlefieldCamera,
        Camera3d::default(),
        // ---- 清屏色：**必须设在这里**（踩了很久的坑）----
        //
        // 表现：等距斜视时画面边缘 / 远处会露出约 **7.3%** 的背景，
        // 颜色是 Bevy 默认的深灰 `(43,44,47)`。它看起来**像一把深色方块**，
        // 于是先后被误判成"缺网格""缺几何""图集颜色不对"，来回查了好几轮。
        // 真相是：**它一直是清屏色，只是我没改成功。**
        //
        // 两种**不生效**的写法（都试过，都用截图证实过无效）：
        //   1. 往相机实体插 `ClearColor(color)` —— 组件插上了，但没人读它；
        //   2. `app.insert_resource(ClearColor(..))` 世界资源 —— 也没生效。
        //
        // **生效的是下面这个字段**：`Camera::clear_color`
        // （`bevy_camera::ClearColorConfig`）。它的默认值是 `Default`
        // = "取世界 `ClearColor` 资源"，而那条路在这个项目里没走通；
        // 用 `Custom(..)` 显式指定就绕开了资源查询。
        //
        // **判别方法**（下次怀疑"这里是背景吗"直接照做）：
        // 把它设成洋红 `(1, 0, 1)` 重编译再截图 —— 洋红像素应占 ~7.3%。
        // 若**一个洋红像素都没有**，说明这个字段没被应用，
        // 而**不是**"那块黑区不是背景"（我在这上面误判过一次）。
        Camera {
            clear_color: bevy::camera::ClearColorConfig::Custom(Color::srgb(0.42, 0.56, 0.72)),
            ..default()
        },
        Projection::Orthographic(OrthographicProjection {
            // 横向取景范围 = `view_width`，高度按窗口比例**自动**推出来。
            //
            // 必须用 `FixedHorizontal`，不能写 `Fixed { width, height: 0.0 }`：
            // 高度为 0 会算出退化的投影矩阵，**画面全黑而且不报错**（踩过一次）。
            scaling_mode: ScalingMode::FixedHorizontal {
                viewport_width: config.view_width,
            },
            // **不要动 near / far**：`OrthographicProjection::default_3d()` 给的是
            // `near: 0.1 / far: 1000`，而相机离地形只有约 90 —— 深度范围完全够用。
            //
            // 曾经把 `near` 写成 `-500`（以为"正交下它只是裁剪面，给足就行"），
            // 结果是**地形被裁成碎片**：反过来的深度映射把大部分面判成在近平面之外，
            // 画面上只剩零散的黑色小块和斜条纹。**默认值是对的，别为了"保险"去改它。**
            ..OrthographicProjection::default_3d()
        }),
        Transform::from_translation(config.offset()).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

/// 相机跟随：跟着 [`CameraFollow`] 目标平移（保持俯角与距离）。
pub fn follow_target(
    config: Res<CameraConfig>,
    target: Query<&Transform, (With<CameraFollow>, Without<BattlefieldCamera>)>,
    mut cameras: Query<&mut Transform, With<BattlefieldCamera>>,
) {
    if !config.is_changed() && target.is_empty() {
        return;
    }
    let Ok(target) = target.single() else {
        return;
    };
    let offset = config.offset();
    let focus = target.translation;
    for mut transform in &mut cameras {
        transform.translation = focus + offset;
        transform.look_at(focus, Vec3::Y);
    }
}

/// 注册相机。
pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CameraConfig>()
            .add_systems(Startup, spawn_battlefield_camera)
            .add_systems(Update, follow_target);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pitch_of_ninety_looks_straight_down() {
        let config = CameraConfig {
            pitch_degrees: 90.0,
            distance: 50.0,
            view_width: 64.0,
            ..Default::default()
        };
        let offset = config.offset();
        assert!((offset.x).abs() < 1e-4, "方位角 0 时该没有 x 分量");
        assert!((offset.y - 50.0).abs() < 1e-3, "垂直俯视：只在正上方");
        assert!(offset.z.abs() < 1e-3);
    }

    #[test]
    fn azimuth_rotates_the_camera_around_the_vertical_axis() {
        // **等距的关键**：光有俯角只能得到"正对网格"（正正方形），
        // 必须转方位角才有菱形地面。
        let base = CameraConfig {
            pitch_degrees: 45.0,
            distance: 100.0,
            ..Default::default()
        };
        let front = CameraConfig {
            azimuth_degrees: 0.0,
            ..base
        };
        let iso = CameraConfig {
            azimuth_degrees: 45.0,
            ..base
        };

        // 方位角 0：相机在 +Z 轴上，x 恒为 0。
        assert!(front.offset().x.abs() < 1e-4);

        // 方位角 45：x 与 z **相等**（这是等距的标志）。
        let o = iso.offset();
        assert!(
            (o.x - o.z).abs() < 1e-3,
            "方位角 45° 时 x 该等于 z，实际 {o:?}"
        );
        assert!(o.x > 0.0, "方位角 45° 时相机该在 +X+Z 象限");

        // 两者高度相同（转方位角不改俯角）。
        assert!((o.y - front.offset().y).abs() < 1e-3);
        // 距离不变（旋转不该改变到注视点的距离）。
        assert!((o.length() - 100.0).abs() < 1e-2, "旋转不该改距离");
    }

    #[test]
    fn lower_pitch_pulls_the_camera_back() {
        let high = CameraConfig {
            pitch_degrees: 60.0,
            distance: 50.0,
            ..Default::default()
        };
        let low = CameraConfig {
            pitch_degrees: 30.0,
            distance: 50.0,
            ..Default::default()
        };
        assert!(
            low.offset().z > high.offset().z,
            "俯角越小，相机越往后退（z 越大）"
        );
        assert!(low.offset().y < high.offset().y, "也越低");
    }

    #[test]
    fn offset_length_equals_distance() {
        let config = CameraConfig::default();
        let offset = config.offset();
        assert!(
            (offset.length() - config.distance).abs() < 1e-3,
            "位置到注视点的距离该等于配置的 distance"
        );
    }
}

/// 诊断节拍（帧计数）。
///
/// **不能用 `on_timer`**：`Time<Virtual>` 会被战斗系统冻结在 `AwaitingInput` 上，
/// 虚拟时间不再前进 ⇒ 计时器永远不触发 ⇒ "诊断系统注册了却一条都不打"。
/// 按帧计数就没有这个依赖。
#[derive(Resource, Debug, Default)]
pub struct DiagnosticTick(pub u32);

/// **诊断**：把场景里所有相机的实际参数打出来。
///
/// 黑屏排查时靠它拿实数（读源码猜不出来"相机到底在不在、投影是什么"）。
/// 只在最初几帧打，之后闭嘴。
pub fn log_cameras(
    mut tick: ResMut<DiagnosticTick>,
    cameras: Query<(
        Entity,
        &Camera,
        &Transform,
        Option<&Projection>,
        Option<&Camera3d>,
        Option<&Camera2d>,
    )>,
) {
    tick.0 += 1;
    if tick.0 > 3 {
        return;
    }
    for (entity, camera, transform, projection, is_3d, is_2d) in &cameras {
        let kind = match (is_3d.is_some(), is_2d.is_some()) {
            (true, _) => "Camera3d",
            (_, true) => "Camera2d",
            _ => "Camera(?)",
        };
        let projection = match projection {
            Some(Projection::Orthographic(ortho)) => format!(
                "Orthographic{{mode={:?}, near={}, far={}, scale={}}}",
                ortho.scaling_mode, ortho.near, ortho.far, ortho.scale
            ),
            Some(Projection::Perspective(_)) => "Perspective".to_owned(),
            Some(other) => format!("{other:?}"),
            None => "**没有 Projection 组件**".to_owned(),
        };
        info!(
            "[相机] {entity} {kind} order={} active={} clear={:?} pos={:?} proj={projection}",
            camera.order, camera.is_active, camera.clear_color, transform.translation,
        );
    }
    if cameras.is_empty() {
        warn!("[相机] 场景里**一个相机都没有**——窗口必然是黑的");
    }
}
