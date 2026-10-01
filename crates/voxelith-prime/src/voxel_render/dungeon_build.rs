//! **地牢的建筑层**：读件名映射、按布局 spawn GLB 实体。
//!
//! 分层见 [`super::dungeon_layout`]：那边只算 `Vec<Placement>`（纯数据），
//! 这边负责"把表变成实体"。
//!
//! ## 怎么用
//!
//! ```powershell
//! $env:DUNGEON = "1"              # 打开（默认关）
//! $env:DUNGEON_KIT = "dungeon"    # dungeon / cave / platformer
//! $env:DUNGEON_SEED = "12345"     # 换种子换布局
//! ```
//!
//! ## 与 `model_gallery` 的关系
//!
//! | | `model_gallery` | 本模块 |
//! |---|---|---|
//! | 目的 | **看清每个件长什么样** | **看清它们拼起来对不对缝** |
//! | 摆法 | 一字排开 | 房间 + 走廊 + 门 |
//!
//! 两者可以同时开（展示台在地板另一侧），互不干扰。

use bevy::prelude::*;

use super::dungeon_layout::{self, DungeonSpec, Placement};

pub use super::dungeon_parts::DEFAULT_PART_KIT;
use super::dungeon_parts::{DungeonParts, loaded_parts};

/// 地牢配置（**Resource**）。
#[derive(Resource, Debug, Clone)]
pub struct DungeonConfig {
    /// 是否生成。
    pub enabled: bool,
    /// 用哪一套件名。
    pub kit: String,
    /// 布局参数。
    pub spec: DungeonSpec,
    /// 地牢的**格距**（世界单位）。
    ///
    /// ## 为什么与 `floor_grid` 的格距不同
    ///
    /// 两处的"格"含义不同：
    ///
    /// - `floor_grid` 的格 = **一个体素**（1 世界单位），因为是逐格铺地板；
    /// - 地牢的格 = **一个模块化件**。Kenney 的件是 4 世界单位宽、
    ///   `template-wall` 高 **4.15** —— 压到 1 格的话墙只有 **0.5 单位高**，
    ///   看起来像地上一圈细线，完全不像墙。
    ///
    /// 取 `2.0`：件缩放 `0.5`，墙高 ≈ **2.1 单位**，接近角色高度
    /// （`world.ron` 的 `world_size` 是 26，角色约 1 格宽）。
    pub cell_spacing: f32,

    /// 地牢中心的格坐标。
    ///
    /// ## 为什么要挪开
    ///
    /// 默认值把地牢放在地板**左前方**，而展示台在右前方 ——
    /// 于是两者不重叠，可以同时看。
    pub origin: IVec2,
}

impl Default for DungeonConfig {
    fn default() -> Self {
        Self {
            // 默认**关**：它是内容演示，不该盖住正常玩法。
            enabled: std::env::var("DUNGEON").is_ok_and(|v| v == "1"),
            kit: std::env::var("DUNGEON_KIT").unwrap_or_else(|_| DEFAULT_PART_KIT.to_owned()),
            spec: DungeonSpec {
                seed: std::env::var("DUNGEON_SEED")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(DungeonSpec::default().seed),
                ..DungeonSpec::default()
            },
            // 件按"一个模块 = 一格"来摆，见 `cell_spacing` 的说明。
            cell_spacing: 2.0,
            // 往 **-X** 挪一小段：地牢与展示台分列原点两侧，
            // 两者都能被同一台相机框进来（展示台在 `+24`，见 `GALLERY_X_OFFSET`）。
            //
            // 为什么不再挪远：件格距是 2.0，挪 30 格 = 世界 60 单位，
            // 地牢会整个跑到地板（±48）外面去 —— 实测就是这样。
            origin: IVec2::new(-12, 0),
        }
    }
}

/// 已生成（避免重复 spawn）。
#[derive(Resource, Debug, Default)]
pub struct DungeonBuilt {
    /// 摆了几个件。
    pub placed: usize,
}

/// 解析 `dungeon.ron`。
///
/// ## 为什么手写解析而不是派生 `Deserialize`
///
/// 格式是我们自己写的、**结构固定**（一个 `default_kit` + 若干 `kits`，
/// 每套五个字符串字段）。为它引 serde 派生 + 把 `content` 模块的类型体系
/// 扩一圈，代码量比这个小解析器大得多。
///
/// 但**校验一点没松**：[`tests`] 里对着**真实的模型清单**
/// （`assets/models/*/models.txt`）逐个核对每个名字都存在 ——
/// 因为"名字写错"**不会报任何错**，只是那一格空着。
/// 按布局把件 spawn 出来。
pub fn build_dungeon(
    mut commands: Commands,
    config: Res<DungeonConfig>,
    assets: Option<Res<AssetServer>>,
    existing: Option<Res<DungeonBuilt>>,
) {
    if !config.enabled || existing.is_some() {
        return;
    }
    let Some(assets) = assets else {
        return;
    };
    let parts = match loaded_parts() {
        Ok(parts) => parts,
        Err(err) => {
            // **启动期就喊出来**：配置写错必须立刻可见，不能静默空着。
            error!("地牢配置解析失败：{err}");
            return;
        }
    };
    let kit = parts.kit(&config.kit);

    let layout = dungeon_layout::generate(&config.spec);
    let mut placed = 0usize;
    for Placement {
        part,
        cell,
        quarter_turns,
    } in layout
    {
        let model = kit.model_for(part);
        let path = format!("models/{}/{model}.glb", config.kit);
        // 格中心：`cell` 是整数格，世界坐标要落在格**中心**。
        //
        // 为什么加 0.5：地板瓦片以自身中心为原点（`template-floor` 是
        // `-2..2`），放在整数格坐标上时四块瓦片会**在格点处相交**，
        // 拼起来偏半格。加 0.5 让每块瓦片正好占一格。
        let spacing = config.cell_spacing;
        let world = Vec3::new(
            (config.origin.x + cell.x) as f32 * spacing + spacing / 2.0,
            0.0,
            (config.origin.y + cell.y) as f32 * spacing + spacing / 2.0,
        );
        commands.spawn((
            Name::new(format!("dungeon/{model}@{},{}", cell.x, cell.y)),
            WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(path))),
            Transform::from_translation(world)
                .with_rotation(
                    Placement {
                        part,
                        cell,
                        quarter_turns,
                    }
                    .rotation(),
                )
                .with_scale(Vec3::splat(spacing / super::floor_grid::GLB_TILE_WIDTH)),
        ));
        placed += 1;
    }

    info!(
        "地牢：套件 `{}`，种子 {}，摆了 {placed} 个件（房间上限 {} 个）",
        config.kit, config.spec.seed, config.spec.rooms
    );
    commands.insert_resource(DungeonBuilt { placed });
}

/// 生成后核对一次：**实际存在的件实体数**要与记录一致。
///
/// 为什么要核对：GLB 是**异步**加载的，`spawn` 成功不代表模型出现；
/// 而几百个件里少几个是**看不出来**的（那只是几个空格子）。
/// 打一行实数，排查时能立刻区分"没摆出来"和"摆了但没渲染"。
pub fn verify_dungeon(
    built: Option<Res<DungeonBuilt>>,
    names: Query<&Name>,
    mut done: Local<bool>,
) {
    if *done {
        return;
    }
    let Some(built) = built else {
        return;
    };
    *done = true;
    let actual = names
        .iter()
        .filter(|name| name.as_str().starts_with("dungeon/"))
        .count();
    info!(
        "地牢核对：记录 {} 个件，实际查到 {actual} 个{}",
        built.placed,
        if actual == built.placed {
            "（一致）"
        } else {
            "（**不一致** —— 有实体没建出来）"
        }
    );
}

/// 把相机对准**地牢与地板的并集范围**。
///
/// ## 为什么要跟 `model_gallery` 一样"并集取景"
///
/// 地牢在 `-X` 侧、地板在中心、展示台在 `+X` 侧。只框地牢的话，
/// 画面里看不到一片完整的格子 —— 而**"墙有没有踩在格线上"
/// 正是要判断的东西**。
///
/// 算法与 `model_gallery::aim_camera_at_gallery` 一致（取 XZ 并集、
/// 按等距折算屏幕宽度），只是把展示台换成地牢。
pub fn aim_camera_at_dungeon(
    config: Res<DungeonConfig>,
    // 暂时不用（见函数内 let _ = floor 的说明），但保留参数：
    // 以后想让"地牢 + 地板"一起框时不用改调用点。
    floor: Option<Res<super::floor_grid::FloorGridConfig>>,
    built: Option<Res<DungeonBuilt>>,
    mut cameras: Query<(&mut Transform, &mut Projection), With<super::camera::BattlefieldCamera>>,
    mut logged: Local<bool>,
) {
    if !config.enabled || built.is_none() {
        return;
    }
    let (lo, hi) = config.spec.bounds;
    // 地牢的世界范围（格 → 世界，含中心偏移）。
    let spacing = config.cell_spacing;
    let dungeon_min = Vec2::new(
        (config.origin.x + lo.x) as f32 * spacing,
        (config.origin.y + lo.y) as f32 * spacing,
    );
    let dungeon_max = Vec2::new(
        (config.origin.x + hi.x) as f32 * spacing,
        (config.origin.y + hi.y) as f32 * spacing,
    );
    // **只框地牢本身 + 一圈余量**，不把整块地板并进来。
    //
    // ## 为什么（第一版并了地板，结果是错的）
    //
    // 第一版照搬 `model_gallery` 的"取 XZ 并集"。但地板是 **96 格**
    // （世界 ±48），而地牢只占 **±20** —— 并集之后地牢只占画面 1/5，
    // **完全看不清墙与墙之间有没有对上缝**，而那正是生成地牢的唯一目的。
    //
    // 现在只框地牢 + `margin` 圈余量：格子仍然充满画面（地板延伸到视野外），
    // 但地牢占满中央。
    let _ = floor;
    let margin = (hi.x - lo.x).max(hi.y - lo.y) as f32 * spacing * 0.25;

    let min_x = dungeon_min.x - margin;
    let max_x = dungeon_max.x + margin;
    let min_z = dungeon_min.y - margin;
    let max_z = dungeon_max.y + margin;

    let view_width = ((max_x - min_x) + (max_z - min_z)) / 2.0_f32.sqrt() * 1.15;
    let focus = Vec3::new((min_x + max_x) / 2.0, 0.0, (min_z + max_z) / 2.0);

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
    // **只打一次**：这个系统每帧都跑（相机可能被别的系统改动），
    // 而每帧一行日志会把终端刷爆 —— 实测刷了 100+ 行。
    if !*logged {
        *logged = true;
        info!(
            "地牢取景：跨度 {:.0} x {:.0}，视野宽 {view_width:.1}",
            max_x - min_x,
            max_z - min_z
        );
    }
}

/// 注册地牢。
pub struct DungeonPlugin;

impl Plugin for DungeonPlugin {
    fn build(&self, app: &mut App) {
        let config = DungeonConfig::default();
        if config.enabled {
            info!(
                "地牢：已开启（套件 `{}`，种子 {}）",
                config.kit, config.spec.seed
            );
        }
        app.insert_resource(config)
            .add_systems(Startup, build_dungeon)
            .add_systems(Update, (verify_dungeon, aim_camera_at_dungeon));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 解析好的配置（缓存过，只解析一次）。
    fn parts() -> &'static DungeonParts {
        loaded_parts().expect("配置文件该能解析")
    }

    /// **配置必须能解析**，且三个套件都在。
    #[test]
    fn the_config_parses_with_every_kit() {
        let parts = parts();
        assert!(!parts.default_kit.is_empty(), "default_kit 不该为空");
        let keys: Vec<&str> = parts.kits.iter().map(|k| k.kit.as_str()).collect();
        for expected in ["dungeon", "cave", "platformer"] {
            assert!(
                keys.contains(&expected),
                "缺少套件 `{expected}`（有 {keys:?}）"
            );
        }
    }

    /// **每个件名都必须在真实的模型清单里存在。**
    ///
    /// ## 为什么这条最重要
    ///
    /// 名字写错**不会报任何错** —— `AssetServer` 加载失败只会让**那一格空着**。
    /// 我第一版就把 `block-grass-slope` 写错了（真实名字是
    /// `block-grass-large-slope`），而画面看起来只是"少了一块"。
    ///
    /// 所以对着 `assets/models/<套件>/models.txt`（或 `blocks.txt`）逐个核对。
    #[test]
    fn every_configured_model_exists_in_its_manifest() {
        let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
        for kit in &parts().kits {
            // platformer 的清单叫 `blocks.txt`，模块化套件叫 `models.txt`。
            let dir = assets.join("models").join(&kit.kit);
            let manifest = ["models.txt", "blocks.txt"]
                .iter()
                .map(|name| dir.join(name))
                .find(|path| path.exists())
                .unwrap_or_else(|| panic!("套件 `{}` 找不到清单文件", kit.kit));
            let listed = std::fs::read_to_string(&manifest).expect("读清单");

            for (role, model) in [
                ("floor", &kit.floor),
                ("wall", &kit.wall),
                ("wall_corner", &kit.wall_corner),
                ("door", &kit.door),
                ("stairs", &kit.stairs),
            ] {
                assert!(
                    listed.lines().any(|line| line.trim() == model),
                    "套件 `{}` 的 `{role}` 写了 `{model}`，但清单 `{}` 里没有这个名字",
                    kit.kit,
                    manifest.display()
                );
                // 而且磁盘上真要有这个文件。
                assert!(
                    dir.join(format!("{model}.glb")).exists(),
                    "套件 `{}` 的 `{role}` = `{model}` 在磁盘上没有对应的 .glb",
                    kit.kit
                );
            }
        }
    }

    /// **地板块必须是 4x4 的那个**（`template-floor`），不能是 `-big`。
    ///
    /// `template-floor-big` 是 8x8 ⇒ 一格会盖住四格，整个布局会糊成一片。
    /// 这是"看起来只是有点怪、其实全错了"的那类错误，值得钉住。
    #[test]
    fn the_floor_piece_is_the_four_by_four_one() {
        let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
        for kit in parts().kits.iter().filter(|k| k.kit != "platformer") {
            let path = assets
                .join("models")
                .join(&kit.kit)
                .join(format!("{}.glb", kit.floor));
            let bytes = std::fs::read(&path).expect("读 GLB");
            let json_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
            let json: serde_json::Value =
                serde_json::from_slice(&bytes[20..20 + json_len]).expect("解析 glTF JSON");
            let accessor = &json["accessors"]
                [json["meshes"][0]["primitives"][0]["attributes"]["POSITION"]
                    .as_u64()
                    .unwrap() as usize];
            let width = accessor["max"][0].as_f64().unwrap() - accessor["min"][0].as_f64().unwrap();
            assert!(
                (width - 4.0).abs() < 0.01,
                "套件 `{}` 的地板块 `{}` 宽 {width}，该是 4（一个网格单位 2 x 2）",
                kit.kit,
                kit.floor
            );
        }
    }

    /// 未知套件名退回默认，不 panic。
    #[test]
    fn an_unknown_kit_falls_back_to_the_default() {
        let parts = parts();
        let fallback = parts.kit("no-such-kit");
        assert_eq!(fallback.kit, parts.default_kit, "未知套件该退回默认");
        assert_eq!(parts.kit("").kit, parts.default_kit);
    }

    /// 解析器对缺字段要**报错**，不能静默产出半成品。
    #[test]
    fn the_parser_rejects_incomplete_config() {
        use super::super::dungeon_parts::parse_parts;
        assert!(parse_parts("(default_kit: \"x\", kits: [])").is_err());
        assert!(
            parse_parts("(kits: [(key: \"a\")])").is_err(),
            "缺字段该报错"
        );
    }

    /// 默认**关闭**（它是内容演示，不该盖住正常玩法）。
    #[test]
    fn the_dungeon_is_off_by_default() {
        if std::env::var("DUNGEON").is_err() {
            assert!(!DungeonConfig::default().enabled, "地牢默认该是关的");
        }
    }

    /// 地牢与展示台**分别在原点两侧**，不该重叠。
    #[test]
    fn the_dungeon_and_the_gallery_do_not_collide() {
        let dungeon = DungeonConfig::default();
        let gallery_x = super::super::model_gallery::GALLERY_X_OFFSET;
        // 地牢往 -X 挪，展示台往 +X 挪。
        assert!(
            (dungeon.origin.x as f32) < 0.0,
            "地牢该在原点 -X 侧，实际 {}",
            dungeon.origin.x
        );
        assert!(gallery_x > 0.0, "展示台该在原点 +X 侧");
    }
}
