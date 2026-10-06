//! **L2 战斗内容装配**：读文件 → 建目录 → 注册模板 → 生成可玩角色 → 接输入。
//!
//! 这是新内容管线在**真实应用**里的落点。分层职责：
//!
//! ```text
//! voxelith-abilities   解析字符串 + 校验 + 构造场景（纯逻辑，不碰文件系统）
//! voxelith-prime       **读文件**（本模块）+ 生成实体 + 接输入 + 显示反馈
//! ```
//!
//! 为什么读文件放在 L2：`abilities` 只吃 `&str`，所以它能在测试里对着
//! `include_str!` 跑，也可以给一个"纯校验 CLI"用；而"文件从哪来"（资产根、热重载、
//! 打包后的路径）是表现层的知识。
//!
//! ## 三份内容有依赖方向，加载顺序不能乱
//!
//! ```text
//! attributes.ron ──► 属性名台账 ──┐
//!                                 ├──► abilities.ron（技能引用属性名与状态 id）
//! status_defs.ron ─► 状态台账 ────┘
//! ```
//!
//! 顺序错了的症状是"属性名/状态 id 明明是写对的，却报未登记"。
//!
//! ## 输入 → 意图 → 请求（三层，别合并）
//!
//! ```text
//! leafwing 动作状态 ──intent_from_input──► CastIntent{slot}
//!                                            └─intent_to_request─► CastRequest{caster, ability}
//!                                                                    └─ abilities::casting（门控 + 扣费）
//! ```
//!
//! 中间那层 `CastIntent` 是刻意的：AI / UI / 脚本都发它，**不必知道技能实体是谁**；
//! 而"第几个技能"到"哪个实体"的翻译是 L2 的知识（技能栏从哪来）。

use std::path::PathBuf;

use bevy::ecs::schedule::ApplyDeferred;
use bevy::prelude::*;
use leafwing_input_manager::prelude::*;
use moonshine_save::prelude::*;

use crate::combat_save::SavedActor;
use voxelith_abilities::actors::ActorCatalog;
use voxelith_abilities::attributes::{self, AttributeCatalog, AttributeSetRon};
use voxelith_abilities::builder::{build_ability, install_statuses};
use voxelith_abilities::casting::CastRequest;
use voxelith_abilities::numeric::RegenRules;
use voxelith_abilities::skills::{self, AbilityRon};
use voxelith_abilities::statuses::{self, StatusCatalog};
use voxelith_abilities::{
    AttributeInitializer, Attributes, InvokedBy, InvokerTarget, TemplateRegistry,
};
use voxelith_axiom::atoms::grid::CellPos;

use crate::ecosystem::GameAction;

/// 可被玩家/玩家式 AI 操作的角色（表现层标记，不碰旧角色模型）。
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct Playable;

/// 技能栏：**按内容顺序**排好的技能实体。
///
/// "第 N 个技能"到实体的翻译在这里；将来接入 `diesel` 的技能池
/// （`AvailableAbilities`）时，它会被替换成"当前可用技能列表"。
#[derive(Component, Debug, Clone, Default)]
pub struct Loadout(pub Vec<Entity>);

/// 想放技能（**Message**）：输入 / AI / UI 都发它。
///
/// 它**只说要第几个**，不说是哪个实体——那是技能栏的事。
#[derive(Message, Clone, Copy, Debug)]
pub struct CastIntent {
    /// 谁想放。
    pub caster: Entity,
    /// 第几个技能（0 起）。
    pub slot: usize,
}

/// 内容文件的目录（默认 = 工作区根的 `assets/data`）。
#[derive(Resource, Debug, Clone)]
pub struct ContentPaths {
    /// 目录。
    pub dir: PathBuf,
}

impl Default for ContentPaths {
    fn default() -> Self {
        Self {
            // 与 `main.rs` 里 `AssetPlugin` 用的是同一个套路：编译期定死，
            // 运行期不用管当前目录在哪。
            dir: PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/data")),
        }
    }
}

/// 加载好的战斗内容（**Resource**）。
#[derive(Resource, Debug, Clone, Default)]
pub struct CombatContent {
    /// 属性名台账（技能与状态的交叉校验都用它）。
    pub attributes: AttributeCatalog,
    /// 状态台账。
    pub statuses: StatusCatalog,
    /// 技能定义（顺序 = 技能栏顺序）。
    pub abilities: Vec<AbilityRon>,
    /// 全局属性定义（派生上限 + 每秒回复）：生成角色时要和角色自己的数拼起来。
    pub attributes_source: AttributeSetRon,
    /// 角色台账（L2 按 id 取"玩家 / 哥布林 / 巨魔"）。
    pub actors: ActorCatalog,
}

/// 四份内容文件名（依赖方向：属性 → 角色 → 状态 → 技能）。
const ATTRIBUTES_FILE: &str = "attributes.ron";
/// 角色定义文件名。
const ACTORS_FILE: &str = "actors.ron";
/// 状态定义文件名。
const STATUS_DEFS_FILE: &str = "status_defs.ron";
/// 技能定义文件名。
const ABILITIES_FILE: &str = "abilities.ron";

/// 内容从哪来。
///
/// | 取值 | 谁用 | 行为 |
/// |---|---|---|
/// | `Disk`（默认） | 测试 / 无资产管线 | `std::fs` 直读，同步、确定 |
/// | `Assets` | 真应用（`EcosystemPlugin` 插的） | 等 `CombatAssets` 集合加载完再读 |
///
/// 为什么两种都要：资产管线**要几帧**（异步加载），而测试希望"update 一次内容就绪"。
/// 与其让测试去驱动状态机，不如把"来源"做成一个显式开关。
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ContentSource {
    /// 直接从磁盘读。
    #[default]
    Disk,
    /// 走 `bevy_asset_loader` 的集合（`combat_assets::CombatAssets`）。
    Assets,
}

/// 启动时从磁盘读四份内容、建目录、注册状态模板（`ContentSource::Disk` 走这条）。
pub fn load_combat_content(
    source: Res<ContentSource>,
    paths: Res<ContentPaths>,
    mut templates: ResMut<TemplateRegistry>,
    mut commands: Commands,
) {
    if *source != ContentSource::Disk {
        return;
    }

    let read = |file: &str| -> Option<String> {
        let path = paths.dir.join(file);
        match std::fs::read_to_string(&path) {
            Ok(text) => Some(text),
            Err(error) => {
                error!("[combat] 读不到内容文件 {}：{error}", path.display());
                None
            }
        }
    };

    let Some(attributes) = read(ATTRIBUTES_FILE) else {
        commands.insert_resource(CombatContent::default());
        return;
    };
    let Some(actors) = read(ACTORS_FILE) else {
        commands.insert_resource(CombatContent::default());
        return;
    };
    let Some(statuses) = read(STATUS_DEFS_FILE) else {
        commands.insert_resource(CombatContent::default());
        return;
    };
    let Some(abilities) = read(ABILITIES_FILE) else {
        commands.insert_resource(CombatContent::default());
        return;
    };

    install_content(
        crate::combat_assets::ContentTexts {
            attributes,
            actors,
            statuses,
            abilities,
        },
        &mut templates,
        &mut commands,
    );
}

/// **资产路径**：集合加载完之后读一次（`ContentSource::Assets` 走这条）。
///
/// 跑在 `Update` 而不是 `Startup`：资产是**异步**加载的，集合出现的那一帧才读得到。
pub fn load_combat_content_from_assets(
    source: Res<ContentSource>,
    loaded: Option<Res<crate::combat_assets::CombatAssets>>,
    sources: Option<Res<Assets<crate::combat_assets::RonSource>>>,
    existing: Option<Res<CombatContent>>,
    mut templates: ResMut<TemplateRegistry>,
    mut commands: Commands,
) {
    // ⚠️ 守卫不能写 `existing.is_some()`：`CombatContent` 一开始就被 `init_resource`
    // 成了**空**资源，那个条件永远为真 ⇒ 资产这条路永远不装（曾经如此）。
    // 判据是"内容还没装上"，不是"资源不存在"。
    if *source != ContentSource::Assets {
        return;
    }
    if existing.is_some_and(|content| !content.abilities.is_empty()) {
        return;
    }
    let Some(loaded) = loaded else {
        return;
    };
    // ⚠️ 参数可选：纯磁盘路径的 App 里根本没有注册这个资产类型（测试就是那样），
    // 少一个 `Option` 会让调度报 "Resource does not exist"。
    let Some(sources) = sources else {
        return;
    };
    let Some(texts) = loaded.texts(&sources) else {
        return;
    };
    info!("[combat] 内容从资产管线来（`bevy_asset_loader` 的集合已就绪）");
    install_content(texts, &mut templates, &mut commands);
}

/// 四份原文 → 台账 / 目录 / 模板（**两条来源共用这一段**）。
fn install_content(
    texts: crate::combat_assets::ContentTexts,
    templates: &mut TemplateRegistry,
    commands: &mut Commands,
) {
    // 这个函数只写目录与模板，不碰世界（`commands` 用来插资源）。
    let mut catalogue = CombatContent::default();
    let attributes_text = texts.attributes;
    let actor_text = texts.actors;
    let status_text = texts.statuses;
    let ability_text = texts.abilities;

    // ① 属性 → ② 角色 → ③ 状态 → ④ 技能：依赖方向，顺序不能乱。
    match attributes::parse(&attributes_text) {
        Ok(attribute_set) => {
            catalogue.attributes.add(&attribute_set);
            // 数值层：`attributes.ron` 里的 `regen` 规则要装进资源，
            // 否则资源只会被扣、不会自然回复（`numeric::regenerate` 读的就是它）。
            commands.insert_resource(RegenRules::from_set(&attribute_set));
            catalogue.attributes_source = attribute_set;
        }
        Err(error) => {
            error!("[combat] {ATTRIBUTES_FILE} 语法错误：{error}");
            commands.insert_resource(catalogue);
            return;
        }
    }

    // ② 角色：**它的数在 `actors.ron` 里**（`attributes.ron` 只有全局定义与词汇）。
    // 加载期会拦住"角色写了全局已定义的名字"——那种写法在 gauge 里是累加而不是覆盖。
    {
        match ActorCatalog::load(
            &actor_text,
            &catalogue.attributes,
            &catalogue.attributes_source,
        ) {
            Ok(actors) => {
                info!(
                    "[combat] 角色：{:?}",
                    actors.iter().map(|a| &a.id).collect::<Vec<_>>()
                );
                catalogue.actors = actors;
            }
            Err(error) => error!("[combat] {ACTORS_FILE} 加载失败：{error}"),
        }
    }

    {
        match statuses::load(&status_text, &catalogue.attributes) {
            Ok(catalog) => {
                // 技能按名字（`status:{id}`）引用状态模板，所以先登记模板。
                install_statuses(templates, &catalog);
                catalogue.statuses = catalog;
            }
            Err(error) => error!("[combat] {STATUS_DEFS_FILE} 加载失败：{error}"),
        }
    }

    {
        match skills::load(&ability_text, &catalogue.attributes, &catalogue.statuses) {
            Ok(abilities) => catalogue.abilities = abilities,
            Err(error) => error!("[combat] {ABILITIES_FILE} 加载失败：{error}"),
        }
    }

    info!(
        "[combat] 内容就绪：{} 个属性名 / {} 个状态 / {} 个技能",
        catalogue.attributes.names().len(),
        catalogue.statuses.len(),
        catalogue.abilities.len()
    );
    commands.insert_resource(catalogue);
}

/// 按内容生成一场演示对局：一个可玩角色（`actors.ron` 的 `player`）+ 一个目标（`goblin`）。
///
/// 内容为空时什么都不做（加载失败已经在 `load_combat_content` 里报过了）。
///
/// **两个角色的数不一样了**：这是这一层的意义——以前所有实体共用一份属性，
/// 现在各自的数来自各自的定义。
pub fn spawn_demo_duel(
    mut commands: Commands,
    content: Res<CombatContent>,
    existing: Query<(), With<Playable>>,
) {
    if content.abilities.is_empty() || content.actors.is_empty() {
        return;
    }
    // **幂等**：它既在 `Startup`（磁盘路径）也在 `Update`（资产路径，等集合就绪）
    // 里注册着，所以必须自己判"已经生成过了"。少了这条守卫会每帧生成一对新的角色 ✗。
    if !existing.is_empty() {
        return;
    }
    let globals = &content.attributes_source;
    // 目标用哥布林（旧 `monsters.ron` 的数字），玩家用 `player`。
    let Some(defender_set) = content.actors.set_for(globals, "goblin") else {
        error!("[combat] 角色文件里没有 `goblin`");
        return;
    };
    let Some(caster_set) = content.actors.set_for(globals, "player") else {
        error!("[combat] 角色文件里没有 `player`");
        return;
    };

    let defender = commands
        .spawn((
            CellPos::new(1, 0),
            Attributes::new(),
            AttributeInitializer::new(defender_set),
            // 存档标记：这场对局里的两个角色都进档（`save` 是 moonshine 的字段式标记）。
            SavedActor {
                template: "goblin".to_string(),
            },
            Save,
            Name::new("哥布林"),
        ))
        .id();

    let caster = commands
        .spawn((
            CellPos::ZERO,
            Attributes::new(),
            AttributeInitializer::new(caster_set),
            InvokerTarget::entity(defender, IVec2::new(1, 0)),
            Playable,
            SavedActor {
                template: "player".to_string(),
            },
            Save,
            Name::new("玩家"),
        ))
        .id();

    // 技能栏 = 内容顺序；每个技能是一个独立场景，认住施法者。
    let mut slots = Vec::new();
    for ability in &content.abilities {
        let entity = commands
            .spawn_scene(build_ability(ability, &content.statuses))
            .id();
        commands.entity(entity).insert(InvokedBy(caster));
        slots.push(entity);
    }
    commands.entity(caster).insert(Loadout(slots));
}

/// 输入 → 意图：leafwing 的动作状态 → [`CastIntent`]。
///
/// 三个技能位各对应技能栏的第 0/1/2 个（内容顺序）。
pub fn intent_from_input(
    players: Query<(Entity, &ActionState<GameAction>), With<Playable>>,
    mut intents: MessageWriter<CastIntent>,
) {
    for (caster, actions) in &players {
        for (slot, action) in [
            GameAction::Ability1,
            GameAction::Ability2,
            GameAction::Ability3,
        ]
        .into_iter()
        .enumerate()
        {
            if actions.just_pressed(&action) {
                intents.write(CastIntent { caster, slot });
            }
        }
    }
}

/// 意图 → 请求：查技能栏 → [`CastRequest`]。
///
/// **不在这里做门控与扣费**：那是 `abilities::casting` 的活（它才是唯一入口）。
pub fn intent_to_request(
    loadouts: Query<&Loadout>,
    mut intents: MessageReader<CastIntent>,
    mut requests: MessageWriter<CastRequest>,
) {
    for intent in intents.read() {
        let Ok(loadout) = loadouts.get(intent.caster) else {
            continue;
        };
        let Some(&ability) = loadout.0.get(intent.slot) else {
            // 槽位越界：UI 里那个按钮该是灰的，不该发意图。
            continue;
        };
        requests.write(CastRequest {
            caster: intent.caster,
            ability,
        });
    }
}

/// 装配战斗内容：加载 → 生成 → 输入。
pub struct CombatContentPlugin;

impl Plugin for CombatContentPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ContentPaths>()
            // 内容来源默认是磁盘（测试友好）；真应用里 `CombatAssetsPlugin` 会把它改成资产管线。
            .init_resource::<ContentSource>()
            .init_resource::<CombatContent>()
            .add_message::<CastIntent>()
            .add_systems(
                Startup,
                (load_combat_content, ApplyDeferred, spawn_demo_duel).chain(),
            )
            .add_systems(
                Update,
                (
                    // 资产管线的那条路：集合就绪后读一次（磁盘那条在 `Startup` 已经读完）。
                    load_combat_content_from_assets,
                    ApplyDeferred,
                    spawn_demo_duel,
                    intent_from_input,
                    intent_to_request,
                )
                    .chain(),
            );
    }
}
