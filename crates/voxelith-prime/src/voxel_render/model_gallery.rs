//! **模型展示台**：把 Kenney 的 GLB 模型**一个一个方格摆出来**。
//!
//! ## 用途
//!
//! 这是一个**素材浏览器**，不是游戏内容。它回答："这些 `.glb` 各长什么样" ——
//! 靠读文件名猜不出来，而挑材质/挑模块化件时必须看。
//!
//! ## Kenney 的 GLB 怎么用
//!
//! Bevy 原生支持 glTF/GLB（`bevy_gltf`）。**不需要转码、不需要自己解析网格。**
//! Bevy **0.19** 的写法是：
//!
//! ```ignore
//! commands.spawn((
//!     WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(path))),
//!     Transform::from_translation(pos),
//! ));
//! ```
//!
//! - `GltfAssetLabel::Scene(0)` 的 `#Scene0` 标签**是必须的** ——
//!   它告诉 Bevy "取文件里的第 0 个场景"。不加的话 Bevy 不知道取哪一部分。
//! - 0.19 叫 **`WorldAssetRoot`**，不再是旧版的 `SceneRoot` / `SceneInstance`。
//!
//! ⚠️ **这些 GLB 的贴图是外链的**：文件里写 `uri: "Textures/colormap.png"`，
//! 所以磁盘上必须有那个文件（**相对 GLB 自己所在的目录**）。
//! 少了它模型会加载成功但**通体无纹理**（不报错，只是颜色不对）。
//! Kenney 的 3D 套件都是这个套路：一张 `colormap` 调色板，UV 直接指到色带上。
//!
//! ## 两套素材的区别
//!
//! | 套件 | 形态 | 适合干什么 |
//! |---|---|---|
//! | `platformer-kit` | 81 个方块，占地 1.08 ~ 2.08（圆角、斜坡、六边形） | 地形美术风格参考 |
//! | **`modular-cave-kit`** | 40 个**模块化网格件**：`template-floor` 是 **4x4**、`template-wall` 高 4.05、`room-small` 是 **12x12** | **拼接地图**（天生按网格对齐） |
//!
//! 两套都能"一个一个方格"规整展示；差别在**尺寸是否统一** ——
//! cave-kit 的 `template-*` 是同网格的模板件，拼起来天然对缝。
//!
//! ## 怎么用
//!
//! 默认**关闭**。打开：
//!
//! ```powershell
//! $env:MODEL_GALLERY = "1"          # 打开
//! $env:MODEL_GALLERY_KIT = "cave"   # 选套件（cave / platformer）
//! ```

use bevy::prelude::*;

/// 一套可供展示的素材。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelKit {
    /// 环境变量 `MODEL_GALLERY_KIT` 用的短名。
    pub key: &'static str,
    /// 模型目录（相对资产根）。
    pub folder: &'static str,
    /// 模型清单（每行一个名字，不含扩展名）。
    pub manifest: &'static str,
    /// 格子间距（世界单位）。
    ///
    /// ## 为什么每套自己定，而不是算出来
    ///
    /// 间距必须大于**最大那个模型**的占地，否则会互相插进去 ——
    /// 而"方块互相穿插"看起来像模型坏了，会被误判。
    ///
    /// 实测：platformer 的 `-large` 变体是 **2.08** 宽（取 2.5）；
    /// cave-kit 的 `room-small` 是 **12** 宽（取 16）。
    pub spacing: f32,
    /// 每行摆几个。
    pub columns: u32,
}

/// 默认展示哪一套。
pub const DEFAULT_KIT: &str = "dungeon";

/// 已知的套件。
///
/// 加一套 = 在这里加一条 + 把 `.glb`、`Textures/colormap.png`、清单放进 `assets/`。
pub const KITS: &[ModelKit] = &[
    ModelKit {
        key: "dungeon",
        folder: "models/dungeon",
        manifest: include_str!("../../../../assets/models/dungeon/models.txt"),
        // `room-*` 是 12 宽，`stairs` 最高 8.55；留余量。
        spacing: 16.0,
        columns: 7,
    },
    ModelKit {
        key: "cave",
        folder: "models/cave",
        manifest: include_str!("../../../../assets/models/cave/models.txt"),
        // `room-small` / `room-large` 是 12 宽，留余量。
        spacing: 16.0,
        columns: 7,
    },
    ModelKit {
        key: "platformer",
        folder: "models/platformer",
        manifest: include_str!("../../../../assets/models/platformer/blocks.txt"),
        // 最大是 `-large` 的 2.08。
        spacing: 2.5,
        columns: 9,
    },
];

impl ModelKit {
    /// 按短名找一套；**找不到就退回默认**（不 panic）。
    ///
    /// 理由：`MODEL_GALLERY_KIT` 是环境变量，打错字是常事 ——
    /// panic 会把"想看素材"变成"程序起不来"。
    pub fn by_key(key: &str) -> &'static Self {
        KITS.iter()
            .find(|kit| kit.key == key)
            .or_else(|| KITS.iter().find(|kit| kit.key == DEFAULT_KIT))
            .expect("DEFAULT_KIT 必须在 KITS 里")
    }

    /// 这套里的模型名。
    pub fn models(&self) -> impl Iterator<Item = &'static str> {
        self.manifest
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
    }

    /// 模型数量。
    pub fn count(&self) -> usize {
        self.models().count()
    }

    /// 行数（含最后那行不满的）。
    pub fn rows(&self) -> usize {
        self.count().div_ceil(self.columns.max(1) as usize)
    }
}

/// 展示台的配置（**Resource**）。
#[derive(Resource, Debug, Clone)]
pub struct ModelGalleryConfig {
    /// 是否启用。默认 `false` —— 游戏时不该看到这堆模型。
    pub enabled: bool,
    /// 展示哪一套。
    pub kit: &'static ModelKit,
    /// 展示台中心（避免和真实地形重叠）。
    pub origin: Vec3,
}

impl Default for ModelGalleryConfig {
    fn default() -> Self {
        // **环境变量开关**（默认关）：`MODEL_GALLERY=1` 打开，
        // `MODEL_GALLERY_KIT=cave|platformer` 选套件。
        //
        // 用环境变量而不是命令行参数：项目里已有先例，而且 `main.rs`
        // 只注册顶层插件、不解析参数。
        let enabled = std::env::var("MODEL_GALLERY").is_ok_and(|v| v == "1");
        let key = std::env::var("MODEL_GALLERY_KIT").unwrap_or_else(|_| DEFAULT_KIT.to_owned());
        Self {
            enabled,
            kit: ModelKit::by_key(&key),
            // 抬到地形上方（地形在 y = ±2 附近）。
            origin: Vec3::new(0.0, 40.0, 0.0),
        }
    }
}

/// 展示台已经摆好了（用来避免重复 spawn）。
#[derive(Resource, Debug, Default)]
pub struct ModelGallery {
    /// 已摆出的模型数量（也用于日志）。
    pub placed: usize,
}

/// 摆出当前套件的所有模型（只跑一次）。
pub fn spawn_model_gallery(
    mut commands: Commands,
    config: Res<ModelGalleryConfig>,
    assets: Option<Res<AssetServer>>,
    existing: Option<Res<ModelGallery>>,
) {
    if !config.enabled || existing.is_some() {
        return;
    }
    let Some(assets) = assets else {
        // 无头测试：没有资产设施，摆不出来。
        return;
    };

    let kit = config.kit;
    let columns = kit.columns.max(1);
    let rows = kit.rows().max(1) as f32;

    let mut placed = 0usize;
    for (index, name) in kit.models().enumerate() {
        let column = (index as u32 % columns) as f32;
        let row = (index as u32 / columns) as f32;
        // 以网格中心为原点 ⇒ `origin` 就是"展示台的中心"。
        let offset = Vec3::new(
            (column - (columns as f32 - 1.0) / 2.0) * kit.spacing,
            0.0,
            (row - (rows - 1.0) / 2.0) * kit.spacing,
        );

        // **补上模型自身的原点偏移。**
        //
        // 这些模型的局部原点约定**不统一**：地板/房间以底面中心为原点，
        // 而 `template-wall` 以**底面边缘**为原点（中心在 `z = -1.0`）、
        // `stairs` 在 `z = -2.0`。不补的话，按"一格一个"摆出来会**整体错位**，
        // 拼房间时墙会从格线上岔开。
        //
        // 偏移表是构建期从 GLB 字节里生成的（`tools/gltf_offsets.py`），
        // 与真实文件脱节时 `model_offset` 的测试会失败。
        let origin = super::model_offset::lookup(kit.key, name);

        let path = format!("{}/{name}.glb", kit.folder);
        // `#Scene0` 标签必须带（见模块文档）。
        commands.spawn((
            Name::new(format!("gallery/{name}")),
            WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(path))),
            // 原点偏移要**按网格间距缩放**（表里是模型自身单位）。
            Transform::from_translation(config.origin + offset + origin),
        ));
        placed += 1;
    }

    info!(
        "模型展示台：套件 `{}`，摆了 {placed} 个模型（{columns} 列，间距 {}）",
        kit.key, kit.spacing
    );
    commands.insert_resource(ModelGallery { placed });
}

/// 把相机对准展示台（**按网格跨度算取景**，不写死数字）。
///
/// 写死的后果：改 `columns` / `spacing` / 模型数量之后就得重新试参数 ——
/// 而"看不全展示台"很容易被误判成"模型没加载出来"
/// （第一版就是这样：81 个模型都在，但相机只框住了十几个）。
pub fn aim_camera_at_gallery(
    config: Res<ModelGalleryConfig>,
    gallery: Option<Res<ModelGallery>>,
    mut cameras: Query<(&mut Transform, &mut Projection), With<super::camera::BattlefieldCamera>>,
) {
    if !config.enabled || gallery.is_none() || !config.is_changed() {
        return;
    }
    let kit = config.kit;
    let span_x = kit.columns as f32 * kit.spacing;
    let span_z = kit.rows() as f32 * kit.spacing;
    // 等距（方位角 45°）下，横向网格与纵向网格在屏幕上各占约 1/sqrt2，
    // 所以屏幕宽度约 (span_x + span_z) / sqrt2；再留 12% 边距。
    let view_width = (span_x + span_z) / 2.0_f32.sqrt() * 1.12;

    let focus = config.origin;
    // 正交下距离不影响取景范围，只影响透视；取够大以免被近平面裁掉。
    let distance = view_width * 1.2;
    let pitch = 45.0_f32.to_radians();
    let azimuth = 45.0_f32.to_radians();
    let offset = Vec3::new(
        distance * azimuth.sin() * pitch.cos(),
        distance * pitch.sin(),
        distance * azimuth.cos() * pitch.cos(),
    );

    for (mut transform, mut projection) in &mut cameras {
        transform.translation = focus + offset;
        transform.look_at(focus, Vec3::Y);
        if let Projection::Orthographic(ortho) = &mut *projection {
            ortho.scaling_mode = bevy::camera::ScalingMode::FixedHorizontal {
                viewport_width: view_width,
            };
        }
    }
    let placed = gallery.map_or(0, |g| g.placed);
    info!(
        "模型展示台取景：{placed} 个模型 / {} 行 x {} 列，跨度 {span_x:.1} x {span_z:.1}，视野宽 {view_width:.1}",
        kit.rows(),
        kit.columns
    );
}

/// 注册模型展示台。
pub struct ModelGalleryPlugin;

impl Plugin for ModelGalleryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ModelGalleryConfig>()
            .add_systems(Startup, spawn_model_gallery)
            .add_systems(Update, (aim_camera_at_gallery, dump_materials_once));
    }
}

/// **把场景里每个 `StandardMaterial` 的实际字段值打印出来**（只打一次）。
///
/// ## 为什么需要它（这是从截图反推改为直接读数据的转折点）
///
/// 同一个 GLB 在**最小探针**（`probe/glb`）里正常，在 **prime** 里发黑。
/// 两边唯一的差别就在"材质是怎么被建出来的"，所以必须**读实际生效的字段值**，
/// 而不是继续看截图猜。
///
/// 关键看三样：
/// - **`unlit`**：为真就完全不走光照，直接用贴图色（地形材质就是 `unlit`）。
///   如果 GLB 的材质也被设成 `unlit`，那它不该受光照影响 ——
///   而实测"主光调亮 400 倍毫无变化"正是这个特征；
/// - **`base_color`**：不是白就整体乘了一个系数；
/// - **`base_color_texture`**：为空说明贴图根本没绑上。
///
/// ## 为什么遍历 `Assets<StandardMaterial>` 而不是查实体句柄
///
/// GLB 的材质挂在 `SceneRoot` 展开出来的**子实体**上，而场景是异步展开的 ——
/// 查实体句柄会漏（第一版就只抓到了地形材质）。
fn dump_materials_once(
    materials: Res<Assets<StandardMaterial>>,
    images: Option<Res<Assets<Image>>>,
    config: Res<ModelGalleryConfig>,
    mut frame: Local<u32>,
    mut done: Local<bool>,
) {
    if *done || !config.enabled {
        return;
    }
    *frame += 1;
    // 等几帧：GLB 是异步加载的，第 1 帧资产表里只有地形材质。
    if *frame < 60 {
        return;
    }
    *done = true;

    info!(
        "[展示台] 材质转储：共 {} 个 StandardMaterial",
        materials.len()
    );
    for (index, (id, material)) in materials.iter().enumerate() {
        info!(
            "[展示台]   [{index}] id={id:?} base_color={:?} **unlit={}** metallic={:.3} \
             roughness={:.3} 有基础色贴图={} cull={:?} double_sided={}",
            material.base_color,
            material.unlit,
            material.metallic,
            material.perceptual_roughness,
            material.base_color_texture.is_some(),
            material.cull_mode,
            material.double_sided,
        );
    }
    if let Some(images) = images {
        info!("[展示台] 贴图转储：共 {} 个 Image", images.len());
        for (index, (id, image)) in images.iter().enumerate() {
            let size = image.texture_descriptor.size;
            // 只打有意义的（跳过 1x1 与 LUT）。
            if size.width >= 32 && size.height >= 32 {
                info!(
                    "[展示台]   [{index}] id={id:?} {}x{} 格式={:?}",
                    size.width, size.height, image.texture_descriptor.format
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **每套的清单都要与磁盘上的 `.glb` 对得上**。
    ///
    /// ## 为什么这条重要
    ///
    /// 清单是硬编码的（`AssetServer` **不能列目录**），所以
    /// "清单与实际文件脱节"是**必然会发生**的失败模式：
    /// - 少一行 ⇒ 那个模型永远不显示；
    /// - 多一行 ⇒ 加载失败，但**只是那一格是空的**，不报错。
    ///
    /// 所以对着真实文件系统核对每一条。
    #[test]
    fn every_kit_manifest_matches_the_files_on_disk() {
        let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
        for kit in KITS {
            let listed: Vec<&str> = kit.models().collect();
            assert!(!listed.is_empty(), "套件 `{}` 的清单是空的", kit.key);

            // 名字不能重复（重复会让同一格摆两次、另一格空着）。
            let mut sorted = listed.clone();
            sorted.sort_unstable();
            let before = sorted.len();
            sorted.dedup();
            assert_eq!(before, sorted.len(), "套件 `{}` 的清单有重复名", kit.key);

            let dir = assets.join(kit.folder);
            for name in &listed {
                let path = dir.join(format!("{name}.glb"));
                assert!(
                    path.exists(),
                    "套件 `{}` 清单里的 `{name}` 在磁盘上找不到（{}）",
                    kit.key,
                    path.display()
                );
            }

            // 贴图也要在：GLB 的贴图是**外链**的（`uri: "Textures/colormap.png"`），
            // 少了它模型会加载成功但**通体无纹理**，而且不报错。
            let texture = dir.join("Textures/colormap.png");
            assert!(
                texture.exists(),
                "套件 `{}` 缺 `Textures/colormap.png` —— GLB 的贴图是外链的，\
                 少了它模型会加载成功但通体无纹理（不报错）",
                kit.key
            );
        }
    }

    /// **间距要放得下最大的模型**，否则方块会互相穿插。
    ///
    /// 实测占地：platformer 的 `-large` 是 **2.08**；cave-kit 的 `room-small` 是 **12**。
    #[test]
    fn each_kit_has_room_for_its_largest_model() {
        let known_largest = [
            ("platformer", 2.08_f32),
            ("cave", 12.0_f32),
            ("dungeon", 12.0_f32),
        ];
        for (key, largest) in known_largest {
            let kit = ModelKit::by_key(key);
            assert_eq!(kit.key, key, "`by_key` 该找到 `{key}`");
            assert!(
                kit.spacing >= largest,
                "套件 `{key}` 的间距 {} 小于最大模型占地 {largest}，方块会互相穿插",
                kit.spacing
            );
        }
    }

    /// 未知的套件名要**退回默认套件**，而不是 panic。
    #[test]
    fn an_unknown_kit_falls_back_to_the_default() {
        for bad in ["no-such-kit", "", "CAVE", " cave"] {
            let kit = ModelKit::by_key(bad);
            assert_eq!(kit.key, DEFAULT_KIT, "未知套件 `{bad}` 该退回默认套件");
        }
    }

    /// 默认**不启用**（它只是素材浏览器）。
    #[test]
    fn the_gallery_is_off_by_default() {
        // 注意：测试进程里可能设置了环境变量；这条只在没设时才断言。
        if std::env::var("MODEL_GALLERY").is_err() {
            assert!(
                !ModelGalleryConfig::default().enabled,
                "展示台默认该是关的 —— 游戏里不该看到这堆素材"
            );
        }
    }

    /// 行数算对（取景与摆放都靠它）。
    #[test]
    fn the_row_count_covers_the_last_partial_row() {
        for kit in KITS {
            assert_eq!(kit.rows(), kit.count().div_ceil(kit.columns as usize));
            assert!(
                kit.rows() * kit.columns as usize >= kit.count(),
                "套件 `{}`：行数 x 列数容不下所有模型",
                kit.key
            );
        }
    }
}
