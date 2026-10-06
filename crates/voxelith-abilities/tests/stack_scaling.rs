//! **按层数缩放**（旧 `Value::Mul(Stacks, …)` 的继任者）。
//!
//! 机制只有两半，合起来就够用：
//!
//! ```text
//! ① 层数变成属性：每个状态实例给自己那条 `<id>Stacks` 各 +1
//!    （`AttributeModifiers` 本来就是累加，所以宿主身上的值 = 层数）
//! ② 周期跳可以写表达式：`amount_expr: "-1.0 * CorrodingStacks * CorrodingStacks"`
//! ```
//!
//! 于是"每层 3 点"（线性）与"层数平方"（非线性）都只是内容里换个写法。
//!
//! ⚠️ 只对**周期跳**断言"稳"：挂上那一跳发生在层数落定之前（修饰符由 diesel 在 `Update`
//! 里施加，首跳发生在 gearbox 调度里），所以首跳可能按 0 层算——这一条写在
//! `statuses::TickRon::amount_expr` 的文档里。

use std::time::Duration;

use bevy::asset::AssetPlugin;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use bevy_diesel::prelude::*;

use voxelith_abilities::attributes::AttributeCatalog;
use voxelith_abilities::builder::{build_ability, install_statuses};
use voxelith_abilities::casting::CastRequest;
use voxelith_abilities::skills::{self, AbilityRon};
use voxelith_abilities::stacking::StatusIdentity;
use voxelith_abilities::statuses::{self, StatusCatalog};
use voxelith_abilities::{AttributeInitializer, Attributes};
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
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
        16,
    )));
    app
}

/// 手动推进时钟（先放宽 `max_delta`，默认 250ms 会把步长夹掉）。
fn step(app: &mut App, seconds: f32) {
    let duration = Duration::from_secs_f32(seconds);
    app.world_mut()
        .resource_mut::<Time<Virtual>>()
        .set_max_delta(duration.max(Duration::from_millis(1)));
    app.world_mut()
        .insert_resource(TimeUpdateStrategy::ManualDuration(duration));
    app.update();
}

struct Fixture {
    app: App,
    content: Vec<AbilityRon>,
    statuses: StatusCatalog,
    caster: Entity,
    target: Entity,
}

impl Fixture {
    fn new() -> Self {
        let mut app = test_app();
        let (globals, catalog, actors) =
            voxelith_abilities::actors::load_from_sources(ATTRIBUTES, ACTORS)
                .expect("属性与角色文件该能加载");
        let statuses = statuses::load(STATUS_DEFS, &catalog).expect("状态文件该能加载");
        let content = skills::load(ABILITIES, &catalog, &statuses).expect("技能文件该能加载");
        install_statuses(
            &mut app.world_mut().resource_mut::<TemplateRegistry>(),
            &statuses,
        );

        let target_set = actors.set_for(&globals, "goblin").expect("goblin");
        let caster_set = actors.set_for(&globals, "player").expect("player");
        let target = app
            .world_mut()
            .spawn((
                CellPos::new(1, 0),
                Attributes::new(),
                AttributeInitializer::new(target_set),
                Name::new("哥布林"),
            ))
            .id();
        let caster = app
            .world_mut()
            .spawn((
                CellPos::ZERO,
                Attributes::new(),
                AttributeInitializer::new(caster_set),
                InvokerTarget::entity(target, IVec2::new(1, 0)),
                Name::new("玩家"),
            ))
            .id();
        app.update();

        Self {
            app,
            content,
            statuses,
            caster,
            target,
        }
    }

    /// 施法者用**新造的一份**技能放一招（前摇压成 0，只关心挂了几层）。
    fn cast(&mut self, id: &str) {
        let mut def = self
            .content
            .iter()
            .find(|ability| ability.id == id)
            .unwrap_or_else(|| panic!("内容里该有 `{id}`"))
            .clone();
        def.cast_time = 0.0;
        let ability = self
            .app
            .world_mut()
            .spawn_scene(build_ability(&def, &self.statuses))
            .expect("技能场景能落地")
            .id();
        self.app
            .world_mut()
            .entity_mut(ability)
            .insert(InvokedBy(self.caster));
        self.app.world_mut().write_message(CastRequest {
            caster: self.caster,
            ability,
        });
        for _ in 0..4 {
            self.app.update();
        }
    }

    fn value(&self, entity: Entity, name: &str) -> f32 {
        self.app
            .world()
            .entity(entity)
            .get::<Attributes>()
            .expect("有属性")
            .value(name)
    }

    fn health(&self) -> f32 {
        self.value(self.target, "Health")
    }

    fn instances(&mut self, id: &str) -> usize {
        let mut query = self.app.world_mut().query::<&StatusIdentity>();
        query
            .iter(self.app.world())
            .filter(|identity| identity.0 == id)
            .count()
    }
}

#[test]
fn the_stack_count_becomes_an_attribute() {
    let mut fixture = Fixture::new();
    fixture.cast("acid_spray");
    fixture.app.update();
    assert_eq!(
        fixture.value(fixture.target, "CorrodingStacks"),
        1.0,
        "挂一层 ⇒ `CorrodingStacks = 1`（实例自己 +1，不是谁手写的）"
    );

    fixture.cast("acid_spray");
    fixture.cast("acid_spray");
    fixture.app.update();
    assert_eq!(fixture.instances("Corroding"), 3, "叠到三层");
    assert_eq!(
        fixture.value(fixture.target, "CorrodingStacks"),
        3.0,
        "三个实例各 +1 ⇒ 3"
    );
}

#[test]
fn a_tick_expression_scales_with_the_stack_count() {
    let mut fixture = Fixture::new();

    // 先叠满三层（这几帧步长很小，所以不会有周期跳），再让层数落定。
    fixture.cast("acid_spray");
    fixture.cast("acid_spray");
    fixture.cast("acid_spray");
    fixture.app.update();
    assert_eq!(fixture.value(fixture.target, "CorrodingStacks"), 3.0);

    // 推进 1 秒：**三个实例各跳一次**，每次 `MaxHealth / -20 = -3` ⇒ 共 9。
    // 线性叠加来自"实例数"，不是来自表达式——这才是新栈的模型（见 `TickRon::amount_expr`）。
    let before = fixture.health();
    step(&mut fixture.app, 1.0);
    let drop = before - fixture.health();
    assert!(
        (drop - 9.0).abs() < 1e-3,
        "三层 ⇒ 三次一跳 × 每次 3 点 = 9 点，实际掉了 {drop}"
    );
}

#[test]
fn one_stack_ticks_the_expression_once() {
    let mut fixture = Fixture::new();
    fixture.cast("acid_spray");
    fixture.app.update();
    assert_eq!(fixture.value(fixture.target, "CorrodingStacks"), 1.0);

    let before = fixture.health();
    step(&mut fixture.app, 1.0);
    let drop = before - fixture.health();
    assert!(
        (drop - 3.0).abs() < 1e-3,
        "一层 ⇒ 表达式算一次（哥布林最大生命 60 的 5% = 3 点），实际掉了 {drop}"
    );
}

#[test]
fn amount_and_amount_expr_cannot_both_be_written() {
    // 表达式编译要用 gauge 的**进程级 interner**，它由 `AttributesPlugin` 初始化；
    // 少了这一步会在 gauge 内部 panic（"Global interner not initialized"）。
    let _app = test_app();
    let source = r#"[
        (id: "Both", duration: 1.0, who: Target,
         tick: (every: 1.0, amount: -3.0, amount_expr: "-1.0", attribute: "Health")),
    ]"#;
    let (_globals, catalog, _actors) =
        voxelith_abilities::actors::load_from_sources(ATTRIBUTES, ACTORS).expect("内容");
    let error = statuses::load(source, &catalog).expect_err("该报错");
    let message = format!("{error}");
    assert!(
        message.contains("只能写一个"),
        "错误该说清二选一：{message}"
    );
}

#[test]
fn a_broken_tick_expression_is_caught_at_load_time() {
    let _app = test_app();
    let source = r#"[
        (id: "Broken", duration: 1.0, who: Target,
         tick: (every: 1.0, amount_expr: "-1.0 * * 2", attribute: "Health")),
    ]"#;
    let (_globals, catalog, _actors) =
        voxelith_abilities::actors::load_from_sources(ATTRIBUTES, ACTORS).expect("内容");
    let error = statuses::load(source, &catalog).expect_err("该报错");
    assert!(
        format!("{error}").contains("编译不过"),
        "表达式写错该在加载期报：{error}"
    );
}
