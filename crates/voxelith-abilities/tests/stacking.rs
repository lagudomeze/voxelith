//! **叠层规则**：同一个状态第二次挂到同一个人身上时，按内容写的规则处理。
//!
//! ```text
//! 新实例（diesel 的 spawn_system 生成）→ Added<Stacking>
//!   ├─ Ignore       已经有同种 → 丢掉新的（Weakened）
//!   ├─ Refresh      留最新的，旧的整棵树销毁（Guarding / Stunned）
//!   └─ Stack(3)     最多三个，超了丢最旧的（Burning）
//! ```
//!
//! 测试每次都用**新造的技能实体**去放：技能自己带着状态机（`#Cooldown` 里没法再放），
//! 而"两个人各放一招同种状态"才是真实场景。

use bevy::asset::AssetPlugin;
use bevy::prelude::*;
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
    // 确定的步长：墙钟 delta 会让"命中在第几帧"随机器而变。
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        std::time::Duration::from_millis(16),
    ));
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

    /// 施法者用**新造的一份**技能放一招。
    fn cast(&mut self, id: &str) {
        let mut def = self
            .content
            .iter()
            .find(|ability| ability.id == id)
            .unwrap_or_else(|| panic!("内容里该有 `{id}`"))
            .clone();
        // 前摇压成 0：测试关心的是"挂上几次"，不是"多久命中"。
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
        // 4 帧：门控 → 命中 → 生成状态 → 帧末叠层对账 → 修饰符回收。
        for _ in 0..4 {
            self.app.update();
        }
    }

    /// 现在活着的、身份为 `id` 的状态实例数。
    fn instances(&mut self, id: &str) -> usize {
        let mut query = self.app.world_mut().query::<&StatusIdentity>();
        query
            .iter(self.app.world())
            .filter(|identity| identity.0 == id)
            .count()
    }

    fn stat(&self, entity: Entity, name: &str) -> f32 {
        self.app
            .world()
            .entity(entity)
            .get::<Attributes>()
            .expect("有属性")
            .value(name)
    }
}

#[test]
fn ignore_keeps_the_first_instance() {
    let mut fixture = Fixture::new();
    let strength_before = fixture.stat(fixture.target, "Strength");

    fixture.cast("basic_attack");
    fixture.cast("basic_attack");

    assert_eq!(
        fixture.instances("Weakened"),
        1,
        "`Ignore`：已经有虚弱了就不再挂"
    );
    assert_eq!(
        fixture.stat(fixture.target, "Strength"),
        strength_before - 6.0,
        "只削一次力量（10 - 6），不是 10 - 12"
    );
}

#[test]
fn refresh_does_not_double_the_modifiers() {
    let mut fixture = Fixture::new();
    let armor_before = fixture.stat(fixture.caster, "Armor");

    fixture.cast("shield_block");
    fixture.cast("shield_block");
    // 再走两帧：旧实例被销毁后，它的修饰符由 diesel 在下一帧回收。
    fixture.app.update();
    fixture.app.update();

    assert_eq!(
        fixture.instances("Guarding"),
        1,
        "`Refresh`：只留最新那一个"
    );
    assert_eq!(
        fixture.stat(fixture.caster, "Armor"),
        armor_before + 5.0,
        "刷新不等于叠加：护甲 +5，不是 +10"
    );
}

#[test]
fn stack_three_caps_the_instances() {
    let mut fixture = Fixture::new();

    fixture.cast("flame_lash");
    fixture.cast("flame_lash");
    assert_eq!(fixture.instances("Burning"), 2, "可以叠到两层");

    fixture.cast("flame_lash");
    assert_eq!(fixture.instances("Burning"), 3, "叠到三层");

    fixture.cast("flame_lash");
    assert_eq!(
        fixture.instances("Burning"),
        3,
        "`Stack(3)`：第四次要丢最旧的，总数仍是 3"
    );
}

#[test]
fn stack_zero_is_rejected_at_load_time() {
    // `Stack(0)` 意味着这个状态永远挂不上——沉默的错，加载期就该拦。
    let source = r#"[
        (id: "Never", duration: 1.0, who: Target, stacking: Stack(0)),
    ]"#;
    let mut catalog = AttributeCatalog::default();
    catalog.add(&attributes::parse(ATTRIBUTES).expect("属性文件"));
    let error = statuses::load(source, &catalog).expect_err("该报错");
    let message = format!("{error}");
    assert!(
        message.contains("Stack(0)") || message.contains("最大层数"),
        "错误信息该说清是叠层写坏了：{message}"
    );
}
