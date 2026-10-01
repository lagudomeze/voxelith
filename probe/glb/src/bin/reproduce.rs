//! **复现实验**：用 gallery 的**相机 + 光照参数**渲染同一个 GLB。
//!
//! ## 进度回顾（这决定了这个实验要做什么）
//!
//! 已经确认：
//!
//! | 场景 | 同一个 `template-floor.glb` 的渲染值 |
//! |---|---|
//! | `probe/glb` 的隔离场景（相机视野 26、灯 `(1,2,0.6)`、光强 12000） | **`(103,108,134)` 正常**（贴图原色 `(90,96,123)`） |
//! | `voxelith-prime` 的展示台（相机视野 **164.7**、物体在 **y=40**） | **发黑** |
//!
//! 已排除：物体高度（`GROUND_Y=40` 无变化）、地板网格（关掉照样黑）、
//! 光照强度（400 倍无变化）、环境光（250 倍无变化）、金属度、顶点色、
//! 法线、贴图加载（文件在、格式 `Rgba8UnormSrgb`）。
//!
//! ⇒ 剩下的差异就是**两个场景的参数**。这个 bin 把它们做成环境变量，
//! 一次只改一个，逐条二分：
//!
//! | 环境变量 | 含义 |
//! |---|---|
//! | `VIEW_WIDTH` | 正交视野宽度（isolate 用 26，gallery 用 164.7） |
//! | `ILLUM` | 平行光照度 |
//! | `AMBIENT` | 环境光亮度 |
//! | `LIGHT_DIR` | 灯的位置（决定方向） |
//! | `TILE_SCALE` | 瓦片缩放（gallery 用 0.25，isolate 用 1.0） |
//! | `TILT` | 是否像 gallery 那样**不旋转**模型（isolate 里我把 floor 立起来了） |
//!
//! ## 最可疑的一条
//!
//! **isolate 里我把 `template-floor` 转了 -90°（立起来对着相机）**，
//! 于是它的法线**正对光源** ⇒ `N·L ≈ 1` ⇒ 最亮。
//! 而 gallery 里地板瓦片**平放**、墙**立着**，法线方向不同 ⇒ `N·L` 小得多。
//!
//! **这可能根本不是 bug，而是"法线朝向 × 光方向"的正常结果。**
//! 本实验用 `TILT=1` 让两边姿态一致，就能分辨这一点。

use bevy::prelude::*;

fn env_f32(name: &str, default: f32) -> f32 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn main() {
    let view_width = env_f32("VIEW_WIDTH", 26.0);
    let illum = env_f32("ILLUM", 12_000.0);
    let ambient = env_f32("AMBIENT", 60.0);
    let tile_scale = env_f32("TILE_SCALE", 1.0);
    let tilt = std::env::var("TILT").is_ok_and(|v| v == "1");
    let light_dir: Vec3 = std::env::var("LIGHT_DIR")
        .ok()
        .and_then(|v| {
            let n: Vec<f32> = v.split(',').filter_map(|s| s.trim().parse().ok()).collect();
            (n.len() == 3).then(|| Vec3::new(n[0], n[1], n[2]))
        })
        .unwrap_or(Vec3::new(1.0, 2.0, 0.6));

    info!(
        "复现实验：视野 {view_width} 照度 {illum} 环境光 {ambient} \
         瓦片缩放 {tile_scale} 立起 {tilt} 灯 {light_dir:?}"
    );

    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: format!("复现：视野{view_width} 照度{illum} 环境光{ambient}"),
                        resolution: (1280, 720).into(),
                        ..default()
                    }),
                    ..default()
                })
                // **指向工作区的 `assets/`**（不是探针自己再拷一份）。
                //
                // 探针曾经自带一份 7.1 MB 的素材副本 —— 那是纯冗余：
                // GLB 与贴图主仓库本来就有。指向同一份还避免了
                // "探针里的素材和产品里的不一致"这种假象。
                .set(bevy::asset::AssetPlugin {
                    file_path: concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets").to_string(),
                    ..default()
                }),
        )
        .add_systems(
            Startup,
            move |commands: Commands, assets: Res<AssetServer>| {
                setup(
                    commands, assets, view_width, illum, ambient, tile_scale, tilt, light_dir,
                );
            },
        )
        .add_systems(
            Update,
            (
                make_unlit,
                report,
                report_light,
                report_entities,
                quit_on_escape,
            ),
        )
        .run();
}

/// **把灯的实际全局朝向打出来。**
///
/// `DirectionalLight` 沿 transform 的 **`-Z`** 发光（`bevy_light` 的文档原文：
/// "The light shines along the forward direction of the entity's transform"）。
///
/// `looking_at(target, up)` 让 `-Z` 指向 target —— **但 `up` 与视线方向共线时会退化**，
/// 基向量被算歪，实际朝向就不是预期的方向。
///
/// **这一条必须读实际值**：我按公式手算过两次，两次都得出错误结论。
/// 只要把 `forward()` 打出来，方向问题就一眼可见。
fn report_light(
    lights: Query<(&GlobalTransform, &DirectionalLight)>,
    mut frame: Local<u32>,
    mut done: Local<bool>,
) {
    if *done {
        return;
    }
    *frame += 1;
    if *frame < 30 {
        return;
    }
    *done = true;
    info!("report_light：查到 {} 盏平行光", lights.iter().count());
    for (transform, light) in &lights {
        let forward = transform.forward();
        info!(
            "灯：forward(-Z)={:?} 照度={} 位置={:?}",
            forward,
            light.illuminance,
            transform.translation()
        );
        // 竖直分量：-1.0 = 完全垂直向下照；0.0 = 水平照。
        info!(
            "    竖直分量 {:.3}（光实际的\"往下程度\"，越接近 1 越垂直）",
            -forward.y
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn setup(
    mut commands: Commands,
    assets: Res<AssetServer>,
    view_width: f32,
    illum: f32,
    ambient: f32,
    tile_scale: f32,
    tilt: bool,
    light_dir: Vec3,
) {
    // ---- 相机：与 gallery 同一套算法（俯角 45 / 方位角 45）----
    let focus = Vec3::ZERO;
    let distance = view_width * 1.2;
    let pitch = 45.0_f32.to_radians();
    let azimuth = 45.0_f32.to_radians();
    let offset = Vec3::new(
        distance * azimuth.sin() * pitch.cos(),
        distance * pitch.sin(),
        distance * azimuth.cos() * pitch.cos(),
    );
    commands.spawn((
        Name::new("camera"),
        Camera3d::default(),
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: bevy::camera::ScalingMode::FixedHorizontal {
                viewport_width: view_width,
            },
            ..OrthographicProjection::default_3d()
        }),
        Transform::from_translation(focus + offset).looking_at(focus, Vec3::Y),
    ));

    // ---- 灯：与 prime 的 `spawn_terrain_light` 完全一致的字段 ----
    commands.spawn((
        Name::new("light"),
        DirectionalLight {
            illuminance: illum,
            shadow_maps_enabled: false,
            contact_shadows_enabled: false,
            ..default()
        },
        // **`UP_Y=1` ⇒ up 用 `Vec3::Y`**（正确）；否则复现原来的 `Vec3::Z`（退化）。
        //
        // 这就是怀疑的 bug：`looking_at` 让 `-Z` 指向 target，
        // 而 `up` 传 `Vec3::Z` 时与视线方向几乎共线 ⇒ **退化**，
        // 基向量被算歪 ⇒ 光的方向不是"从斜上方往下"，而是接近水平。
        Transform::from_translation(light_dir).looking_at(
            Vec3::ZERO,
            if std::env::var("UP_Y").is_ok_and(|v| v == "1") {
                Vec3::Y
            } else {
                Vec3::Z
            },
        ),
    ));
    commands.insert_resource(GlobalAmbientLight {
        color: Color::WHITE,
        brightness: ambient,
        affects_lightmapped_meshes: true,
    });

    // ---- 两个对照物：同一个 GLB，一个平放、一个立起 ----
    //
    // `TILT=1` ⇒ **两个都按 gallery 的姿态**（平放），去掉"法线朝向"这个变量。
    // FLIP=1 ⇒ 平放的那块**翻 180°**（让法线朝下的那一面朝上）。
    // 用来验证"渲染的是背面"这一条：如果翻转后变亮，就说明原来是背面朝上。
    // FLIP 选一个轴转 180°：x / y / z / 空（不转）。
    //
    // 用来确定"**怎么转才是对的**"：
    // - 绕 X 转 180° ⇒ 上下翻个身（Y 轴反号）—— 但那样物体位置也变了；
    // - 绕 Y 转 180° ⇒ 水平转半圈（不改变上下）；
    // - 绕 Z 转 180° ⇒ 左右翻（X 与 Y 都反号）。
    //
    // 实测：**不转是黑的，绕 X 转 180° 是亮的**。
    let flip = std::env::var("FLIP").unwrap_or_default();
    let spin = match flip.as_str() {
        "x" => Quat::from_rotation_x(std::f32::consts::PI),
        "y" => Quat::from_rotation_y(std::f32::consts::PI),
        "z" => Quat::from_rotation_z(std::f32::consts::PI),
        _ => Quat::IDENTITY,
    };
    let flat = Transform::from_xyz(-2.5 * tile_scale, 0.0, 0.0)
        .with_scale(Vec3::splat(tile_scale))
        .with_rotation(spin);
    let upright = Transform::from_xyz(2.5 * tile_scale, 0.0, 0.0)
        .with_scale(Vec3::splat(tile_scale))
        .with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2));

    let path = "models/dungeon/template-floor.glb";
    commands.spawn((
        Name::new("flat"),
        WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(path))),
        flat,
    ));
    commands.spawn((
        Name::new("upright"),
        WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(path))),
        if tilt {
            flat.with_translation(Vec3::new(2.5 * tile_scale, 0.0, 0.0))
        } else {
            upright
        },
    ));
}

/// **实验：把 GLB 材质改成 `unlit` + `cull_mode = None`。**
///
/// 动机（已确认的事实）：
/// - 这些 quad 有 **8 个顶点 = 两个重叠的三角形**（一组法线朝上、一组朝下）；
/// - GLB 是 `doubleSided: true` ⇒ Bevy 用 `cull_mode = None` ⇒ **两面都画**；
/// - 两者共用同一组位置 ⇒ **z-fighting**，朝下的那个赢 ⇒ 渲染成背光 ⇒ 近黑；
/// - 这解释了"调光完全无效"（N·L 恒为负）与"拿掉贴图就正常"（纯色时两面一样）。
///
/// `unlit` 绕开光照 ⇒ 不依赖哪一面赢 ⇒ 颜色直接等于贴图色。
/// `UNLIT=1` 打开这个实验。
fn make_unlit(
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut frame: Local<u32>,
    mut done: Local<bool>,
) {
    if *done || !std::env::var("UNLIT").is_ok_and(|v| v == "1") {
        return;
    }
    *frame += 1;
    if *frame < 60 {
        return;
    }
    *done = true;
    let ids: Vec<_> = materials
        .iter()
        .filter(|(_, m)| m.base_color_texture.is_some())
        .map(|(id, _)| id)
        .collect();
    let n = ids.len();
    for id in ids {
        if let Some(mut m) = materials.get_mut(id) {
            m.unlit = true;
        }
    }
    info!("[实验] 把 {n} 个带贴图的材质改成 unlit");
}

fn report(materials: Res<Assets<StandardMaterial>>, mut frame: Local<u32>, mut done: Local<bool>) {
    if *done {
        return;
    }
    *frame += 1;
    if *frame < 90 {
        return;
    }
    *done = true;
    info!("复现实验：材质 {} 个", materials.len());
    for (i, (id, m)) in materials.iter().enumerate() {
        info!(
            "  [{i}] {id:?} unlit={} metallic={:.2} rough={:.2} 贴图={}",
            m.unlit,
            m.metallic,
            m.perceptual_roughness,
            m.base_color_texture.is_some()
        );
    }
}

/// **把"模型实体"的世界变换与前几个网格实体的法线变换打出来。**
///
/// 目的：确认 glTF 导入后**世界变换里到底有没有额外的旋转/镜像**。
/// 光看公式推断已经错了两次，必须读实际值。
fn report_entities(
    roots: Query<(Entity, &Name, &GlobalTransform), With<WorldAssetRoot>>,
    meshes: Query<(&GlobalTransform, &Mesh3d, &Name)>,
    mut frame: Local<u32>,
    mut done: Local<bool>,
) {
    if *done {
        return;
    }
    *frame += 1;
    if *frame < 60 {
        return;
    }
    *done = true;
    for (entity, name, tf) in &roots {
        let (scale, rotation, translation) = tf.to_scale_rotation_translation();
        info!(
            "模型根 {entity} `{name}`：\n     平移={translation:?}\n     \
             旋转={rotation:?}\n     缩放={scale:?}",
        );
        // 旋转矩阵的行列式：**负值 = 镜像（手性翻转）**。
        let m = tf.affine().to_cols_array_2d();
        let det = tf.affine().matrix3.determinant();
        info!(
            "     矩阵=\n     {:?}\n     行列式={det:.4}（**负 = 镜像**）",
            m
        );
    }
    info!("网格实体 {} 个", meshes.iter().count());
    for (tf, _, name) in meshes.iter().take(4) {
        let (scale, rotation, _) = tf.to_scale_rotation_translation();
        info!("  网格 `{name}` 缩放={scale:?} 旋转={rotation:?}");
    }
}

fn quit_on_escape(keys: Res<ButtonInput<KeyCode>>, mut exit: MessageWriter<AppExit>) {
    if keys.just_pressed(KeyCode::Escape) {
        exit.write(AppExit::Success);
    }
}
