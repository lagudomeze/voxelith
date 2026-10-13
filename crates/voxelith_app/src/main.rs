//! 装配层：把模型挂到一个 Bevy App 上。
//!
//! 现在**还没有可跑的东西**（没有渲染、没有输入、没有时间推进），所以这里只做
//! “装配 + 自检”：等配置到货 → 造一个英雄 → 跑一遍重算与派生 → 从**缓存**里读数值 →
//! 走一遍“扣血” → 打印 → 退出。
//!
//! 两件值得看的事：
//!
//! - **归属**：`Affects` / `Modifiers` 是**库自带、唯一的一条关系**（`RulesPlugin` 没有泛型参数，
//!   所以"挂两个实例各算一半"从结构上不可能）。`Modifiers [ … ]` 里写修正，归属由宏自动补；
//!   `linked_spawn` 让“卸下 = 销毁修正实体”自动成立。来源（装备/增益）是修正实体上的标签。
//! - **加载闸门**：配置是异步加载的，而派生要求“配置一定在”。所以**进游戏前等齐**
//!   （真实项目里这一等就是加载界面），system 里不留“还没到货”这条分支。

use bevy_app::prelude::*;
use bevy_asset::{AssetPlugin, AssetServer, Assets, LoadState};
use bevy_ecs::prelude::*;
use bevy_scene::prelude::*;
use bevy_scene::ScenePlugin;
use voxelith_core::prelude::*;
use voxelith_rules::prelude::*;



fn main() {
    let mut app = App::new();
    app.add_plugins((
        TaskPoolPlugin::default(),
        // 内容与代码分开放：资产根是仓库根的 assets/
        AssetPlugin {
            file_path: format!("{}/../../assets", env!("CARGO_MANIFEST_DIR")),
            ..Default::default()
        },
        ScenePlugin,
        // 规则层：重算有效值 + 派生。归属是库自带的 Affects / Modifiers。
        RulesPlugin,
    ));
    // 当前值的唯一写入通路（`SY-17`：治疗与扣血同一条路）
    app.add_message::<Adjust<Health>>();
    app.add_systems(Update, apply_adjustments::<Health>);

    // ① 加载闸门：等这一局要用的配置到货
    wait_for_config(&mut app, "rules/health_from_constitution.ron");

    // ② 摆英雄：属性、资源、装备给的修正、派生规则——全在 bsn! 里
    let hero = app
        .world_mut()
        .spawn_scene(bsn! {
            #Hero
            Strength(10.0)
            Constitution(12.0)
            Health(30.0)
            HealthMax(60.0)   // 当前值与上限是两个组件（别名 = Max<Health>）
            Modifiers [
                // 一条修正同时带平加和比例
                ModStrength { flat: 3.0, ratio: 0.5 },
                ModStrength { flat: -2.0 },
                ModHealthMax { flat: 20.0 },
            ]
            // 派生：血量上限由体质来（参数在配置里，这里只写用哪份）
            HealthMaxFromConstitution { config: "rules/health_from_constitution.ron" }
        })
        .expect("装配英雄失败")
        .id();

    // 挨 7 点伤害：**走消息**，不直接改组件
    app.world_mut()
        .write_message(Adjust::<Health>::lose(hero, 7.0, None));

    // ③ 一帧：属性重算 → 派生写修正 → 资源上限重算
    app.update();

    report(app.world(), hero);
}

/// 等一份配置加载好。**失败要吵**，别静默空转。
///
/// 路径是 `&'static str`：`AssetServer::load` 要 `'static` 的路径。
fn wait_for_config(app: &mut App, path: &'static str) {
    let handle = app
        .world()
        .resource::<AssetServer>()
        .load::<RuleConfig>(path);

    for _ in 0..600 {
        if app
            .world()
            .resource::<Assets<RuleConfig>>()
            .get(&handle)
            .is_some()
        {
            return;
        }
        match app.world().resource::<AssetServer>().load_state(&handle) {
            LoadState::Failed(err) => panic!("配置加载失败：{err}"),
            _ => app.update(),
        }
    }
    panic!("配置等了很久还没到货：{path}");
}

/// 自检：读的全是**缓存**（`Total`），一个挂法都不用认识。
fn report(world: &World, hero: Entity) {
    let strength = world.get::<Total<Strength>>(hero).unwrap().value;
    let constitution = world.get::<Total<Constitution>>(hero).unwrap().value;
    let health = world.get::<Health>(hero).unwrap();
    let max_hp = world.get::<Total<Max<Health>>>(hero).unwrap().value;
    let derived = world.get::<Modifier<Max<Health>>>(hero).unwrap().flat;

    println!("英雄 {hero:?}");
    println!("  力量 {strength}　基础 10 ＋3 －2 ＋ 50% 比例（＋3 与 ＋50% 在同一条修正上）");
    println!("  体质 {constitution}　无修正");
    println!(
        "  血量上限 {max_hp}　基础 60 ＋ 装备 20 ＋ 派生 {derived}（体质 {constitution} × 每点 5，参数在 .ron 里）"
    );
    println!(
        "  当前血量 {}　不算修正；挨了 7 点，走 Adjust 消息",
        health.raw()
    );
}
