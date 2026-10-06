//! **威胁窗口**：内容标了 `threatening` 的技能，前摇期间会让窗口打开并**冻结时间**。
//!
//! 这条链路是三件套没有、必须自己搭的那一块（gearbox 无守卫、diesel 无威胁概念）：
//!
//! ```text
//! 怪物释放 #Invoking → 进入 #WindUp（带 ThreatensPlayer(true)）
//!   └─ collect_threats   窗口里多一条威胁（source = 发起者，target = 它瞄的人）
//!   └─ freeze_while_…    Time<Virtual> 倍率 → 0（gearbox 的 Delay 读 Res<Time>，所以真停）
//! 前摇结束（#Fire 拿走 Active）/ 被销毁
//!   └─ retire_threats    威胁离场 + 时间恢复
//! ```

use bevy::asset::AssetPlugin;
use bevy::prelude::*;
use bevy_diesel::prelude::*;
use bevy_diesel::target::Target;

use voxelith_abilities::attributes::{self, AttributeCatalog};
use voxelith_abilities::builder::{build_ability, install_statuses};
use voxelith_abilities::casting::CastRequest;
use voxelith_abilities::skills::{self, AbilityRon};
use voxelith_abilities::statuses::{self, StatusCatalog};
use voxelith_abilities::threat::{ThreatWindow, ThreatensPlayer};
use voxelith_axiom::atoms::grid::CellPos;

const ATTRIBUTES: &str = include_str!("../../../assets/data/attributes.ron");
const ACTORS: &str = include_str!("../../../assets/data/actors.ron");
const STATUS_DEFS: &str = include_str!("../../../assets/data/status_defs.ron");
const ABILITIES: &str = include_str!("../../../assets/data/abilities.ron");

fn test_app() -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        bevy::scene::ScenePlugin,
        voxelith_abilities::plugin(),
    ));
    app
}

fn load_content(app: &mut App) -> (Vec<AbilityRon>, StatusCatalog) {
    let mut catalog = AttributeCatalog::default();
    catalog.add(&attributes::parse(ATTRIBUTES).expect("属性文件语法没问题"));
    let statuses = statuses::load(STATUS_DEFS, &catalog).expect("状态文件该能加载");
    let abilities = skills::load(ABILITIES, &catalog, &statuses).expect("技能文件该能加载");
    install_statuses(
        &mut app.world_mut().resource_mut::<TemplateRegistry>(),
        &statuses,
    );
    (abilities, statuses)
}

/// 一场"怪物打玩家"：怪物在原点（瞄着玩家），玩家在一格之外。
///
/// 注意方向与 `content_driven_ability` 相反——那边是玩家打目标，这边是**怪物威胁玩家**，
/// 而威胁窗口要的是"有人在打我"。
fn spawn_monster_and_player(app: &mut App) -> (Entity, Entity) {
    // 数值层：全局定义 + **角色自己的数**（现在每个角色可以不一样了）。
    let (globals, _catalog, actors) =
        voxelith_abilities::actors::load_from_sources(ATTRIBUTES, ACTORS)
            .expect("属性与角色文件该能加载");
    let set = actors
        .set_for(&globals, "player")
        .expect("角色文件里该有 player");

    let player = app
        .world_mut()
        .spawn((
            CellPos::new(1, 0),
            Attributes::new(),
            AttributeInitializer::new(set.clone()),
            Name::new("玩家"),
        ))
        .id();
    let monster = app
        .world_mut()
        .spawn((
            CellPos::ZERO,
            Attributes::new(),
            AttributeInitializer::new(set),
            InvokerTarget::entity(player, IVec2::new(1, 0)),
            Name::new("哥布林"),
        ))
        .id();
    (monster, player)
}

struct Fixture {
    app: App,
    content: Vec<AbilityRon>,
    statuses: StatusCatalog,
    monster: Entity,
    player: Entity,
}

impl Fixture {
    fn new() -> Self {
        let mut app = test_app();
        let (content, statuses) = load_content(&mut app);
        let (monster, player) = spawn_monster_and_player(&mut app);
        app.update();
        Self {
            app,
            content,
            statuses,
            monster,
            player,
        }
    }

    /// 怪物释放某个技能（前摇**不压缩**：威胁只在真前摇里存在）。
    fn monster_casts(&mut self, id: &str) -> Entity {
        let ability = self
            .content
            .iter()
            .find(|ability| ability.id == id)
            .unwrap_or_else(|| panic!("内容里该有 `{id}`"))
            .clone();
        let entity = self
            .app
            .world_mut()
            .spawn_scene(build_ability(&ability, &self.statuses))
            .expect("技能场景能落地")
            .id();
        self.app
            .world_mut()
            .entity_mut(entity)
            .insert(InvokedBy(self.monster));
        self.app.update();

        self.app.world_mut().write_message(CastRequest {
            caster: self.monster,
            ability: entity,
        });
        // 两帧：一帧走门控与扣费，一帧让 `#WindUp` 拿到 `Active`。
        for _ in 0..2 {
            self.app.update();
        }
        // 诊断（也是好不变量）：释放真发生了的话费用已经扣掉。
        let action = self
            .app
            .world()
            .entity(self.monster)
            .get::<Attributes>()
            .expect("有属性")
            .value("Action");
        assert_eq!(action, 2.0, "怪物释放该扣 1 点行动力（3.0 → 2.0）");
        entity
    }

    fn window(&self) -> &ThreatWindow {
        self.app.world().resource::<ThreatWindow>()
    }

    fn virtual_speed(&self) -> f32 {
        self.app
            .world()
            .resource::<Time<Virtual>>()
            .relative_speed()
    }
}

#[test]
fn a_threatening_wind_up_opens_the_window_and_freezes_time() {
    let mut fixture = Fixture::new();
    assert!(!fixture.window().is_open(), "开局窗口是空的");
    assert_eq!(fixture.virtual_speed(), 1.0, "时间正常流动");

    fixture.monster_casts("goblin_slash");

    let window = fixture.window();
    assert_eq!(window.len(), 1, "一条威胁");
    let threat = window.first().expect("有威胁");
    assert_eq!(threat.source, fixture.monster, "发起者是怪物");
    assert_eq!(threat.target, fixture.player, "被威胁的是玩家");
    assert_eq!(
        fixture.virtual_speed(),
        0.0,
        "窗口开着 ⇒ 虚拟时间倍率 0 ⇒ gearbox 的延时真的停住"
    );
}

#[test]
fn a_non_threatening_ability_leaves_the_window_shut() {
    let mut fixture = Fixture::new();
    // `basic_attack` 是玩家的招（内容里没标 `threatening`），怪物放它也不该开窗口。
    fixture.monster_casts("basic_attack");
    assert!(!fixture.window().is_open(), "没标威胁的技能不该冻结全场");
    assert_eq!(fixture.virtual_speed(), 1.0, "时间照常流动");
}

#[test]
fn retiring_the_wind_up_closes_the_window_and_resumes_time() {
    let mut fixture = Fixture::new();
    fixture.monster_casts("goblin_slash");
    assert!(fixture.window().is_open(), "先开起来");

    // "前摇结束"在实现上就是那个状态失去 `Active`（进 `#Fire` 时发生）。
    // 测试里不等它自然走到（时间正被冻着），直接拿走。
    let wind_up = fixture
        .app
        .world_mut()
        .query_filtered::<Entity, With<ThreatensPlayer>>()
        .single(fixture.app.world())
        .expect("前摇态实体（挂着 ThreatensPlayer）");
    fixture
        .app
        .world_mut()
        .entity_mut(wind_up)
        .remove::<Active>();
    for _ in 0..3 {
        fixture.app.update();
    }

    assert!(!fixture.window().is_open(), "威胁该离场");
    assert_eq!(fixture.virtual_speed(), 1.0, "时间该恢复");
}

#[test]
fn a_lost_signal_still_clears_the_window() {
    let mut fixture = Fixture::new();
    fixture.monster_casts("goblin_slash");
    assert!(fixture.window().is_open(), "先开起来");

    // 模拟"信号漏了"：只把**标记**摘掉（`Active` 没动、也没触发 `RemovedComponents`）。
    // 靠的就是 `retire_threats` 里那行存活对账——否则窗口会永久卡住、游戏再也解不开冻结。
    let wind_up = fixture
        .app
        .world_mut()
        .query_filtered::<Entity, With<ThreatensPlayer>>()
        .single(fixture.app.world())
        .expect("前摇态实体");
    fixture
        .app
        .world_mut()
        .entity_mut(wind_up)
        .remove::<ThreatensPlayer>();
    for _ in 0..3 {
        fixture.app.update();
    }

    assert!(
        !fixture.window().is_open(),
        "存活对账必须把漏信号的条目清掉"
    );
    assert_eq!(fixture.virtual_speed(), 1.0, "时间必须恢复（不能永久冻结）");
}
