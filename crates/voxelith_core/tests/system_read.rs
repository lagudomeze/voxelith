//! 在真正的 system 里怎么读一个可修正的量。
//!
//! 要点：**手里已经有组件和关联列表了**——`Query<&Modifiers>` 就是主人的修正实体列表，
//! 直接把它喂给修正查询即可，不需要回头去 `World` 里捞（所以也没有 `resolved(world, owner)`）。

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use voxelith_core::prelude::*;

/// 系统算出什么，就记在这里。
#[derive(Resource, Default, PartialEq, Debug)]
struct Reported(Vec<(f32, f32)>);

fn setup(world: &mut World) {
    let hero = world
        .spawn((
            Strength(10.0),
            Health {
                current: 30.0,
                base_max: 60.0,
            },
        ))
        .id();
    world.spawn((ModifiesOf(hero), ModStrength::flat(5.0)));
    world.spawn((ModifiesOf(hero), ModStrength::ratio(0.5)));
    world.spawn((ModifiesOf(hero), ModHealthMax::flat(20.0)));
}

/// 读力量与血量上限：关联列表来自 `&Modifiers`，修正组件来自各自的查询。
fn report(
    heroes: Query<(&Strength, &Health, &Modifiers)>,
    strength_mods: Query<&Modifier<Strength>>,
    health_mods: Query<&Modifier<Max<Health>>>,
    mut reported: ResMut<Reported>,
) {
    for (strength, health, related) in &heroes {
        let strength_total = strength.resolve(
            related.iter().filter_map(|m| strength_mods.get(m).ok()),
        );
        let max_hp = health.resolve_max(
            related.iter().filter_map(|m| health_mods.get(m).ok()),
        );
        reported.0.push((strength_total, max_hp));
    }
}

#[test]
fn a_system_reads_values_straight_from_its_queries() {
    let mut app = App::new();
    app.add_plugins(TaskPoolPlugin::default());
    app.init_resource::<Reported>();
    app.add_systems(Startup, setup);
    app.add_systems(Update, report);
    app.update();

    let reported = app.world().get_resource::<Reported>().unwrap();
    // 力量：10 + 5 + 10 × 0.5 = 20；血量上限：60 + 20 = 80
    assert_eq!(reported.0, vec![(20.0, 80.0)]);
}
