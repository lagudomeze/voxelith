//! **持续伤害（DoT）**：内容里的 `tick` 真的会按秒掉血，而且**只在生效期间**。
//!
//! ```text
//! 火焰鞭笞命中 → 目标挂上 Burning（who: Target, tick: every 1.0s / -3 Health）
//!   PeriodicEffect（状态生效态上）→ tick_periodic_effects → PeriodicTick
//!     → fire_periodic_effects::<GridBackend> → 读该状态的 GoOffConfig → GoOffOrigin
//!       → diesel propagate → GoOff → instant_set_system → 扣 3 点
//! 状态失去 Active → 计时停、不再掉血
//! ```
//!
//! 时钟用 `TimeUpdateStrategy::ManualDuration`：每帧一个**确定**的时长，
//! 否则"每秒掉 3 点"要靠真实墙钟，测试会飘。

use std::time::Duration;

use bevy::asset::AssetPlugin;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use bevy_diesel::prelude::*;

use voxelith_abilities::attributes::{self, AttributeCatalog};
use voxelith_abilities::builder::{build_ability, install_statuses};
use voxelith_abilities::casting::CastRequest;
use voxelith_abilities::periodic::PeriodicEffect;
use voxelith_abilities::skills::{self, AbilityRon};
use voxelith_abilities::statuses::{self, StatusCatalog};
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

/// 手动推进时钟：先放宽 `max_delta`（默认 250ms 会把 1 秒夹掉），再给一帧固定时长。
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
    /// 一个"打**玩家**"的场：施法者在原点（瞄着目标），目标在一格之外。
    ///
    /// 注意方向：`Burning` 是 `who: Target` 的减益，所以要有一个"对方"来承受它。
    fn new() -> Self {
        let mut app = test_app();
        let mut catalog = AttributeCatalog::default();
        catalog.add(&attributes::parse(ATTRIBUTES).expect("属性文件语法没问题"));
        let statuses = statuses::load(STATUS_DEFS, &catalog).expect("状态文件该能加载");
        let content = skills::load(ABILITIES, &catalog, &statuses).expect("技能文件该能加载");
        install_statuses(
            &mut app.world_mut().resource_mut::<TemplateRegistry>(),
            &statuses,
        );

        // 数值层：全局定义 + **角色自己的数**（现在每个角色可以不一样了）。
        let (_globals, _catalog, actors) =
            voxelith_abilities::actors::load_from_sources(ATTRIBUTES, ACTORS)
                .expect("属性与角色文件该能加载");
        let set = actors
            .set_for(&_globals, "player")
            .expect("角色文件里该有 player");
        let target = app
            .world_mut()
            .spawn((
                CellPos::new(1, 0),
                Attributes::new(),
                AttributeInitializer::new(set.clone()),
                Name::new("目标"),
            ))
            .id();
        let caster = app
            .world_mut()
            .spawn((
                CellPos::ZERO,
                Attributes::new(),
                AttributeInitializer::new(set),
                InvokerTarget::entity(target, IVec2::new(1, 0)),
                Name::new("施法者"),
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

    /// 施法者放一招（前摇压成 0，方便确定性地看后果）。
    fn cast(&mut self, id: &str) {
        let mut ability = self
            .content
            .iter()
            .find(|ability| ability.id == id)
            .unwrap_or_else(|| panic!("内容里该有 `{id}`"))
            .clone();
        ability.cast_time = 0.0;
        let entity = self
            .app
            .world_mut()
            .spawn_scene(build_ability(&ability, &self.statuses))
            .expect("技能场景能落地")
            .id();
        self.app
            .world_mut()
            .entity_mut(entity)
            .insert(InvokedBy(self.caster));
        self.app.update();
        self.app.world_mut().write_message(CastRequest {
            caster: self.caster,
            ability: entity,
        });
        // 命中这几帧给一个**确定的小步长**：delta 为 0 时前摇计时器永远不跳，
        // 命中就根本不会发生（曾经正是这么红的）；而 16ms 又远小于 1 秒，
        // 所以"命中伤害"与"第一跳"仍然分得清。
        self.app
            .world_mut()
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
                16,
            )));
        for _ in 0..3 {
            self.app.update();
        }
    }

    fn health(&self) -> f32 {
        self.app
            .world()
            .entity(self.target)
            .get::<Attributes>()
            .expect("有属性")
            .value("Health")
    }

    /// 目标身上有没有周期效果（挂在状态的生效态上）。
    fn periodic_state(&mut self) -> Option<Entity> {
        let mut query = self
            .app
            .world_mut()
            .query_filtered::<Entity, With<PeriodicEffect>>();
        query.iter(self.app.world()).next()
    }
}

#[test]
fn a_content_dot_ticks_for_exactly_its_amount() {
    let mut fixture = Fixture::new();
    let before = fixture.health();

    fixture.cast("flame_lash");

    // 命中 11 点（攻 14 - 防 3），**外加应用时立刻跳的第一下 3 点**：
    // `#Active` 上挂着 `GoOffConfig`，所以 diesel 的 `go_off_on_entry` 在进入生效态时会响一次。
    // 这是刻意的语义（"上状态即第一跳"），不是 bug——见 `periodic` 的模块文档。
    let after_hit = fixture.health();
    assert_eq!(after_hit, before - 14.0, "命中 11 + 应用时第一跳 3");
    assert!(
        fixture.periodic_state().is_some(),
        "目标身上该有一个带周期效果的状态"
    );

    // 每帧 1 秒 ⇒ 每个间隔跳一次，每次正好 3 点。
    let mut drops = Vec::new();
    let mut previous = after_hit;
    for _ in 0..4 {
        step(&mut fixture.app, 1.0);
        let now = fixture.health();
        drops.push(previous - now);
        previous = now;
    }
    assert!(
        drops.iter().all(|drop| *drop == 0.0 || *drop == 3.0),
        "每一跳只能是 3 点（多打/少打都是 bug）：{drops:?}"
    );
    assert!(
        drops.iter().filter(|drop| **drop == 3.0).count() >= 2,
        "至少该跳两次：{drops:?}"
    );
}

#[test]
fn the_tick_stops_when_the_status_leaves_its_active_state() {
    let mut fixture = Fixture::new();
    fixture.cast("flame_lash");
    let state = fixture.periodic_state().expect("有周期状态");

    // 先让它跳一次，确认机制在跑。
    step(&mut fixture.app, 1.0);
    step(&mut fixture.app, 1.0);
    let ticking = fixture.health();

    // 状态离开生效态（到期 / 被驱散都是这件事）。
    fixture
        .app
        .world_mut()
        .entity_mut(state)
        .remove::<bevy_gearbox::Active>();
    for _ in 0..3 {
        step(&mut fixture.app, 1.0);
    }

    assert_eq!(
        fixture.health(),
        ticking,
        "离开生效态后周期伤害必须停（计时随 `Active` 一起停）"
    );
}
