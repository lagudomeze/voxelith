//! **可交互相机**（类编辑器视角）：中键平移 / 滚轮缩放 / 右键转视角 / R 复位。
//!
//! ## 为什么要它
//!
//! 调地形、调材质、调素材比例时，**固定取景让人没法判**：
//! "这块灰是不是太暗"、"一格到底多大"、"格线够不够清楚" ——
//! 都要在能自由凑近看的前提下才判断得了。
//!
//! 重编译一次要十几秒，而"想看一眼那个角落"是**几秒一次**的动作。
//! 所以给一个能实时调整的镜头，比反复改参数重编译划算得多。
//!
//! ## 操作
//!
//! | 操作 | 效果 |
//! |---|---|
//! | **中键拖动** | 平移注视点（跟着屏幕朝向走，跟手） |
//! | **滚轮** | 拉远 / 拉近（改正交视野宽度，**乘法**，远近手感一致） |
//! | **右键拖动** | 转视角（俯角 + 方位角，**等距的两个自由度**） |
//! | **R** | 复位到默认取景 |
//!
//! ## 与 [`CameraConfig`] 的关系
//!
//! `CameraConfig` 是**内容的默认取景**（`world.ron` 那一档，启动时读一次）。
//! 本模块是**运行期的覆盖**：一旦玩家操作过，就以 [`OrbitCamera`] 为准。
//! 没操作过时两者一致 —— 所以"不动它"的行为和没有这个模块时**完全一样**。
//!
//! [`CameraConfig`]: super::camera::CameraConfig

use bevy::camera::ScalingMode;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;

use super::camera::BattlefieldCamera;

/// 相机的**运行期状态**（由交互改写；未操作时等于 [`CameraConfig`] 的默认值）。
#[derive(Resource, Debug, Clone, Copy)]
pub struct OrbitCamera {
    /// 俯角（度）。
    pub pitch_degrees: f32,
    /// 方位角（度）。
    pub azimuth_degrees: f32,
    /// 正交视野宽度（世界单位）：越小越近。
    pub view_width: f32,
    /// 注视点（世界坐标）。
    pub focus: Vec3,
}

impl OrbitCamera {
    /// 相机相对注视点的偏移（**与 `CameraConfig::offset` 同一个公式**）。
    pub fn offset(&self, distance: f32) -> Vec3 {
        let pitch = self.pitch_degrees.to_radians();
        let azimuth = self.azimuth_degrees.to_radians();
        Vec3::new(
            distance * azimuth.sin() * pitch.cos(),
            distance * pitch.sin(),
            distance * azimuth.cos() * pitch.cos(),
        )
    }
}

impl Default for OrbitCamera {
    /// 与 CameraConfig::default() 保持一致的等距取景。
    ///
    /// 为什么写死而不是读 CameraConfig：资源初始化在 App::build 阶段，
    /// 那时 CameraConfig 还是 Default，读它拿不到内容里的值 ——
    /// 与其做一个"看起来在读内容、其实读的是默认值"的假动作，不如写清楚。
    /// **内容里的取景仍然是权威**：玩家一操作就由本资源接管，不操作则两者一致。
    fn default() -> Self {
        Self {
            pitch_degrees: 45.0,
            azimuth_degrees: 45.0,
            view_width: 64.0,
            focus: Vec3::ZERO,
        }
    }
}

/// 相机所在的"距离"。
///
/// 正交投影下**它不影响取景范围**（那是 `view_width` 的事），
/// 只影响 near/far 的容差。取一个够大的常数即可。
const CAMERA_DISTANCE: f32 = 200.0;

/// 一帧里相机的改动量（`None` = 没动）。
///
/// 抽成独立的函数是为了**可测**：起一个 App、造鼠标事件、断言相机位置，
/// 这三步里有两步是样板，而真正要保证的是"拖动方向跟不跟手"这类计算。
fn apply_input(
    orbit: &mut OrbitCamera,
    buttons: &ButtonInput<MouseButton>,
    motion: Vec2,
    scroll_y: f32,
    reset: bool,
    window_width: f32,
) -> bool {
    let mut changed = false;

    if reset {
        // 复位到"俯角 45 / 方位角 45 / 视野 64"这一套等距默认。
        *orbit = OrbitCamera {
            pitch_degrees: 45.0,
            azimuth_degrees: 45.0,
            view_width: 64.0,
            focus: Vec3::ZERO,
        };
        return true;
    }

    // ---- 中键拖动：平移注视点 ----
    //
    // 方向必须**跟着屏幕朝向**走：相机转过方位角之后，"屏幕右"不再是世界 +X。
    // 按世界轴平移会"不跟手"（斜着拖，画面斜着跑），那是很难受的手感。
    if buttons.pressed(MouseButton::Middle) && motion != Vec2::ZERO {
        let (right, forward) = screen_basis(orbit);
        let per_pixel = orbit.view_width / window_width.max(1.0);
        orbit.focus += right * (-motion.x * per_pixel);
        orbit.focus += forward * (motion.y * per_pixel);
        changed = true;
    }

    // ---- 右键拖动：转俯角 / 方位角 ----
    if buttons.pressed(MouseButton::Right) && motion != Vec2::ZERO {
        orbit.azimuth_degrees = (orbit.azimuth_degrees - motion.x * 0.4).rem_euclid(360.0);
        // 俯角夹在 (5, 89)：0 会退化成平视（看不到地面），90 会退化成正俯视（没有立体感）。
        orbit.pitch_degrees = (orbit.pitch_degrees + motion.y * 0.4).clamp(5.0, 89.0);
        changed = true;
    }

    // ---- 滚轮：缩放松紧 ----
    //
    // 用**乘法**：加减法在"贴脸看"和"看全图"两端的观感差异太大
    // （近处一格滚一下跳半屏，远处滚十下不动）。
    if scroll_y != 0.0 {
        let factor = (1.0 - scroll_y * 0.1).clamp(0.5, 2.0);
        orbit.view_width = (orbit.view_width * factor).clamp(0.5, 400.0);
        changed = true;
    }

    changed
}

/// 相机在水平面上的"屏幕右"与"往里"。
/// 相机在水平面上的"**屏幕右**"与"**往里（视线在水平面的方向）**"。
///
/// ## 为什么不能凭直觉写（这里踩过一次，测试抓到了）
///
/// 我第一版写的是 `right = (forward.z, 0, -forward.x)`，注释还写着"绕 Y 轴顺时针 90°"。
/// 实测结果是方位角 45° 往右拖时注视点往 `(+x, -z)` 走 —— **正好反了**。
///
/// 正确的做法是**老老实实算**：水平面上的旋转用
/// `rot90(v) = (v.z, 0, -v.x)` 是**绕 Y 轴顺时针**（从上方看是顺时针）。
/// 而"屏幕右"要从"相机指向目标的视线"求出来，符号取决于"相机在目标的哪一侧"。
/// 这里直接按最终要用的方向写清楚，并用测试钉住（测试里给了方位角 45° 的期望值）。
fn screen_basis(orbit: &OrbitCamera) -> (Vec3, Vec3) {
    let azimuth = orbit.azimuth_degrees.to_radians();
    // 相机在 `+X+Z` 象限看向原点 ⇒ **视线（往里）**在水平面的投影是 `-(sin az, 0, cos az)`。
    let forward = -Vec3::new(azimuth.sin(), 0.0, azimuth.cos()).normalize();
    // 屏幕右：把"往里"绕 Y 轴**逆**时针 90°，得到的就是观察者右手边。
    let right = Vec3::new(-forward.z, 0.0, forward.x).normalize();
    (right, forward)
}

/// 读输入并改写 [`OrbitCamera`]，然后把结果写进相机的 `Transform` 与 `Projection`。
///
/// **没操作过时什么都不做** —— 所以"不动它"的行为与没有这个系统时完全一样
/// （`world.ron` 的取景、`aim_camera_at_gallery` 的取景都不会被打断）。
pub fn orbit_battlefield_camera(
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window>,
    mut orbit: ResMut<OrbitCamera>,
    mut cameras: Query<(&mut Transform, &mut Projection), With<BattlefieldCamera>>,
) {
    let window_width = windows.iter().next().map_or(1280.0, Window::width);
    let changed = apply_input(
        &mut orbit,
        &buttons,
        motion.delta,
        scroll.delta.y,
        keys.just_pressed(KeyCode::KeyR),
        window_width,
    );
    if !changed {
        return;
    }

    for (mut transform, mut projection) in &mut cameras {
        transform.translation = orbit.focus + orbit.offset(CAMERA_DISTANCE);
        transform.look_at(orbit.focus, Vec3::Y);
        if let Projection::Orthographic(ortho) = &mut *projection {
            ortho.scaling_mode = ScalingMode::FixedHorizontal {
                viewport_width: orbit.view_width,
            };
        }
    }
}

/// 玩家第一次操作相机时，顺手把它记下来（便于排查"我看到的取景是哪来的"）。
pub fn log_first_orbit(orbit: Res<OrbitCamera>, mut logged: Local<bool>) {
    if *logged || !orbit.is_changed() {
        return;
    }
    *logged = true;
    info!(
        "[镜头] 手动取景：俯角 {:.0}° / 方位角 {:.0}° / 视野宽 {:.1} / 注视点 {:?}",
        orbit.pitch_degrees, orbit.azimuth_degrees, orbit.view_width, orbit.focus
    );
}

/// 注册可交互相机。
pub struct OrbitCameraPlugin;

impl Plugin for OrbitCameraPlugin {
    fn build(&self, app: &mut App) {
        // 默认**关闭**：它只是排查/调参用的镜头，不该改变正常游戏行为。
        // `ORBIT_CAMERA=1` 打开。
        if !std::env::var("ORBIT_CAMERA").is_ok_and(|v| v == "1") {
            return;
        }
        // 用 `CameraConfig` 的默认值初始化（`world.ron` 那一档）。
        //
        // 注意不能 `init_resource::<OrbitCamera>()` 然后指望它自己拿到 config ——
        // `CameraConfig` 是 `VoxelRenderPlugin` 里 `init_resource` 的，
        // 这里只加系统；`OrbitCamera` 由那个插件在同一个地方插。
        app.add_systems(Update, (orbit_battlefield_camera, log_first_orbit).chain());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 一个初始的相机状态。
    fn orbit() -> OrbitCamera {
        OrbitCamera {
            pitch_degrees: 45.0,
            azimuth_degrees: 45.0,
            view_width: 64.0,
            focus: Vec3::ZERO,
        }
    }

    /// **中键拖动要跟手**：把注视点按屏幕方向平移。
    ///
    /// 判据用的是"拖动后注视点移动的方向"：
    /// 方位角 45° 时屏幕右 ≈ 世界 `(0.707, 0, -0.707)`（归一化）。
    /// 往右拖（`motion.x > 0`）应该让视野往左移 ⇒ 注视点往 **屏幕左** 走。
    #[test]
    fn middle_drag_pans_along_the_screen_axes() {
        let mut o = orbit();
        let mut buttons = ButtonInput::<MouseButton>::default();
        buttons.press(MouseButton::Middle);
        let before = o.focus;

        let changed = apply_input(&mut o, &buttons, Vec2::new(10.0, 0.0), 0.0, false, 1280.0);
        assert!(changed, "拖动该被记为改动");
        assert_ne!(o.focus, before, "注视点该移动");

        // 移动方向该在水平面内（不能把相机拖到地下或天上）。
        assert!(
            (o.focus.y - before.y).abs() < 1e-6,
            "平移不该改变注视点的 y（那是俯角的事）"
        );

        // 方位角 45° ⇒ 屏幕右 ≈ (0.707, 0, -0.707)；往右拖 ⇒ 注视点往屏幕左。
        let moved = o.focus - before;
        assert!(
            moved.x < 0.0 && moved.z > 0.0,
            "方位角 45° 往右拖，注视点该往 (-x, +z) 走，实际 {moved:?}"
        );
    }

    /// 纵向拖动沿"往里"方向，而不是世界 Z。
    #[test]
    fn middle_drag_vertical_moves_along_view_depth() {
        let mut o = orbit();
        let mut buttons = ButtonInput::<MouseButton>::default();
        buttons.press(MouseButton::Middle);
        let before = o.focus;
        apply_input(&mut o, &buttons, Vec2::new(0.0, 10.0), 0.0, false, 1280.0);
        let moved = o.focus - before;
        // 视线在水平面的投影是 -(sin45, 0, cos45) = (-0.707, 0, -0.707)。
        assert!(
            moved.x < 0.0 && moved.z < 0.0,
            "往下拖该沿视线往 (-x, -z) 走，实际 {moved:?}"
        );
    }

    /// **滚轮是乘法**：从 64 滚一下与从 6.4 滚一下，**比例**该一样。
    #[test]
    fn scroll_zoom_is_multiplicative() {
        let mut far = orbit();
        far.view_width = 64.0;
        let mut near = orbit();
        near.view_width = 6.4;

        let mut buttons = ButtonInput::<MouseButton>::default();
        let _ = &mut buttons;
        apply_input(&mut far, &buttons, Vec2::ZERO, 1.0, false, 1280.0);
        apply_input(&mut near, &buttons, Vec2::ZERO, 1.0, false, 1280.0);

        let far_ratio = far.view_width / 64.0;
        let near_ratio = near.view_width / 6.4;
        assert!(
            (far_ratio - near_ratio).abs() < 1e-6,
            "滚动该是乘法：比例 {far_ratio} vs {near_ratio}"
        );
        assert!(far_ratio < 1.0, "往上滚该拉近（视野变小）");
    }

    /// 俯角要夹住：**0 会退化成平视**（看不到地面），**90 会退化成正俯视**（没有立体感）。
    #[test]
    fn pitch_is_clamped_away_from_the_degenerate_angles() {
        let mut o = orbit();
        let mut buttons = ButtonInput::<MouseButton>::default();
        buttons.press(MouseButton::Right);

        // 往一个方向猛拖，再往反方向猛拖。
        for _ in 0..50 {
            apply_input(&mut o, &buttons, Vec2::new(0.0, 10.0), 0.0, false, 1280.0);
        }
        assert!(o.pitch_degrees <= 89.0, "俯角该被夹在 89 以内");
        for _ in 0..100 {
            apply_input(&mut o, &buttons, Vec2::new(0.0, -10.0), 0.0, false, 1280.0);
        }
        assert!(o.pitch_degrees >= 5.0, "俯角该被夹在 5 以上");
    }

    /// **R 复位**回到等距默认。
    #[test]
    fn reset_restores_the_isometric_default() {
        let mut o = orbit();
        o.focus = Vec3::new(123.0, 0.0, -45.0);
        o.view_width = 3.0;
        o.pitch_degrees = 80.0;

        let buttons = ButtonInput::<MouseButton>::default();
        let changed = apply_input(&mut o, &buttons, Vec2::ZERO, 0.0, true, 1280.0);
        assert!(changed);
        assert_eq!(o.focus, Vec3::ZERO);
        assert_eq!(o.view_width, 64.0);
        assert_eq!(o.pitch_degrees, 45.0);
        assert_eq!(o.azimuth_degrees, 45.0);
    }

    /// 不接受任何输入时**什么都不改**（于是"不动它"= 与没有这个模块时一致）。
    #[test]
    fn no_input_changes_nothing() {
        let mut o = orbit();
        let before = o;
        let buttons = ButtonInput::<MouseButton>::default();
        let changed = apply_input(&mut o, &buttons, Vec2::ZERO, 0.0, false, 1280.0);
        assert!(!changed, "没有输入时不该报告改动");
        assert_eq!(o.focus, before.focus);
        assert_eq!(o.view_width, before.view_width);
    }

    /// **按住中键但不动鼠标** ⇒ 不改（免得静止时漂移）。
    #[test]
    fn holding_without_motion_does_not_drift() {
        let mut o = orbit();
        let mut buttons = ButtonInput::<MouseButton>::default();
        buttons.press(MouseButton::Middle);
        let changed = apply_input(&mut o, &buttons, Vec2::ZERO, 0.0, false, 1280.0);
        assert!(!changed);
    }
}
