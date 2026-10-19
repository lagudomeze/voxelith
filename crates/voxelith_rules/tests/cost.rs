//! 付费：够就全扣 ✓ 不够就**一条都不扣** ✓ 同一帧两条请求抢同一份资源只成全一条 ✓

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use voxelith_core::prelude::*;
use voxelith_rules::prelude::*;

/// 这次要花多少：法力 10 + 精力 5。
fn bill() -> Costs {
    Costs {
        mana: 10.0,
        stamina: 5.0,
    }
}

#[derive(Resource)]
struct Actor(Entity);

#[derive(Resource, Default)]
struct Outcome(Vec<Result<(), RejectReason>>);

/// 发一次请求。
fn one(
    actor: Res<Actor>,
    mut mana: Query<&mut Mana>,
    mut stamina: Query<&mut Stamina>,
    mut out: ResMut<Outcome>,
) {
    out.0
        .push(try_pay(actor.0, &bill(), &mut mana, &mut stamina));
}

/// 同一帧发两次（两条请求抢同一份资源）。
fn two(
    actor: Res<Actor>,
    mut mana: Query<&mut Mana>,
    mut stamina: Query<&mut Stamina>,
    mut out: ResMut<Outcome>,
) {
    for _ in 0..2 {
        out.0
            .push(try_pay(actor.0, &bill(), &mut mana, &mut stamina));
    }
}

fn app() -> App {
    let mut app = App::new();
    app.add_plugins(TaskPoolPlugin::default());
    app.init_resource::<Outcome>();
    app
}

fn with_actor(app: &mut App, components: impl Bundle) -> Entity {
    let actor = app.world_mut().spawn(components).id();
    app.insert_resource(Actor(actor));
    actor
}

fn outcome(app: &App) -> &Vec<Result<(), RejectReason>> {
    &app.world().resource::<Outcome>().0
}

fn mana(app: &App, actor: Entity) -> f32 {
    app.world().get::<Mana>(actor).map(|v| v.raw()).unwrap_or(f32::NAN)
}

fn stamina(app: &App, actor: Entity) -> f32 {
    app.world()
        .get::<Stamina>(actor)
        .map(|v| v.raw())
        .unwrap_or(f32::NAN)
}

/// 都够 → 全扣。
#[test]
fn enough_pays_all() {
    let mut app = app();
    let actor = with_actor(&mut app, (Mana(30.0), Stamina(30.0)));
    app.add_systems(Update, one);

    app.update();

    assert_eq!(outcome(&app), &vec![Ok(())]);
    assert_eq!(mana(&app, actor), 20.0);
    assert_eq!(stamina(&app, actor), 25.0);
}

/// 有一条不够 → **一条都不扣**（跨资源全不或全）。
#[test]
fn one_short_means_nothing_is_charged() {
    let mut app = app();
    // 法力够、精力不够
    let actor = with_actor(&mut app, (Mana(30.0), Stamina(1.0)));
    app.add_systems(Update, one);

    app.update();

    assert_eq!(
        outcome(&app),
        &vec![Err(RejectReason::NotEnough(ResourceKind::Stamina))]
    );
    assert_eq!(mana(&app, actor), 30.0, "法力一点没动");
    assert_eq!(stamina(&app, actor), 1.0);
}

/// 同一帧两条请求抢同一份资源：**只成全一条**，另一条被拒绝。
#[test]
fn same_frame_contention_only_one_wins() {
    let mut app = app();
    // 法力只够一次（15 < 10 + 10）；精力够两次
    let actor = with_actor(&mut app, (Mana(15.0), Stamina(30.0)));
    app.add_systems(Update, two);

    app.update();

    assert_eq!(
        outcome(&app),
        &vec![Ok(()), Err(RejectReason::NotEnough(ResourceKind::Mana))]
    );
    assert_eq!(mana(&app, actor), 5.0, "只扣了一次");
    assert_eq!(stamina(&app, actor), 25.0, "被拒绝的那条也没动精力");
}

/// 没有这条量的实体 → 视作**不够**，不 panic。
#[test]
fn missing_resource_counts_as_not_enough() {
    let mut app = app();
    let actor = with_actor(&mut app, Stamina(30.0)); // 没有 Mana
    app.add_systems(Update, one);

    app.update();

    assert_eq!(
        outcome(&app),
        &vec![Err(RejectReason::NotEnough(ResourceKind::Mana))]
    );
    assert_eq!(stamina(&app, actor), 30.0, "一条都不扣");
}
