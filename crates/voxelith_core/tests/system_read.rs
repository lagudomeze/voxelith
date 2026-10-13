//! 在真正的 system 里怎么读一个可修正的量。
//!
//! 分**两半**，这个文件把分界线演示清楚：
//!
//! - **重算的那一半**（这里手写；真实项目里是 `voxelith_rules::recompute`）：它读**库自带的**
//!   归属名单 [`Modifiers`]，把 `raw + Σ贡献(raw)` 算出来写进 [`Total`] 缓存；
//! - **读的那一半**：只看 `Total`，**不认识任何归属**。
//!
//! 归属只有一条（[`Affects`] / [`Modifiers`]，库自带、无泛型）。**来源是标签**：这个文件给
//! 修正实体挂上 [`FromGear`] / [`FromBuff`]，于是"装备给了多少"可以按标签单独筛出来——
//! 而不是再拉一条归属关系（那会让两个重算系统互相覆盖）。

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use voxelith_core::prelude::*;

// ==================== 来源：标签，不是关系 ====================

/// 这条修正来自装备。
#[derive(Component)]
struct FromGear;

/// 这条修正来自增益。（真实项目里这里会带上"是哪个增益给的"，方便一起清理。）
#[derive(Component)]
struct FromBuff;

// ==================== 公式的求和（真实项目里在 rules 层）====================

fn sum<Mark>(raw: f32, modifiers: impl Iterator<Item = Modifier<Mark>>) -> f32 {
    raw + modifiers.map(|m| m.contribution(raw)).sum::<f32>()
}

// ==================== 重算那一半：读内置名单 ====================

#[allow(clippy::type_complexity, reason = "测试里直接写长查询，看得更直白")]
fn recompute_strength(
    mut owners: Query<(Entity, &Strength, Option<&Modifiers>, &mut Total<Strength>)>,
    modifiers: Query<&Modifier<Strength>>,
) {
    for (owner, strength, list, mut total) in &mut owners {
        let raw = strength.raw();
        let value = sum(
            raw,
            list.into_iter()
                .flat_map(|l| l.iter())
                .filter_map(|e| modifiers.get(e).ok())
                .cloned(),
        );
        let _ = owner;
        if total.value != value {
            total.value = value;
        }
    }
}

#[allow(clippy::type_complexity, reason = "同上")]
fn recompute_max_hp(
    mut owners: Query<(&Max<Health>, Option<&Modifiers>, &mut Total<Max<Health>>)>,
    modifiers: Query<&Modifier<Max<Health>>>,
) {
    for (max, list, mut total) in &mut owners {
        let raw = max.raw();
        let value = sum(
            raw,
            list.into_iter()
                .flat_map(|l| l.iter())
                .filter_map(|e| modifiers.get(e).ok())
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
    /// 只算装备那部分（按标签筛出来的）。
    gear_only: f32,
}

#[allow(clippy::type_complexity, reason = "同上")]
fn report(
    owners: Query<(Entity, &Strength, &Total<Strength>, &Total<Max<Health>>, &Health)>,
    lists: Query<&Modifiers>,
    strength_mods: Query<&Modifier<Strength>>,
    gear_tags: Query<(), With<FromGear>>,
    mut reported: ResMut<Reported>,
) {
    for (owner, base, strength, max_hp, health) in &owners {
        reported.strength = strength.value;
        reported.max_hp = max_hp.value;
        reported.current_hp = health.raw();

        // 来源标签的用处：只想看装备那份，按标签筛就行
        let raw = base.raw();   // 想按来源分账，就得回到基础值自己算（那是重算侧的活）
        reported.gear_only = lists
            .get(owner)
            .map(|l| {
                sum(
                    raw,
                    l.iter()
                        .filter(|e| gear_tags.contains(*e))
                        .filter_map(|e| strength_mods.get(e).ok())
                        .cloned(),
                )
            })
            .unwrap_or(raw);
    }
}

fn setup(world: &mut World) {
    let hero = world.spawn((Strength(10.0), Health(30.0))).id();
    world.get_mut::<Max<Health>>(hero).unwrap().0 = 60.0;

    // 装备：+5 力量、+20 血量上限
    world.spawn((Affects(hero), FromGear, ModStrength::flat(5.0)));
    world.spawn((Affects(hero), FromGear, ModHealthMax::flat(20.0)));
    // 增益：+2 力量
    world.spawn((Affects(hero), FromBuff, ModStrength::flat(2.0)));
}

#[test]
fn a_system_reads_values_straight_from_its_queries() {
    let mut app = App::new();
    app.add_plugins(TaskPoolPlugin::default());
    app.init_resource::<Reported>();
    app.add_systems(Startup, setup);
    // 重算必须排在读者之前
    app.add_systems(
        Update,
        (recompute_strength, recompute_max_hp, report).chain(),
    );
    app.update();

    let reported = app.world().get_resource::<Reported>().unwrap();
    assert_eq!(
        *reported,
        Reported {
            strength: 17.0,        // 10 + 5（装备）+ 2（增益）
            max_hp: 80.0,          // 60 + 20（装备）
            current_hp: 30.0,      // 当前值是状态，不走修正
            gear_only: 15.0,       // 只看装备那份：10 + 5
        }
    );
}
