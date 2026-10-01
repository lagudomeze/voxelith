//! 最小 glTF 渲染探针 + 可交互相机。
//!
//! ## 这个 crate 为什么存在
//!
//! `voxelith-prime` 里从 `modular-cave-kit` 加载的 GLB 模型**整体偏暗**
//! （实测：白色 albedo 渲染成 `(57,57,57)`，只有 22%；橙色 `(184,80,30)` →
//! `(40,15,8)`，同样是 ~22%）。
//!
//! 我试过把光照调亮 25 倍 —— **亮度完全不变**。所以那不是光照问题，
//! 但我一直没找出原因，因为 `voxelith-prime` 里有太多变量：
//! 地形材质、相机投影、环境光、后期、场景结构……
//!
//! 这个 crate 是**对照实验**：剥掉所有 voxelith 代码，用最简单的 Bevy 场景
//! 渲染同一个 GLB。
//!
//! - **模型正常** ⇒ 问题在我们项目里的某个配置；
//! - **模型照样黑** ⇒ 问题在 Bevy 的 glTF 加载/材质，或者素材本身。
//!
//! ## 怎么跑
//!
//! ```powershell
//! cd probe/glb
//! cargo run
//! ```
//!
//! 换模型：`$env:GLB = "block-grass.glb"; cargo run`
//!
//! ## 操作
//!
//! | 操作 | 效果 |
//! |---|---|
//! | **中键拖动** | 平移注视点（在水平面上） |
//! | **滚轮** | 拉远 / 拉近（改正交视野宽度） |
//! | **右键拖动** | 转视角（俯角 / 方位角，**等距的两个自由度**） |
//! | **R** | 复位 |
//! | **Esc** | 退出 |
//!
//! 相机模型与 `voxelith` 的 `CameraConfig` 一致（俯角 + **方位角** + 正交视野宽度），
//! 这样"取景方式"不会成为混淆变量。

use bevy::camera::ScalingMode;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;

fn main() {
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "glb-probe（中键拖动=平移，滚轮=缩放，右键拖动=转视角，R=复位）"
                            .into(),
                        resolution: (1280, 720).into(),
                        ..default()
                    }),
                    ..default()
                })
                // **指向工作区的 `assets/`**（不是探针自己再拷一份）。
                //
                // 探针曾经自带一份 **7.1 MB** 的素材副本 —— 那是纯冗余：
                // GLB 与贴图主仓库本来就有。指向同一份还有个额外好处：
                // 不会出现"探针里的素材和产品里的不一致"这种假象。
                //
                // 为什么必须显式指定：Bevy 的默认资产根是**可执行文件旁边**的 `assets/`
                // （`target/debug/assets`），而 exe 在共享的工作区 target 里。
                // 用 `CARGO_MANIFEST_DIR`（编译期常量）—— 不用管运行期当前目录。
                .set(bevy::asset::AssetPlugin {
                    file_path: concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets").to_string(),
                    // **关掉文件监听。**
                    //
                    // 一个没查过的变量：debug 构建下 Bevy 默认会开文件监听，
                    // 而它在某些环境下会让**加载永远挂住**（状态停在 `Loading`、
                    // 既不完成也不报错）。关掉它是为了排除这一条。
                    watch_for_changes_override: Some(false),
                    ..default()
                })
                // **把 `bevy_asset` 的日志开到 DEBUG**：外链贴图加载失败时
                // Bevy 往往**不报错**，只有 debug 级别才留下痕迹 ——
                // 这是唯一能看见"它到底在找哪个路径"的办法。
                .set(bevy::log::LogPlugin {
                    filter: "info,bevy_asset=debug,bevy_gltf=debug".to_string(),
                    level: bevy::log::Level::DEBUG,
                    ..default()
                }),
        )
        // `orbit_camera` 要这个资源；不初始化它，系统参数校验就会失败
        // （报错是 `ResMut<OrbitCamera> failed validation: Resource does not exist`，
        // 而**调度器会连带把整个 App 干掉** —— 我之前删重复代码时误删了这一行）。
        .init_resource::<OrbitCamera>()
        .add_systems(Startup, setup)
        .add_systems(Update, diagnose_materials)
        .add_systems(Update, (orbit_camera, quit_on_escape, dump_materials_once))
        .run();
}

/// **把场景里每个 `StandardMaterial` 的实际字段值打印出来**（只打一次）。
///
/// ## 为什么要它，而不是继续看截图
///
/// 实测：同一个场景、同一套灯，
/// 我们自己的 `StandardMaterial`（`base_color 0.6` 灰）渲染成 `(108,108,109)`，
/// 而 GLB 的 `colormap` 材质渲染成 `(18,19,25)` —— 暗了约 **4.5 倍**。
///
/// 从截图反推"是贴图 / 光照 / 色彩空间"已经绕了很多轮。
/// 直接读**实际生效的材质字段**才是决定性的：
/// - `base_color` 不是白 ⇒ 整体乘了一个系数；
/// - `unlit` 意外为真 ⇒ 完全不走光照；
/// - `base_color_texture` 为空 ⇒ 贴图根本没绑上；
/// - `metallic` 高 / `roughness` 低 ⇒ 反射走偏，漫反射被吃掉。
///
/// ## 为什么遍历 `Assets<StandardMaterial>` 而不是查实体的句柄
///
/// 第一版查 `MeshMaterial3d` 的实体句柄，结果**只抓到了自己 spawn 的地面立方体** ——
/// GLB 的材质挂在 `SceneRoot` 展开出来的**子实体**上，而那时场景还没展开完。
/// 直接遍历资产表更稳，也不受"场景加载到哪一步"影响。
///
/// 用一个 `Local<u32>` 当帧计数：等几帧再打，避开"资产还没读进来"的窗口。
fn dump_materials_once(
    materials: Res<Assets<StandardMaterial>>,
    mut frame: Local<u32>,
    mut done: Local<bool>,
) {
    if *done {
        return;
    }
    *frame += 1;
    // 等几帧：GLB 是异步加载的，第 1 帧资产表里只有我们自己 spawn 的那个。
    if *frame < 30 {
        return;
    }
    *done = true;

    info!("材质转储：场景里共 {} 个 StandardMaterial", materials.len());
    for (index, (id, material)) in materials.iter().enumerate() {
        info!(
            "  [{index}] id={id:?} base_color={:?} unlit={} metallic={:.3} \
             roughness={:.3} reflectance={:.3} 有基础色贴图={} alpha_mode={:?} \
             cull={:?} double_sided={}",
            material.base_color,
            material.unlit,
            material.metallic,
            material.perceptual_roughness,
            material.reflectance,
            material.base_color_texture.is_some(),
            material.alpha_mode,
            material.cull_mode,
            material.double_sided,
        );
    }
}

/// 相机状态（Resource）。
///
/// **字段与 `voxelith::CameraConfig` 一一对应**：俯角、方位角、视野宽度、注视点。
#[derive(Resource, Debug, Clone, Copy)]
struct OrbitCamera {
    /// 俯角（度）：90 = 正上方看。
    pitch_degrees: f32,
    /// 方位角（度）：绕竖直轴转。
    azimuth_degrees: f32,
    /// 正交视野宽度（世界单位）：越小越近。
    view_width: f32,
    /// 注视点（世界坐标）。
    focus: Vec3,
}

impl Default for OrbitCamera {
    fn default() -> Self {
        Self {
            pitch_degrees: 45.0,
            azimuth_degrees: 45.0,
            view_width: 6.0,
            focus: Vec3::ZERO,
        }
    }
}

impl OrbitCamera {
    /// 相机相对注视点的偏移（**与 `voxelith` 的公式一致**）。
    fn offset(&self, distance: f32) -> Vec3 {
        let pitch = self.pitch_degrees.to_radians();
        let azimuth = self.azimuth_degrees.to_radians();
        Vec3::new(
            distance * azimuth.sin() * pitch.cos(),
            distance * pitch.sin(),
            distance * azimuth.cos() * pitch.cos(),
        )
    }
}

/// 加载的文件名（可用 `GLB` 环境变量换）。
fn glb_name() -> String {
    std::env::var("GLB").unwrap_or_else(|_| "dungeon/template-floor.glb".to_owned())
}

fn setup(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let name = glb_name();

    // ---- 相机：正交 + 等距（与 voxelith 一致，免得投影方式成为混淆变量）----
    let orbit = OrbitCamera::default();
    commands.spawn((
        Name::new("probe-camera"),
        Camera3d::default(),
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::FixedHorizontal {
                viewport_width: orbit.view_width,
            },
            ..OrthographicProjection::default_3d()
        }),
        Transform::from_translation(orbit.offset(20.0)).looking_at(orbit.focus, Vec3::Y),
    ));

    // ---- 灯光：**与 voxelith 的 terrain-light / terrain-ambient 用同样的数** ----
    //
    // 这是关键对照：如果这边正常而产品那边黑，说明差别不在光本身。
    commands.spawn((
        Name::new("probe-light"),
        DirectionalLight {
            // 光照强度也可用环境变量扫（见 README 的"排查步骤"）。
            illuminance: env_f32("ILLUM").unwrap_or(12_000.0),
            shadow_maps_enabled: false,
            contact_shadows_enabled: false,
            ..default()
        },
        Transform::from_xyz(15.0, 15.0, 0.6).looking_at(Vec3::ZERO, Vec3::Z),
    ));
    // **Bevy 0.19：全局环境光是 Resource，不是 Component。**
    // `AmbientLight` 现在 `#[require(Camera)]`（它是"挂在相机上的覆盖值"），
    // spawn 它会凭空造出一个没有渲染图的裸相机，并每帧刷警告。
    commands.insert_resource(GlobalAmbientLight {
        color: Color::WHITE,
        brightness: env_f32("AMBIENT").unwrap_or(60.0),
        affects_lightmapped_meshes: true,
    });

    // ---- 地面参照物 ----
    //
    // **不透明**的亮色标准材质。它同时干两件事：
    // 1. 做尺度参照（能看出模型多大）；
    // 2. **亮度对照** —— 同一个场景、同一套灯，它亮而模型黑，
    //    就说明问题出在模型/材质上，不在光上。
    commands.spawn((
        Name::new("probe-ground"),
        Mesh3d(meshes.add(Cuboid::new(6.0, 0.2, 6.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.6, 0.6, 0.6),
            metallic: 0.0,
            perceptual_roughness: 0.9,
            ..default()
        })),
        Transform::from_xyz(0.0, -0.1, 0.0),
    ));

    // ---- 主角：那个 GLB ----
    //
    // Bevy 0.19 的写法：`WorldAssetRoot` + `GltfAssetLabel::Scene(0)`。
    // `#Scene0` 标签**必须有**，否则 Bevy 不知道该取文件里的哪一部分。
    let path = format!("models/{name}");
    commands.spawn((
        Name::new("probe-model"),
        WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(path.clone()))),
        Transform::default(),
    ));

    info!(
        "glb-probe：加载 {path}；相机 正交/视野宽 {:.1}/俯角 {:.0}°/方位角 {:.0}°；\
         平行光 {:.0} / 环境光 {:.0}",
        orbit.view_width,
        orbit.pitch_degrees,
        orbit.azimuth_degrees,
        env_f32("ILLUM").unwrap_or(12_000.0),
        env_f32("AMBIENT").unwrap_or(60.0),
    );
}

/// **可切换的材质排查开关**（全部注释掉了，用的时候放开一行）。
///
/// 每一行排除一类原因：
///
/// | 放开这行 | 排除什么 |
/// |---|---|
/// | `mat.metallic = 0.0` | 金属度导致漫反射被吃掉（**GLB 里金属度默认是 1.0**，这是首要怀疑） |
/// | `mat.base_color = 红色` | 贴图/顶点色问题（强制纯色，看还黑不黑） |
/// | `mat.cull_mode = None` | 背面剔除（法线朝内 ⇒ 只看到背面） |
/// | `mat.unlit = true` | 完全绕过光照，只看几何对不对 |
/// | `mat.base_color_texture = None` | 贴图本身（拿掉它看是否变白） |
///
/// 用 `#[allow]` 是因为**放开第一行之前整个函数体是空的** ——
/// 那会让 `mats` 与 `mat` 变成未使用/不需要 `mut`。
/// 加 `#[allow]` 比为了消警告而**删掉这些开关**好：它们就是这个函数存在的意义。
#[allow(unused_mut, unused_variables)]
fn diagnose_materials(
    q: Query<&MeshMaterial3d<StandardMaterial>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    for m in &q {
        if let Some(mut mat) = mats.get_mut(&m.0) {
            // **实验 1：金属度 -- 已排除。**
            // glTF 的 `metallicFactor` 默认 1.0，而金属没 IBL 时会发黑，
            // 但材质转储显示 Kenney 的模型本来就是 `metallic = 0.000`，
            // 强制 0 之后**画面没有任何变化**。所以不是它。
            // mat.metallic = 0.0;

            // **实验 2：`base_color` -- 已排除。**
            // 材质转储里出现过纯红/洋红，但强制成白色之后**画面无变化**，
            // 说明生效的那份材质基色本来就是白的。（转储看到的是另一份。）
            // mat.base_color = Color::WHITE;

            // **实验 3：拿掉贴图。**
            //
            // 如果模型变白 ⇒ 问题在**贴图采样/色彩空间**；
            // 如果还是黑 ⇒ 问题在网格（法线）或光照求值。
            // mat.base_color_texture = None;

            // **实验 4：完全绕过光照。**
            //
            // `unlit` 直接把 `base_color`（乘贴图）输出，不参与任何光照计算。
            // 如果 unlit 之后颜色正常 ⇒ 问题在光照求值；
            // 如果 unlit 之后照样黑 ⇒ 问题在贴图本身。
            // mat.unlit = true;
            // mat.cull_mode = None; // 排除背面剔除
        }
    }
}

fn env_f32(name: &str) -> Option<f32> {
    std::env::var(name).ok()?.parse().ok()
}

/// 相机交互：中键平移、滚轮缩放、右键转视角、R 复位。
///
/// ## 为什么"转视角"要有两个自由度
///
/// 等距视图由 **俯角 + 方位角** 共同决定（见 `voxelith` 的 `CameraConfig` 文档）。
/// 只调俯角永远只能得到"正对方格"的视角，格子不会是菱形 ——
/// 排查渲染问题时很容易把"看着不对"归到材质上，其实是视角问题。
fn orbit_camera(
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    keys: Res<ButtonInput<KeyCode>>,
    mut orbit: ResMut<OrbitCamera>,
    mut cameras: Query<(&mut Transform, &mut Projection), With<Camera3d>>,
) {
    let mut changed = false;

    // ---- R：复位 ----
    if keys.just_pressed(KeyCode::KeyR) {
        *orbit = OrbitCamera::default();
        changed = true;
    }

    // ---- 中键拖动：平移注视点（在水平面上）----
    //
    // 拖动的方向要**跟着屏幕朝向**走：相机转了方位角之后，
    // "屏幕右"已经不是世界 +X 了。按屏幕基向量换算，拖动方向才跟手。
    if mouse_buttons.pressed(MouseButton::Middle) && motion.delta != Vec2::ZERO {
        let (right, forward) = screen_basis(&orbit);
        // 世界单位 / 像素：正交下就是"视野宽度 / 窗口宽度"。
        let per_pixel = orbit.view_width / 1280.0;
        orbit.focus += right * (-motion.delta.x * per_pixel);
        orbit.focus += forward * (motion.delta.y * per_pixel);
        changed = true;
    }

    // ---- 右键拖动：转俯角 / 方位角 ----
    if mouse_buttons.pressed(MouseButton::Right) && motion.delta != Vec2::ZERO {
        orbit.azimuth_degrees -= motion.delta.x * 0.4;
        orbit.pitch_degrees = (orbit.pitch_degrees + motion.delta.y * 0.4).clamp(5.0, 89.0);
        // 角度绕回，免得数字无限增长。
        orbit.azimuth_degrees = orbit.azimuth_degrees.rem_euclid(360.0);
        changed = true;
    }

    // ---- 滚轮：缩放松紧（改正交视野宽度）----
    //
    // 用**乘法**：加减法在远近两端的观感差异太大。
    if scroll.delta.y != 0.0 {
        let factor = (1.0 - scroll.delta.y * 0.1).clamp(0.5, 2.0);
        orbit.view_width = (orbit.view_width * factor).clamp(0.3, 60.0);
        changed = true;
    }

    if !changed {
        return;
    }
    for (mut transform, mut projection) in &mut cameras {
        // 正交下"距离"不影响取景范围（只影响近/远平面），取一个够大的常数即可。
        transform.translation = orbit.focus + orbit.offset(20.0);
        transform.look_at(orbit.focus, Vec3::Y);
        if let Projection::Orthographic(ortho) = &mut *projection {
            ortho.scaling_mode = ScalingMode::FixedHorizontal {
                viewport_width: orbit.view_width,
            };
        }
    }
}
/// 相机在水平面上的"**屏幕右**"与"**往里（视线在水平面的方向）**"。
///
/// 水平面上的旋转用 `rot90(v) = (v.z, 0, -v.x)` 是绕 Y 轴顺时针（从上方看）。
/// "屏幕右"要从"相机指向目标的视线"求出来，符号取决于相机在目标的哪一侧 ——
/// 所以这里是**按最终要用的方向写清楚**，而不是凭直觉推。
fn screen_basis(orbit: &OrbitCamera) -> (Vec3, Vec3) {
    let azimuth = orbit.azimuth_degrees.to_radians();
    // 相机在 `+X+Z` 象限看向原点 ⇒ 视线（往里）在水平面的投影是 `-(sin az, 0, cos az)`。
    let forward = -Vec3::new(azimuth.sin(), 0.0, azimuth.cos()).normalize();
    // 屏幕右：把"往里"绕 Y 轴**逆**时针 90°。
    let right = Vec3::new(-forward.z, 0.0, forward.x).normalize();
    (right, forward)
}

fn quit_on_escape(keys: Res<ButtonInput<KeyCode>>, mut exit: MessageWriter<AppExit>) {
    if keys.just_pressed(KeyCode::Escape) {
        exit.write(AppExit::Success);
    }
}
