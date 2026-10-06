//! **硬控**：状态封掉某一类技能（旧 `blocks_tags`）。
//!
//! ```text
//! 巨魔重击命中 → 玩家挂上 Stunned（who: Target, blocks: [ATTACK, COUNTER, MOVEMENT]）
//!   derive_blocked_tags  把"谁被封了什么"落到**宿主**身上 → 玩家：BlockedTags(ATTACK|COUNTER|MOVEMENT)
//! 玩家请求 basic_attack（tags: [ATTACK]）
//!   check_cast_requests  (BlockedTags & SkillTags) != 0 → **拒绝**
//! 玩家请求 flame_lash（tags: [SPELL]）→ 没有交集 → 照常放
//! ```
//!
//! 关键设计取舍：门控**不现场反查**"这个状态封的是谁"——那要沿 `InvokerTarget` 解析，
//! 而怪物会重新选目标，于是三秒前挂的眩晕会指到别人身上。所以由 `derive_blocked_tags`
//! 在**状态生效时**把它落到宿主身上，门控只做一次位与。

use bevy::asset::AssetPlugin;
use bevy::prelude::*;
use bevy_diesel::prelude::*;

use voxelith_abilities::attributes::{self, AttributeCatalog};
use voxelith_abilities::builder::{build_ability, install_statuses};
use voxelith_abilities::casting::{BlockedTags, CastRequest};
use voxelith_abilities::skills::{self, AbilityRon};
use voxelith_abilities::statuses::{self, StatusCatalog};
use voxelith_abilities::tags::SkillTag;
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
                Name::new("巨魔"),
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

    fn spawn_ability(&mut self, id: &str, owner: Entity) -> Entity {
        let mut def = self
            .content
            .iter()
            .find(|ability| ability.id == id)
            .unwrap_or_else(|| panic!("内容里该有 `{id}`"))
            .clone();
        // 前摇压成 0：测试要看的是"能不能放"，不是"多久命中"。
        def.cast_time = 0.0;
        let entity = self
            .app
            .world_mut()
            .spawn_scene(build_ability(&def, &self.statuses))
            .expect("技能场景能落地")
            .id();
        self.app
            .world_mut()
            .entity_mut(entity)
            .insert(InvokedBy(owner));
        self.app.update();
        entity
    }

    /// 让巨魔打玩家一下（于是玩家挂上它带的那个状态）。
    fn monster_hits_player(&mut self, id: &str) {
        let ability = self.spawn_ability(id, self.monster);
        self.app.world_mut().write_message(CastRequest {
            caster: self.monster,
            ability,
        });
        for _ in 0..4 {
            self.app.update();
        }
    }

    /// 玩家试图放一招，返回被拒的理由（`None` = 放出去了）。
    fn player_tries(&mut self, id: &str) -> Option<&'static str> {
        let ability = self.spawn_ability(id, self.player);
        self.app.world_mut().write_message(CastRequest {
            caster: self.player,
            ability,
        });
        self.app.update();
        self.app.update();
        let mut rejected = self
            .app
            .world_mut()
            .resource_mut::<Messages<voxelith_abilities::casting::CastRejected>>();
        rejected.drain().last().map(|rejected| rejected.reason)
    }

    fn blocked(&self) -> Option<SkillTag> {
        let mask = self
            .app
            .world()
            .entity(self.player)
            .get::<BlockedTags>()
            .map(|blocked| blocked.0);
        let mask = mask?;
        SkillTag::ALL.into_iter().find(|tag| mask.contains(*tag))
    }

    fn action(&self) -> f32 {
        self.app
            .world()
            .entity(self.player)
            .get::<Attributes>()
            .expect("有属性")
            .value("Action")
    }
}

#[test]
fn a_stun_lands_on_its_host_as_blocked_tags() {
    let mut fixture = Fixture::new();
    assert!(fixture.blocked().is_none(), "开局没被控");

    fixture.monster_hits_player("troll_smash");

    let mask = fixture
        .app
        .world()
        .entity(fixture.player)
        .get::<BlockedTags>()
        .expect("被打晕之后宿主身上该有 BlockedTags")
        .0;
    assert!(mask.contains(SkillTag::Attack), "攻击被封");
    assert!(mask.contains(SkillTag::Counter), "反制被封");
    assert!(mask.contains(SkillTag::Movement), "位移被封");
    assert!(!mask.contains(SkillTag::Spell), "法术没被封");
}

#[test]
fn hard_control_blocks_only_its_own_categories() {
    let mut fixture = Fixture::new();
    fixture.monster_hits_player("troll_smash");

    // 攻击类：被封。
    let reason = fixture.player_tries("basic_attack");
    assert_eq!(
        reason,
        Some("被控制：这一类技能被封"),
        "眩晕期间攻击类该放不出来"
    );

    // 法术类：照常放（而且真的扣了费 —— 说明它走完了门控）。
    let action_before = fixture.action();
    let reason = fixture.player_tries("flame_lash");
    assert_eq!(reason, None, "法术不在眩晕的封禁类别里，该放得出来");
    assert_eq!(
        fixture.action(),
        action_before - 1.0,
        "放出去了就该扣费（1 点行动力）"
    );
}

#[test]
fn control_expires_with_its_status() {
    let mut fixture = Fixture::new();
    fixture.monster_hits_player("troll_smash");
    assert!(
        fixture.player_tries("basic_attack").is_some(),
        "先确认在被控状态里"
    );

    // 状态到期 / 被驱散在实现上都是"失去 `Active`"。
    let state = {
        let mut query = fixture
            .app
            .world_mut()
            .query_filtered::<Entity, With<voxelith_abilities::casting::BlocksTags>>();
        query.iter(fixture.app.world()).next().expect("有硬控状态")
    };
    fixture
        .app
        .world_mut()
        .entity_mut(state)
        .remove::<bevy_gearbox::Active>();
    fixture.app.update();
    fixture.app.update();

    assert!(
        fixture
            .app
            .world()
            .entity(fixture.player)
            .get::<BlockedTags>()
            .is_none(),
        "状态不再生效后，宿主身上的封禁该被摘掉"
    );
    assert_eq!(
        fixture.player_tries("basic_attack"),
        None,
        "解除控制后攻击类恢复可用"
    );
}
