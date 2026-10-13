//! 在真正的 system 里怎么读一个可修正的量。
//!
//! 新形状下分成**两半**，这个文件把分界线演示清楚：
//!
//! - **重算的那一半**（这里手写；真实项目里是 `voxelith_rules::recompute`）：它知道“修正怎么挂”，
//!   把 `基础值 + Σ贡献(基础值)` 算出来写进 [`Total`] 缓存；
//! - **读的那一半**：只看 `Total`，**不认识任何挂法**。
//!
//! 这里定义两套挂法（装备槽 `Gear` 与增益列表 `Buffs`）只是为了说明：**喂哪条链接是重算系统的事**。
//!
//! 顺带一个后果：缓存里只有**一个数**。要“面板值（只看装备）”和“结算值（装备＋增益）”两个数并存，
//! 那是**两个量**（两份缓存），不是同一个量的两种读法。

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use voxelith_core::prelude::*;

// ==================== 应用侧自己定的两套挂法 ====================

/// 装备带来的修正。
#[derive(Component, FromTemplate)]
#[relationship(relationship_target = Gear)]
struct FromGear(Entity);

#[derive(Component, Default)]
#[relationship_target(relationship = FromGear, linked_spawn)]
struct Gear(Vec<Entity>);

/// 增益带来的修正。
#[derive(Component, FromTemplate)]
#[relationship(relationship_target = Buffs)]
struct FromBuff(Entity);

#[derive(Component, Default)]
#[relationship_target(relationship = FromBuff, linked_spawn)]
struct Buffs(Vec<Entity>);

// ==================== 公式的求和（真实项目里在 rules 层）====================

fn sum<Mark>(base: f32, modifiers: impl Iterator<Item = Modifier<Mark>>) -> f32 {
    base + modifiers.map(|m| m.contribution(base)).sum::<f32>()
}

// ==================== 重算那一半：知道挂法 ====================

#[allow(clippy::type_complexity, reason = "查询就是要写这么长")]
fn recompute_strength(
    mut owners: Query<(&Strength, &Gear, &Buffs, &mut Total<Strength>)>,
    modifiers: Query<&Modifier<Strength>>,
) {
    for (strength, gear, buffs, mut total) in &mut owners {
        let base = strength.base();
        let value = sum(
            base,
            gear.0
                .iter()
                .chain(buffs.0.iter())
                .filter_map(|m| modifiers.get(*m).ok())
                .cloned(),
        );
        if total.value != value {
            total.value = value;
        }
    }
}

#[allow(clippy::type_complexity, reason = "同上")]
fn recompute_max_hp(
    mut owners: Query<(&Health, &Gear, &mut Total<Max<Health>>)>,
    modifiers: Query<&Modifier<Max<Health>>>,
) {
    for (health, gear, mut total) in &mut owners {
        let base = health.base_max();
        let value = sum(
            base,
            gear.0
                .iter()
                .filter_map(|m| modifiers.get(*m).ok())
                .cloned(),
        );
        if total.value != value {
            total.value = value;
        }
    }
}

// ==================== 读的那一半：只看缓存 ====================

#[derive(Resource, Default, PartialEq, Debug)]
struct Reported {
    strength: f32,
    max_hp: f32,
    current_hp: f32,
}

#[allow(clippy::type_complexity, reason = "查询就是要写这么长")]
fn report(
    owners: Query<(&Total<Strength>, &Total<Max<Health>>, &Health)>,
    mut reported: ResMut<Reported>,
) {
    for (strength, max_hp, health) in &owners {
        reported.strength = strength.value;
        reported.max_hp = max_hp.value;
        reported.current_hp = health.current();
    }
}

#[test]
fn a_system_reads_values_straight_from_its_queries() {
    let mut app = App::new();
    app.add_plugins(TaskPoolPlugin::default());
    app.init_resource::<Reported>();
    app.add_systems(Startup, setup);
    // 重算必须排在读者之前
    app.add_systems(Update, (recompute_strength, recompute_max_hp, report).chain());
    app.update();

    let reported = app.world().get_resource::<Reported>().unwrap();
    assert_eq!(
        *reported,
        Reported {
            strength: 17.0,  // 10 + 5（装备）+ 2（增益）
            max_hp: 80.0,    // 60 + 20（装备）
            current_hp: 30.0 // 当前值是状态，不走修正
        }
    );
}

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

    // 装备：+5 力量、+20 血量上限
    world.spawn((FromGear(hero), ModStrength::flat(5.0)));
    world.spawn((FromGear(hero), ModHealthMax::flat(20.0)));
    // 增益：+2 力量
    world.spawn((FromBuff(hero), ModStrength::flat(2.0)));
}
