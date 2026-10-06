//! **进出效果**：状态在三个时机各能打一下（旧 `on_apply` / `on_expire` / `on_remove`）。
//!
//! ```text
//! 挂上        #Active 上的 GoOffConfig   → on_apply
//! 自然到期    #Expired 上的 GoOffConfig  → on_expire
//! 被顶掉/驱散 #Removed 上的 GoOffConfig  → on_remove
//! ```
//!
//! **"为什么离场"做成了不同的目标状态**，而不是旧引擎里那个 `RemovalReason` 枚举——
//! 结局是一个地方，效果就挂在那个地方。于是"到期爆炸"与"被净化时反噬"是两条独立的边。
//!
//! 用 `Marked`（猎手印记）验证三条路，因为它的三个时机都有确定的伤害：
//! `on_apply -4` / `on_expire -8` / `on_remove -3`，而 `hunters_mark` 本身命中 9 点（永不暴击）。

use std::time::Duration;

use bevy::asset::AssetPlugin;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use bevy_diesel::prelude::*;

use voxelith_abilities::attributes::{self, AttributeCatalog};
use voxelith_abilities::builder::{build_ability, install_statuses};
use voxelith_abilities::casting::CastRequest;
use voxelith_abilities::skills::{self, AbilityRon};
use voxelith_abilities::stacking::StatusIdentity;
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

/// 手动推进时钟：先放宽 `max_delta`（默认 250ms 会把步长夹掉），再给一帧固定时长。
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
        // 命中期给一个确定的小步长（delta 为 0 时前摇计时器永远不跳）。
        app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
            16,
        )));

        Self {
            app,
            content,
            statuses,
            caster,
            target,
        }
    }

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

    fn health(&self) -> f32 {
        self.app
            .world()
            .entity(self.target)
            .get::<Attributes>()
            .expect("有属性")
            .value("Health")
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
fn on_apply_fires_when_the_status_lands() {
    let mut fixture = Fixture::new();
    let before = fixture.health();

    fixture.cast("hunters_mark");

    assert_eq!(
        fixture.health(),
        before - 9.0 - 4.0,
        "命中 9 点 + 挂上的 `on_apply` 4 点"
    );
    assert_eq!(fixture.instances("Marked"), 1, "印记该挂上了（还没到期）");
}

#[test]
fn on_expire_fires_at_natural_expiry() {
    let mut fixture = Fixture::new();
    fixture.cast("hunters_mark");
    let after_apply = fixture.health();

    // `Marked` 的时长是 0.5 秒：推进过去，让它**自然到期**。
    for _ in 0..4 {
        step(&mut fixture.app, 0.2);
    }

    assert_eq!(
        fixture.health(),
        after_apply - 8.0,
        "到期引爆 8 点（`on_expire`）"
    );
    assert_eq!(
        fixture.instances("Marked"),
        0,
        "到期后实例该被收场（分两步：先卸修饰符，下一帧才销毁）"
    );
}

#[test]
fn on_remove_fires_when_superseded_instead_of_on_expire() {
    let mut fixture = Fixture::new();

    // 第一次挂：命中 9 + on_apply 4。
    fixture.cast("hunters_mark");
    let after_first = fixture.health();
    assert_eq!(after_first, 100.0 - 13.0);

    // 第二次挂：`Refresh` 会把旧的顶掉 ⇒ 旧实例走 `#Removed`（on_remove 3），
    // **不是** `#Expired`（那会是 8）。新的那次照常 on_apply 4。
    fixture.cast("hunters_mark");
    assert_eq!(
        fixture.health(),
        after_first - 9.0 - 4.0 - 3.0,
        "第二次：命中 9 + 新印记 on_apply 4 + 旧印记 on_remove 3"
    );
    assert_eq!(fixture.instances("Marked"), 1, "`Refresh` 只留最新那一个");
}
