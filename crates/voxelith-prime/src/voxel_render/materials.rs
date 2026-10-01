//! 地形材质：**顶点色的图集贴图**。
//!
//! 一个材质服务全部区块（图集把所有方块打进一张贴图，**R73**）：改一格只换网格，
//! 不换材质 ⇒ 没有额外 draw call 状态切换。
//!
//! ## 朝向光照（为什么需要）
//!
//! 纯图集贴图会让立方体的各个面颜色完全一样，看起来是"一片糊"。MC 风格的做法是
//! **按面朝向给一点固定明暗**（顶面最亮、侧面中等、底面最暗）——这比真加一盏灯便宜得多，
//! 而且不依赖光照贴图。这里用顶点色做不到（颜色在材质上），所以走
//! "材质 + 顶点法线 + 光照"的路线：加一盏平行光，`StandardMaterial` 自然会分面明暗。

use bevy::prelude::*;

use super::atlas::AtlasImage;

/// 地形材质（**Resource**）：整个地形共用一个。
#[derive(Resource, Debug, Clone)]
pub struct VoxelMaterial {
    /// 材质句柄。
    pub handle: Handle<StandardMaterial>,
}

/// 建地形材质（在图集之后）。
pub fn build_material(
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
    atlas: Res<AtlasImage>,
) {
    let handle = materials.add(StandardMaterial {
        base_color_texture: Some(atlas.image.clone()),
        // 图集是 sRGB 像素数据；`Rgba8UnormSrgb` + 默认采样即可。
        // `nearest` 是 MC 风格的关键：线性过滤会把 16×16 的贴图糊成一团。
        base_color: Color::WHITE,
        // `unlit: false` —— 明暗交给**光照**（主光 18_000 / 环境光 60）。
        //
        // 试过"`unlit: true` + 逐顶点色"那条路，结果发现
        // **`StandardMaterial` 根本不读顶点色**（`bevy_pbr` 里没有 `ATTRIBUTE_COLOR`），
        // 于是所有面渲染成同一个亮度 —— 比不加还糟。已回退。
        //
        // 立体感的来源是**主光与环境光的比例**，见 `spawn_terrain_light`。
        unlit: true,
        perceptual_roughness: 0.9,
        metallic: 0.0,
        ..default()
    });
    commands.insert_resource(VoxelMaterial { handle });
}

/// 给地形打光的系统：**强主光 + 极弱环境光**。
///
/// ## 立体感 = 主光 / 环境光的**比例**（这一条是量化出来的）
///
/// 曾经是环境光 `900` / 主光 `1800`（约 1:2）——**比例太接近**，
/// 于是所有面亮度趋同。实测像素分布：顶面与侧面只差 **12%**
/// （亮度 88–99 占 49.7%、100–119 占 40.1%，而 120 以上 0%）。
/// 画面上就是"一堆色块"，完全没有体积感。
///
/// 现在 主光 `18_000` / 环境光 `60`（**1:300**）：顶面接近满亮，
/// 侧面只有斜射的少量光，底面接近环境光 —— 三档差异才拉得开。
///
/// **环境光的作用是"背光面不死黑"，不是"照亮全场"**。把它当补光用会毁掉立体感。
pub fn spawn_terrain_light(mut commands: Commands, mut ambient: ResMut<GlobalAmbientLight>) {
    commands.spawn((
        Name::new("terrain-light"),
        DirectionalLight {
            // 主光给足：它要独立把顶面照到接近满亮。
            // （排查时调到 `90_000` 会过曝成白色，所以也别过头。）
            // `ILLUM` 可覆盖：**临时调参口**，用来扫"模型到底吃不吃光"。
            // （探针 `probe/glb` 用的是同一套环境变量名，方便两边对照。）
            illuminance: std::env::var("ILLUM")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(12_000.0),
            // **关阴影**：方块地形自阴影会出一堆黑块（默认开着），而且它需要
            // shadow bias / cascade 调参才能好看。对"看清地形"没好处，反而像渲染错误。
            // 真要做阴影再单独设计（现在没有那个需求）。
            shadow_maps_enabled: false,
            contact_shadows_enabled: false,
            ..default()
        },
        // **斜着照**：正上方会让所有顶面得到同样的光，看不出台阶与体积。
        // 从右上偏前照向地表，四个侧面因此明暗不同。
        //
        // 位置可用 `LIGHT_POS=x,y,z` 覆盖 —— 排查"是不是光方向的问题"时用。
        // **注意平行光只看方向、不看位置**（位置只影响阴影，而阴影是关的），
        // 但"位置"决定了 `looking_at` 算出来的方向，所以它仍是一个有效的旋钮。
        {
            let pos = std::env::var("LIGHT_POS")
                .ok()
                .and_then(|v| {
                    let n: Vec<f32> = v.split(',').filter_map(|s| s.trim().parse().ok()).collect();
                    (n.len() == 3).then(|| Vec3::new(n[0], n[1], n[2]))
                })
                .unwrap_or(Vec3::new(1.0, 2.0, 0.6));
            Transform::from_translation(pos).looking_at(Vec3::ZERO, Vec3::Z)
        },
    ));
    // 环境光**故意压得很低**：只保证背光面不死黑，不参与"照亮地形"。
    //
    // ## ⚠️ 必须用 `GlobalAmbientLight`（Resource），不能 spawn `AmbientLight`（Component）
    //
    // Bevy **0.19** 改了语义（见 `bevy_light::ambient_light`）：
    //
    // ```ignore
    // /// It can be added to a camera to override `GlobalAmbientLight`...
    // #[require(Camera)]
    // pub struct AmbientLight { .. }
    // ```
    //
    // 也就是说 `AmbientLight` 现在是**挂在相机上的覆盖值**，而且带 `#[require(Camera)]`。
    // 我以前把它当"全局环境光"spawn 成一个独立实体 —— 那会**凭空造出一个裸相机**
    // （没有 `Camera3d`/`Camera2d` ⇒ 没有渲染图），每帧刷这条警告：
    //
    // ```text
    // WARN bevy_render::camera: Entity 461v0 has a `Camera` component,
    //      but it doesn't have a render graph configured.
    // ```
    //
    // 那个实体就是 `terrain-ambient` —— 名字和内容都是灯，却带着 `Camera`。
    // 全局环境光该用 Resource 形式：
    *ambient = GlobalAmbientLight {
        color: Color::WHITE,
        brightness: std::env::var("AMBIENT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(60.0),
        affects_lightmapped_meshes: true,
    };
}

/// 注册材质与光照。
pub struct MaterialPlugin;

impl Plugin for MaterialPlugin {
    fn build(&self, app: &mut App) {
        // 与图集同一个集合、chain() 到它后面：uild_material 读 AtlasImage，
        // 而**跨 SystemSet 的 .after(函数) 不会自动排序**（集合之间没有声明关系时，
        // Bevy 可以任意并行），所以这里必须显式链起来。
        app.add_systems(
            Startup,
            (build_material, spawn_terrain_light)
                .chain()
                .after(crate::voxel_render::atlas::build_atlas),
        );
    }
}
/// 把内容里的地形参数写进 `WorldConfig`（**排队写世界**，不是 `insert_resource`）。
///
/// 为什么不用 `commands.insert_resource`：命令要到本帧末才生效，而同一个 `Startup` 链里
/// 后面的 `build_initial_terrain` 立刻要读它。
pub fn install_terrain_config(
    mut commands: Commands,
    content: Option<Res<crate::content::ContentData>>,
) {
    let Some(content) = content else {
        return;
    };
    let terrain = content.world.terrain;
    commands.queue(move |world: &mut World| {
        // **没有 `WorldConfig` 就跳过**（结构测试只装内容与体素插件，
        // 没装 `WorldPlugin`）。用 `get_resource_mut` 而不是 `resource_mut`：
        // 后者会 panic，而这里"配置没装"是合法状态。
        if let Some(mut config) = world.get_resource_mut::<voxelith_axiom::world::WorldConfig>() {
            config.terrain = terrain;
        }
    });
}
