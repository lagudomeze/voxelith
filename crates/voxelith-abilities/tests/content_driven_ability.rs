//! **内容驱动闭环**：`abilities.ron` + `status_defs.ron` → 构造器 → 可玩技能 → 数值 / 费用 / 门控 / 状态。
//!
//! 这里**一个场景字段都没手写**——技能与状态来自 `assets/data/*.ron`，
//! 构造器只负责把它们搭成场景。释放走**请求门控**（`CastRequest`），不直接写 `StartInvoke`。
//!
//! ```text
//! 文件语法与语义   attributes → statuses → skills（依赖方向，逐个建台账）
//! 场景搭得出来     builder（技能含费用/需求/状态生成；状态是一台独立状态机）
//! 放得出来         casting（需求 + 付得起 → 扣费 → 进状态机）
//! 打出来数字对     contest + gauge
//! 状态挂得上/卸得下 diesel 的 sustained modifier（失去 Active 时精确回收）
//! ```

use bevy::asset::AssetPlugin;
use bevy::prelude::*;
use bevy_diesel::prelude::*;
use bevy_gauge::requirements::AttributeRequirements;

use voxelith_abilities::attributes::{self, AttributeCatalog};
use voxelith_abilities::builder::{build_ability, install_statuses};
use voxelith_abilities::casting::{CastCosts, CastRejected, CastRequest};
use voxelith_abilities::contest::CombatLogLine;
use voxelith_abilities::skills::{self, AbilityRon};
use voxelith_abilities::statuses::{self, StatusCatalog};
use voxelith_axiom::atoms::grid::CellPos;

/// 真内容文件（路径编译期检查）。
const ATTRIBUTES: &str = include_str!("../../../assets/data/attributes.ron");
const ACTORS: &str = include_str!("../../../assets/data/actors.ron");
const STATUS_DEFS: &str = include_str!("../../../assets/data/status_defs.ron");
const ABILITIES: &str = include_str!("../../../assets/data/abilities.ron");

/// 行动力为 0 的施法者：用来验证门控真的会拦人。
const BROKE_ATTRIBUTES: &str = r#"(
    base: [
        (name: "Strength", literal: 10.0),
        (name: "Armor",    literal: 3.0),
        (name: "Health",   literal: 100.0),
        (name: "Action",   literal: 0.0),
    ],
)"#;

#[derive(Resource, Default)]
struct Log(Vec<String>);

fn collect_log(mut reader: MessageReader<CombatLogLine>, mut log: ResMut<Log>) {
    for line in reader.read() {
        log.0.push(line.0.clone());
    }
}

#[derive(Resource, Default)]
struct Rejections(Vec<&'static str>);

fn collect_rejections(mut reader: MessageReader<CastRejected>, mut log: ResMut<Rejections>) {
    for rejection in reader.read() {
        log.0.push(rejection.reason);
    }
}

fn test_app() -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        bevy::scene::ScenePlugin,
        voxelith_abilities::plugin(),
    ));
    app.init_resource::<Log>()
        .init_resource::<Rejections>()
        .add_systems(Update, (collect_log, collect_rejections));
    app
}

/// 解析并校验真内容，并把状态模板注册进 `TemplateRegistry`。
///
/// 顺序很重要：**先起 App**（gauge 的全局 interner 在插件里初始化），
/// 再按依赖方向建台账：属性 → 状态 → 技能（技能引用前两者的名字）。
fn load_content(app: &mut App) -> (Vec<AbilityRon>, StatusCatalog) {
    let mut catalog = AttributeCatalog::default();
    catalog.add(&attributes::parse(ATTRIBUTES).expect("属性文件语法没问题"));

    let statuses = statuses::load(STATUS_DEFS, &catalog).expect("状态文件该能加载");
    let abilities = skills::load(ABILITIES, &catalog, &statuses).expect("技能文件该能加载");

    // 技能按名字（`status:{id}`）引用状态模板，所以要先把它们登记进注册表。
    install_statuses(
        &mut app.world_mut().resource_mut::<TemplateRegistry>(),
        &statuses,
    );

    (abilities, statuses)
}

/// 造一场对局；属性来自给的那份 RON（默认是真内容那份）。
///
/// ⚠️ 真内容那份 `attributes.ron` **只有全局定义**（派生的上限与每秒回复），
/// 主属性在 `actors.ron` 里；而本文件里几份内联 RON 是**自足**的（自己写全了数值）。
/// 所以这里先试"全局 + 角色"，试不通（内联那份不认识角色文件的词汇）就用它自己。
fn spawn_duel(app: &mut App, attributes_source: &str) -> (Entity, Entity) {
    let globals = attributes::parse(attributes_source).expect("属性文件");
    let mut set = globals.build_base().expect("属性集能构造");
    if let Ok((_, _, actors)) =
        voxelith_abilities::actors::load_from_sources(attributes_source, ACTORS)
        && let Some(player) = actors.set_for(&globals, "player")
    {
        set = player;
    }

    let defender = app
        .world_mut()
        .spawn((
            CellPos::new(1, 0),
            Attributes::new(),
            AttributeInitializer::new(set.clone()),
        ))
        .id();
    let caster = app
        .world_mut()
        .spawn((
            CellPos::ZERO,
            Attributes::new(),
            AttributeInitializer::new(set),
            InvokerTarget::entity(defender, IVec2::new(1, 0)),
        ))
        .id();
    (caster, defender)
}

/// 完整准备：内容 + 对局，返回可用的 App 与技能表。
struct Fixture {
    app: App,
    content: Vec<AbilityRon>,
    statuses: StatusCatalog,
    caster: Entity,
    defender: Entity,
}

impl Fixture {
    fn new(attributes_source: &str) -> Self {
        let mut app = test_app();
        let (content, statuses) = load_content(&mut app);
        let (caster, defender) = spawn_duel(&mut app, attributes_source);
        app.update();
        Self {
            app,
            content,
            statuses,
            caster,
            defender,
        }
    }

    /// 用**构造器**把内容里的技能搭出来、认好施法者，并返回技能实体。
    fn spawn_ability(&mut self, id: &str, instant: bool) -> Entity {
        let mut ability = self
            .content
            .iter()
            .find(|ability| ability.id == id)
            .unwrap_or_else(|| panic!("内容里该有 `{id}`"))
            .clone();
        if instant {
            // 只测数值时把前摇压成 0，免得跟测试里的真实时钟较劲。
            ability.cast_time = 0.0;
        }

        let entity = self
            .app
            .world_mut()
            .spawn_scene(build_ability(&ability, &self.statuses))
            .unwrap_or_else(|error| panic!("技能 `{id}` 场景落地失败：{error:?}"))
            .id();
        self.app
            .world_mut()
            .entity_mut(entity)
            .insert(InvokedBy(self.caster));
        self.app.update();
        entity
    }

    /// 发一次释放请求，跑几帧。
    fn cast(&mut self, ability: Entity) {
        self.app.world_mut().write_message(CastRequest {
            caster: self.caster,
            ability,
        });
        for _ in 0..8 {
            self.app.update();
        }
    }

    fn value_of(&self, entity: Entity, name: &str) -> f32 {
        self.app
            .world()
            .entity(entity)
            .get::<Attributes>()
            .expect("有属性")
            .value(name)
    }

    fn health(&self) -> f32 {
        self.value_of(self.defender, "Health")
    }

    fn logs(&self) -> Vec<String> {
        self.app.world().resource::<Log>().0.clone()
    }

    fn rejections(&self) -> Vec<&'static str> {
        self.app.world().resource::<Rejections>().0.clone()
    }
}

#[test]
fn the_content_files_load_and_validate() {
    let mut app = test_app();
    let (content, statuses) = load_content(&mut app);
    // 不写死条数（内容会一直长）：查"每条该在的都在"。
    for id in ["Guarding", "Weakened", "Burning"] {
        assert!(statuses.get(id).is_some(), "status_defs.ron 里该有 `{id}`");
    }
    assert!(content.iter().any(|ability| ability.id == "basic_attack"));
    assert!(content.iter().any(|ability| ability.id == "goblin_slash"));
}

#[test]
fn content_lands_on_the_ability_as_components() {
    let mut fixture = Fixture::new(ATTRIBUTES);
    let entity = fixture.spawn_ability("basic_attack", true);

    let costs = fixture
        .app
        .world()
        .entity(entity)
        .get::<CastCosts>()
        .expect("费用该挂在技能根上");
    assert_eq!(costs.0, vec![("Action".to_string(), 1.0)]);

    let requirements = fixture
        .app
        .world()
        .entity(entity)
        .get::<AttributeRequirements>()
        .expect("需求该挂在技能根上");
    assert_eq!(requirements.len(), 1, "旧内容那条 `Action >= 1.0`");
}

#[test]
fn basic_attack_from_content_crits_for_eighteen_and_pays_one_action() {
    let mut fixture = Fixture::new(ATTRIBUTES);
    let entity = fixture.spawn_ability("basic_attack", true);

    fixture.cast(entity);

    // 攻 = 力量 10 + 5 = 15；防 = 护甲 3；raw = 12 ≥ 暴击线 5 ⇒ 暴击；伤害 12 × 1.5 = 18
    assert_eq!(fixture.health(), 82.0, "内容驱动的普攻该是 18 点暴击");
    assert_eq!(
        fixture.value_of(fixture.caster, "Action"),
        2.0,
        "放一次要花 1 点行动力"
    );
    let lines = fixture.logs();
    assert!(
        lines.iter().any(|line| line.contains("重击")),
        "实际：{lines:?}"
    );
}

#[test]
fn goblin_slash_never_crits_even_with_a_big_margin() {
    let mut fixture = Fixture::new(ATTRIBUTES);
    let entity = fixture.spawn_ability("goblin_slash", true);

    fixture.cast(entity);

    // 攻 = 10 + 3 = 13；防 = 3；raw = 10 ⇒ 命中（`crit_margin: 0.0` ⇒ 不暴击）⇒ 10 点
    assert_eq!(fixture.health(), 90.0, "余量 10 也不该暴击");
}

#[test]
fn a_cast_is_rejected_when_the_requirement_fails() {
    let mut fixture = Fixture::new(BROKE_ATTRIBUTES);
    let entity = fixture.spawn_ability("basic_attack", true);

    fixture.cast(entity);

    assert_eq!(fixture.health(), 100.0, "被拒的释放不该打到人");
    assert_eq!(
        fixture.value_of(fixture.caster, "Action"),
        0.0,
        "被拒的释放不该扣费"
    );
    assert!(!fixture.rejections().is_empty(), "该发出 `CastRejected`");
}

#[test]
fn the_cast_time_delays_the_hit() {
    let mut fixture = Fixture::new(ATTRIBUTES);
    // 内容和 `abilities.ron` 一致：前摇 1.0 秒。
    let entity = fixture.spawn_ability("basic_attack", false);

    fixture.cast(entity);

    // 测试里的真实时钟只走了几毫秒 ⇒ 前摇没走完 ⇒ 一点血都不该掉。
    assert_eq!(fixture.health(), 100.0, "1.0 秒的前摇不该在这几毫秒里走完");
    // 但**费用已经付了**（进状态机之前扣）——刻意语义，不是 bug。
    assert_eq!(
        fixture.value_of(fixture.caster, "Action"),
        2.0,
        "付款发生在进入状态机那一刻，不等命中"
    );
}

#[test]
fn a_self_buff_from_content_changes_the_casters_attributes() {
    let mut fixture = Fixture::new(ATTRIBUTES);
    let entity = fixture.spawn_ability("shield_block", true);
    assert_eq!(fixture.value_of(fixture.caster, "Armor"), 3.0, "起点");

    fixture.cast(entity);

    // `status_defs.ron` 的 `Guarding`：`who: Caster`，+5 护甲。
    assert_eq!(
        fixture.value_of(fixture.caster, "Armor"),
        8.0,
        "自身增益该挂到自己身上（`who: Caster` ⇒ `SpawnConfig::invoker`）"
    );
    assert_eq!(
        fixture.value_of(fixture.defender, "Armor"),
        3.0,
        "**不该**挂到对面身上"
    );
}

#[test]
fn a_debuff_from_content_lands_on_the_target() {
    let mut fixture = Fixture::new(ATTRIBUTES);
    let entity = fixture.spawn_ability("basic_attack", true);
    assert_eq!(fixture.value_of(fixture.defender, "Strength"), 10.0, "起点");

    fixture.cast(entity);

    // `status_defs.ron` 的 `Weakened`：`who: Target`，-6 力量。
    assert_eq!(
        fixture.value_of(fixture.defender, "Strength"),
        4.0,
        "减益该挂到对方身上（`who: Target` ⇒ `SpawnConfig::target`）"
    );
    assert_eq!(
        fixture.value_of(fixture.caster, "Strength"),
        10.0,
        "**不该**挂到自己身上"
    );
}

#[test]
fn a_status_is_removed_cleanly_when_it_expires() {
    let mut fixture = Fixture::new(ATTRIBUTES);
    let entity = fixture.spawn_ability("shield_block", true);
    fixture.cast(entity);
    assert_eq!(fixture.value_of(fixture.caster, "Armor"), 8.0, "增益生效");

    // 不等 2 秒（测试里没有真实时钟），直接模拟"状态失去 `Active`"——
    // 那正是到期/被驱散时发生的事，diesel 的 `sustained_modifier_remove` 负责回收。
    let state = fixture
        .app
        .world_mut()
        .query_filtered::<Entity, With<AttributeModifiers>>()
        .single(fixture.app.world())
        .expect("状态生效态实体（挂着 AttributeModifiers）");
    fixture.app.world_mut().entity_mut(state).remove::<Active>();
    for _ in 0..4 {
        fixture.app.update();
    }

    assert_eq!(
        fixture.value_of(fixture.caster, "Armor"),
        3.0,
        "失去 `Active` 时必须**精确还原**（不能反着再加一遍）"
    );
}
