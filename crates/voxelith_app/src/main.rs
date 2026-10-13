//! 装配层：把模型挂到一个 Bevy App 上。
//!
//! 现在**还没有可跑的东西**（没有渲染、没有输入、没有时间推进），所以这里只做
//! “装配 + 自检”：造一个英雄、跑一遍重算、从**缓存**里读出最终数值、走一遍“扣血”，然后退出。
//!
//! 这个文件同时是**挂法**的示例：`Equipment` / `EquippedOn` 是应用侧自己定的 relationship
//! （core 与 rules 都不认识它），它靠 [`RulesPlugin::<Equipment>`] 那个泛型参数交给规则层。
//! `linked_spawn` 让“卸下 = 销毁修正实体”自动成立，而且因为它是 `RelationshipTarget`，
//! `bsn!` 里能直接写成列表。

use bevy_app::prelude::*;
use bevy_asset::AssetPlugin;
use bevy_ecs::prelude::*;
use bevy_scene::prelude::*;
use bevy_scene::ScenePlugin;
use voxelith_core::prelude::*;
use voxelith_rules::prelude::*;

/// 这条修正挂在哪个主人身上。
#[derive(Component, FromTemplate)]
#[relationship(relationship_target = Equipment)]
struct EquippedOn(Entity);

/// 主人身上自动维护的修正列表。
#[derive(Component, Default)]
#[relationship_target(relationship = EquippedOn, linked_spawn)]
struct Equipment(Vec<Entity>);

/// 自检里那个英雄。
#[derive(Resource)]
struct Hero(Entity);

fn main() {
    let mut app = App::new();
    app.add_plugins((
        TaskPoolPlugin::default(),
        AssetPlugin::default(),
        ScenePlugin,
        // 规则层：重算有效值 + 派生。挂法（Equipment）由这里说。
        RulesPlugin::<Equipment>::default(),
    ));
    // 当前值的唯一写入通路（`SY-17`：治疗与扣血同一条路）
    app.add_message::<Adjust<Health>>()
        .add_systems(Startup, setup_hero)
        .add_systems(Update, apply_adjustments::<Health>)
        .add_systems(Last, report);
    app.update();
}

/// 一个英雄：两项属性、一条资源、三条修正——全部写在 `bsn!` 里。
fn setup_hero(world: &mut World) {
    let hero = world
        .spawn_scene(bsn! {
            #Hero
            Strength(10.0)
            Constitution(12.0)
            Health { current: 30.0, base_max: 60.0 }
            Equipment [
                // 一条修正同时带平加和比例
                ModStrength { flat: 3.0, ratio: 0.5 },
                ModStrength { flat: -2.0 },
                ModHealthMax { flat: 20.0 },
            ]
        })
        .expect("装配英雄失败")
        .id();

    // 挨 7 点伤害：**走消息**，不直接改组件
    world.write_message(Adjust::<Health>::lose(hero, 7.0, None));
    world.insert_resource(Hero(hero));
}

/// 自检：读的全是**缓存**（`Total`），一个挂法都不用认识。
fn report(
    hero: Res<Hero>,
    attributes: Query<(&Total<Strength>, &Total<Constitution>)>,
    health: Query<(&Health, &Total<Max<Health>>)>,
) {
    let Ok((strength, constitution)) = attributes.get(hero.0) else {
        return;
    };
    let Ok((health, max_hp)) = health.get(hero.0) else {
        return;
    };

    println!("英雄 {:?}", hero.0);
    println!(
        "  力量 {}　基础 10 ＋3 －2 ＋ 50% 比例（＋3 与 ＋50% 在同一条修正上）",
        strength.value
    );
    println!("  体质 {}　无修正", constitution.value);
    println!(
        "  血量 {} / {}　上限 = 基础 60 ＋ 20 平加；当前值不走修正（挨了 7 点，走消息）",
        health.current(),
        max_hp.value
    );
}
