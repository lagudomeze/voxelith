//! **反制**：让出 + 组件裁决 + 出手。
//!
//! 这是旧设计里最核心的一条链路，现在落在新栈上：
//!
//! ```text
//! 巨魔进入 #WindUp（threatening）→ threat 窗口开 → 时间冻结
//! 玩家 CounterStrike{riposte}
//!   ├─ 让出：玩家正在释放的技能收到 AbortInvocation（= replace 的一半）
//!   ├─ 裁决：riposte 带 Interrupts、巨魔重击带 SuperArmor → **打断不了**
//!   └─ 出手：CastRequest{riposte} → 门控 + 扣费 + 进状态机
//! 对照：哥布林劈砍没有 SuperArmor → 前摇被打断，威胁立刻离场，伤害不再落地
//! ```

use bevy::asset::AssetPlugin;
use bevy::prelude::*;
use bevy_diesel::prelude::*;
use bevy_diesel::target::Target;

use voxelith_abilities::attributes::{self, AttributeCatalog};
use voxelith_abilities::builder::{build_ability, install_statuses};
use voxelith_abilities::casting::CastRequest;
use voxelith_abilities::counter::{CounterStrike, Interrupts, SuperArmor};
use voxelith_abilities::skills::{self, AbilityRon};
use voxelith_abilities::statuses::{self, StatusCatalog};
use voxelith_abilities::threat::ThreatWindow;
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

struct Fixture {
    app: App,
    content: Vec<AbilityRon>,
    statuses: StatusCatalog,
    /// 怪物（发起威胁的一方）。
    monster: Entity,
    /// 玩家（被威胁、要反制的一方）。
    player: Entity,
}

impl Fixture {
    fn new() -> Self {
        let mut app = test_app();
        let (content, statuses) = load_content(&mut app);

        // 数值层：全局定义 + **角色自己的数**（现在每个角色可以不一样了）。
        let (_globals, _catalog, actors) =
            voxelith_abilities::actors::load_from_sources(ATTRIBUTES, ACTORS)
                .expect("属性与角色文件该能加载");
        let set = actors
            .set_for(&_globals, "player")
            .expect("角色文件里该有 player");
        // 玩家的属性：反应力来自 `attributes.ron`（1.0）✓
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
                Name::new("怪物"),
            ))
            .id();
        app.update();

        Self {
            app,
            content,
            statuses,
            monster,
            player,
        }
    }

    fn ability_def(&self, id: &str) -> AbilityRon {
        self.content
            .iter()
            .find(|ability| ability.id == id)
            .unwrap_or_else(|| panic!("内容里该有 `{id}`"))
            .clone()
    }

    /// 把某个技能搭出来、认好归属，返回技能实体。
    fn spawn_ability(&mut self, id: &str, owner: Entity) -> Entity {
        let def = self.ability_def(id);
        let entity = self
            .app
            .world_mut()
            .spawn_scene(build_ability(&def, &self.statuses))
            .unwrap_or_else(|error| panic!("技能 `{id}` 场景落地失败：{error:?}"))
            .id();
        self.app
            .world_mut()
            .entity_mut(entity)
            .insert(InvokedBy(owner));
        self.app.update();
        entity
    }

    /// 怪物开始一次释放（走到前摇，于是威胁开窗）。
    fn monster_starts(&mut self, id: &str) -> Entity {
        let entity = self.spawn_ability(id, self.monster);
        self.app.world_mut().write_message(CastRequest {
            caster: self.monster,
            ability: entity,
        });
        for _ in 0..2 {
            self.app.update();
        }
        entity
    }

    /// 玩家反制。
    fn player_counters(&mut self, id: &str) -> Entity {
        let entity = self.spawn_ability(id, self.player);
        self.app.world_mut().write_message(CounterStrike {
            caster: self.player,
            ability: entity,
        });
        for _ in 0..4 {
            self.app.update();
        }
        entity
    }

    fn window(&self) -> &ThreatWindow {
        self.app.world().resource::<ThreatWindow>()
    }

    fn value_of(&self, entity: Entity, name: &str) -> f32 {
        self.app
            .world()
            .entity(entity)
            .get::<Attributes>()
            .expect("有属性")
            .value(name)
    }

    fn has<T: Component>(&self, entity: Entity) -> bool {
        self.app.world().entity(entity).get::<T>().is_some()
    }
}

#[test]
fn the_content_flags_land_as_components() {
    let mut fixture = Fixture::new();
    let riposte = fixture.spawn_ability("riposte", fixture.player);
    let troll = fixture.spawn_ability("troll_smash", fixture.monster);

    assert!(
        fixture
            .app
            .world()
            .entity(riposte)
            .get::<Interrupts>()
            .is_some_and(|marker| marker.yes()),
        "`interrupts: true` 该落成组件"
    );
    assert!(
        fixture
            .app
            .world()
            .entity(troll)
            .get::<SuperArmor>()
            .is_some_and(|marker| marker.yes()),
        "`super_armor: true` 该落成组件"
    );
}

#[test]
fn a_counter_interrupts_a_wind_up_without_super_armor() {
    let mut fixture = Fixture::new();
    // 哥布林劈砍：`threatening` 但**没有**霸体。
    fixture.monster_starts("goblin_slash");
    assert_eq!(fixture.window().len(), 1, "先开窗");

    fixture.player_counters("riposte");

    assert!(
        !fixture.window().is_open(),
        "打断成功后威胁该立刻离场（前摇被打断 = 打不出来了）"
    );
    assert_eq!(
        fixture.value_of(fixture.player, "Reaction"),
        0.0,
        "反制自己也花了 1 点反应力（1.0 → 0.0）"
    );
}

#[test]
fn super_armor_survives_the_interrupt_but_the_counter_still_happens() {
    let mut fixture = Fixture::new();
    // 巨魔重击：`threatening` **且** `super_armor`。
    fixture.monster_starts("troll_smash");
    assert_eq!(fixture.window().len(), 1, "先开窗");

    fixture.player_counters("riposte");

    assert!(
        fixture.window().is_open(),
        "霸体打不断：威胁该继续挂在窗口里"
    );
    assert_eq!(
        fixture.value_of(fixture.player, "Reaction"),
        0.0,
        "打断失败不影响反制本身——它照样出手并扣费"
    );
}

#[test]
fn a_counter_without_a_threat_does_nothing() {
    let mut fixture = Fixture::new();
    assert!(!fixture.window().is_open(), "开局没有威胁");

    fixture.player_counters("riposte");

    assert_eq!(
        fixture.value_of(fixture.player, "Reaction"),
        1.0,
        "没有可反制的对象时不该扣费（`CounterStrike` 直接返回）"
    );
}
