//! **净化** 与 **状态类需求**（旧 `NoActiveAction`）——内容词汇的最后两块。
//!
//! ```text
//! 净化：命中 → Dispels → 找出目标的减益 → SupersedeStatus → #Removed → on_remove
//! 空闲：basic_attack 带 requires_idle ⇒ 施法者另有技能停在 #Invoking 时被拒
//! ```

use std::time::Duration;

use bevy::asset::AssetPlugin;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use bevy_diesel::prelude::*;

use voxelith_abilities::attributes::{self, AttributeCatalog};
use voxelith_abilities::builder::{build_ability, install_statuses};
use voxelith_abilities::casting::{CastRejected, CastRequest};
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
    // 确定的步长：墙钟 delta 会让"前摇还在不在"随机。
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
        16,
    )));
    app
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

        Self {
            app,
            content,
            statuses,
            caster,
            target,
        }
    }

    /// 施法者用**新造的一份**技能放一招；`compress` = 把前摇压成 0。
    fn request(&mut self, id: &str, compress: bool) -> Entity {
        let mut def = self
            .content
            .iter()
            .find(|ability| ability.id == id)
            .unwrap_or_else(|| panic!("内容里该有 `{id}`"))
            .clone();
        if compress {
            def.cast_time = 0.0;
        }
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
        ability
    }

    fn cast(&mut self, id: &str) {
        self.request(id, true);
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

    /// 上一帧有没有被拒、理由是什么。
    fn last_rejection(&mut self) -> Option<&'static str> {
        let mut rejected = self
            .app
            .world_mut()
            .resource_mut::<Messages<CastRejected>>();
        rejected.drain().last().map(|rejected| rejected.reason)
    }
}

#[test]
fn cleansing_removes_the_debuff_and_triggers_on_remove() {
    let mut fixture = Fixture::new();

    // 先给目标挂一个减益（`Marked`：命中 9 + on_apply 4）。
    fixture.cast("hunters_mark");
    let after_mark = fixture.health();
    assert_eq!(after_mark, 100.0 - 13.0);
    assert_eq!(fixture.instances("Marked"), 1, "印记挂上了");

    // 净化：移除目标的减益 ⇒ 走 `#Removed` ⇒ `on_remove` 3 点。
    fixture.cast("cleanse");

    assert_eq!(
        fixture.instances("Marked"),
        0,
        "净化之后印记该没了（走的是被移除那条边，不是到期）"
    );
    assert_eq!(
        fixture.health(),
        after_mark - 3.0,
        "被净化触发 `on_remove`（3 点），**不是**到期的 8 点"
    );
}

#[test]
fn a_skill_that_requires_idle_is_rejected_while_another_is_invoking() {
    let mut fixture = Fixture::new();

    // 第一发**不压前摇**：它会停在 `#Invoking` 里（1 秒前摇，测试只走 2 帧）。
    fixture.request("basic_attack", false);
    for _ in 0..2 {
        fixture.app.update();
    }

    // 第二发（同一招、另一个实例）：施法者还忙着 ⇒ 该被拒。
    fixture.request("basic_attack", false);
    fixture.app.update();
    assert_eq!(
        fixture.last_rejection(),
        Some("动作还没结束（这一招要求空闲）"),
        "`requires_idle` 要拦住连放两次"
    );
}
