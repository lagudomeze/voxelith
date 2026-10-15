//! 行动：释放 → 前摇 → 到点；以及取消、代价、冻结。
//!
//! 每帧喂"刚好一步"的假时间（`ManualDuration(1/64 秒)`），所以每一帧跑几个 tick 是确定的 ✓
//! 测试里所有断言都能精确到步 ✓

use bevy_app::prelude::*;
use bevy_asset::AssetPlugin;
use bevy_ecs::prelude::*;
use bevy_time::{Fixed, Time, TimeUpdateStrategy};
use core::time::Duration;
use voxelith_core::prelude::*;
use voxelith_rules::prelude::*;

/// 一个逻辑步 = 1/64 秒。
const STEP: Duration = Duration::from_micros(15625);

#[derive(Resource, Default)]
struct Resolved(Vec<ResolveAction>);

fn collect_resolved(mut reader: MessageReader<ResolveAction>, mut out: ResMut<Resolved>) {
    for message in reader.read() {
        out.0.push(*message);
    }
}

fn test_app() -> App {
    let mut app = App::new();
    app.add_plugins((
        TaskPoolPlugin::default(),
        AssetPlugin::default(),
        RulesPlugin,
    ));
    app.init_resource::<Resolved>();
    app.add_systems(PostUpdate, collect_resolved);
    app.insert_resource(TimeUpdateStrategy::ManualDuration(STEP));
    app
}

/// 一个能放火球的角色（法力由 `#[require(Max<Mana>)]` 一起带上）。
///
/// 顺带空跑一帧：**第一帧的 `delta` 是 0**，固定步长不跑，行动系统也就不会执行 ✓
/// （真实游戏里第一条输入远在这之后，不受影响 ✓）
fn a_hero(app: &mut App) -> Entity {
    let hero = app.world_mut().spawn(Mana(100.0)).id();
    app.update();
    hero
}

const AT: TilePos = TilePos::new(3, 0, 4);

fn cast_at(hero: Entity, at: TilePos) -> BeginAction {
    BeginAction {
        actor: hero,
        skill: SkillId::Fireball,
        params: ActionParams::Point { at },
    }
}

fn only_action(app: &App, hero: Entity) -> Entity {
    app.world()
        .get::<Actions>(hero)
        .expect("身上应该有正在进行的行动")
        .iter()
        .next()
        .expect("名单里应该有一条")
}

/// 释放：生成行动实体，参数、归属、强度都对，前摇对齐到整步。
#[test]
fn releasing_creates_the_action_entity() {
    let mut app = test_app();
    let hero = a_hero(&mut app);

    app.world_mut().write_message(cast_at(hero, AT));
    app.update();

    let world = app.world();
    let casting = world.get::<Actions>(hero).expect("应该有正在进行的行动");
    assert_eq!(casting.len(), 1, "一次只做一件事（GP-03）");

    let action = casting.iter().next().unwrap();
    assert_eq!(world.get::<ActionOf>(action).unwrap().0, hero, "归属写对了");

    let state = world.get::<Action>(action).unwrap();
    assert_eq!(state.skill, SkillId::Fireball);
    assert_eq!(state.params, ActionParams::Point { at: AT });
    assert_eq!(world.get::<Potency>(action).unwrap().0, 6.0);
    assert_eq!(
        state.resolves_at.as_nanos() % STEP.as_nanos(),
        0,
        "前摇要对齐到整步"
    );
    assert!(world.get::<Total<Potency>>(action).is_some(), "强度带缓存");
}

/// 前摇时长 = 0.4 秒 = 25.6 步 → **四舍五入到 26 步**。
#[test]
fn windup_is_rounded_to_a_whole_step() {
    let mut app = test_app();
    let hero = a_hero(&mut app);
    app.update();

    let before = app.world().resource::<Time<Fixed>>().elapsed();
    app.world_mut().write_message(cast_at(hero, AT));
    app.update();
    let after = app.world().resource::<Time<Fixed>>().elapsed();

    let resolves_at = app.world().get::<Action>(only_action(&app, hero)).unwrap().resolves_at;
    assert_eq!(
        resolves_at - after,
        STEP * 26,
        "0.4 秒 = 25.6 步 → 26 步（这一步在 begin 时算定）"
    );
    assert!(after >= before, "时间轴只往前走");
}

/// 参数形状不对 → 释放那一刻就炸（不静默）。
#[test]
#[should_panic(expected = "形状的参数")]
fn wrong_param_shape_panics() {
    let mut app = test_app();
    let hero = a_hero(&mut app);

    app.world_mut().write_message(BeginAction {
        actor: hero,
        skill: SkillId::Fireball,
        params: ActionParams::None,
    });
    app.update();
}

/// 落新技能会取消当前前摇；**同一帧里连发两次也只留一条**。
#[test]
fn a_new_action_cancels_the_old_one() {
    let mut app = test_app();
    let hero = a_hero(&mut app);

    app.world_mut().write_message(cast_at(hero, AT));
    app.update();
    let first = only_action(&app, hero);

    // 同一帧发两次：命令还没落地，靠"本轮局部表"也得分得清
    app.world_mut().write_message(cast_at(hero, TilePos::new(9, 0, 9)));
    app.world_mut().write_message(cast_at(hero, AT));
    app.update();

    let casting = app.world().get::<Actions>(hero).unwrap();
    assert_eq!(casting.len(), 1, "一次只做一件事（GP-03）");
    assert!(app.world().get_entity(first).is_err(), "旧的那次被删了");
}

/// 代价在行动出现之前就扣，且**已扣不退**（`GP-26` / `SY-43`）。
#[test]
fn cost_is_paid_and_never_refunded() {
    let mut app = test_app();
    let hero = a_hero(&mut app);
    app.update();
    assert_eq!(app.world().get::<Mana>(hero).unwrap().raw(), 100.0);

    app.world_mut().write_message(cast_at(hero, AT));
    app.update();
    app.update(); // 扣费消息落地
    assert_eq!(app.world().get::<Mana>(hero).unwrap().raw(), 90.0, "先扣掉");

    // 取消那次行动：不退
    let action = only_action(&app, hero);
    app.world_mut().entity_mut(action).despawn();
    app.update();
    assert_eq!(
        app.world().get::<Mana>(hero).unwrap().raw(),
        90.0,
        "已扣不退（SY-43）"
    );
}

/// 走到前摇结束那一步正好生效，行动实体消失，名单也一起消失。
#[test]
fn resolves_exactly_on_the_windup_tick() {
    let mut app = test_app();
    let hero = a_hero(&mut app);
    app.update();
    app.world_mut().write_message(cast_at(hero, AT));
    app.update();

    let action = only_action(&app, hero);
    let resolves_at = app.world().get::<Action>(action).unwrap().resolves_at;

    let mut frames = 0;
    while app.world().resource::<Resolved>().0.is_empty() {
        app.update();
        frames += 1;
        assert!(frames < 200, "该到点了");
    }

    assert_eq!(
        app.world().resource::<Time<Fixed>>().elapsed(),
        resolves_at,
        "生效判定与时间轴对齐（不是那一步之后）"
    );

    let record = app.world().resource::<Resolved>().0[0];
    assert_eq!(record.actor, hero);
    assert_eq!(record.action, action, "消息里带的就是那次行动（此刻已删）");
    assert_eq!(record.skill, SkillId::Fireball);
    assert_eq!(record.params, ActionParams::Point { at: AT });
    assert!(
        app.world().get::<Actions>(hero).is_none(),
        "行动实体没了，空名单也一起消失"
    );
}



/// 冻结时世界一步都不走（`SY-57`：状态时长、每跳、耐久恢复全停）。
#[test]
fn frozen_world_does_not_advance() {
    let mut app = test_app();
    let hero = a_hero(&mut app);
    app.update();
    app.world_mut().write_message(cast_at(hero, AT));
    app.update();

    app.world_mut().resource_mut::<Freeze>().manual = true;
    for _ in 0..60 {
        app.update();
    }
    assert!(
        app.world().resource::<Resolved>().0.is_empty(),
        "冻结时不该到点"
    );
    assert!(app.world().get::<Actions>(hero).is_some(), "行动还悬着");

    // 解冻后接着走，时间轴从停下的地方继续
    app.world_mut().resource_mut::<Freeze>().manual = false;
    let mut frames = 0;
    while app.world().resource::<Resolved>().0.is_empty() {
        app.update();
        frames += 1;
        assert!(frames < 200, "解冻后该到点");
    }
}

/// 角色没了，行动跟着没（`linked_spawn`）。
#[test]
fn despawning_the_actor_takes_the_action_along() {
    let mut app = test_app();
    let hero = a_hero(&mut app);
    app.world_mut().write_message(cast_at(hero, AT));
    app.update();

    let action = only_action(&app, hero);
    app.world_mut().entity_mut(hero).despawn();
    app.update();

    assert!(app.world().get_entity(action).is_err());
}
