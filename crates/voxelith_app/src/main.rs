//! 装配层：把模型挂到一个 Bevy App 上。
//!
//! 现在**还没有可跑的东西**（没有渲染、没有输入），所以这里只做"装配 + 自检"：
//! 等配置到货 → 造一个英雄 → 跑一遍重算与派生 → 从**缓存**里读数值 → 走一遍"扣血" →
//! 再放一个火球、看着它走完前摇 → 打印 → 退出。
//!
//! 三件值得看的事：
//!
//! - **归属**：`Affects` / `Modifiers` 是**库自带、唯一的一条关系**（`RulesPlugin` 没有泛型参数，
//!   所以"挂两个实例各算一半"从结构上不可能）。`Modifiers [ … ]` 里写修正，归属由宏自动补；
//!   `linked_spawn` 让"卸下 = 销毁修正实体"自动成立。来源（装备/增益）是修正实体上的标签。
//! - **加载闸门**：配置是异步加载的，而派生要求"配置一定在"。所以**进游戏前等齐**
//!   （真实项目里这一等就是加载界面），system 里不留"还没到货"这条分支。
//! - **时间与行动**：世界只在"玩家已决策、正在结算"时流动（`Time<Virtual>` 暂停开关 +
//!   `Time<Fixed>` 64 步/秒）。一次行动是一个实体，前摇是它的属性，代价在它出现之前就扣掉。

use bevy_app::prelude::*;
use bevy_asset::{AssetPlugin, AssetServer, Assets, LoadState};
use bevy_ecs::prelude::*;
use bevy_scene::prelude::*;
use bevy_scene::ScenePlugin;
use bevy_time::{Fixed, Time, TimeUpdateStrategy, Virtual};
use core::time::Duration;
use voxelith_core::prelude::*;
use voxelith_rules::prelude::*;

/// 收集到点的行动，给自检看。
#[derive(Resource, Default)]
struct Resolved(Vec<ResolveAction>);

fn collect_resolved(mut reader: MessageReader<ResolveAction>, mut out: ResMut<Resolved>) {
    for message in reader.read() {
        out.0.push(*message);
    }
}

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
        // 规则层：重算有效值 + 派生 + 时间与行动。归属是库自带的 Affects / Modifiers。
        RulesPlugin,
    ));
    app.init_resource::<Resolved>();
    app.add_systems(PostUpdate, collect_resolved);
    // 扣费/治疗这类改值的消息与落地系统都由 RulesPlugin 注册，这里不重复注册（重复 = 扣两次）。

    // ① 加载闸门：等这一局要用的配置到货（这一段用真实时间，资产加载不受影响）
    wait_for_config(&mut app, "rules/health_from_constitution.ron");

    // 自检从这里开始走**固定节拍**：每帧正好一个逻辑步。
    // 真实游戏用默认的 Automatic（按墙钟走）；离线自检这样才与机器快慢无关。
    let step = app.world().resource::<Time<Fixed>>().timestep();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(step));

    // ② 摆英雄：属性、资源、装备给的修正、派生规则——全在 bsn! 里
    let hero = app
        .world_mut()
        .spawn_scene(bsn! {
            #Hero
            Strength(10.0)
            Constitution(12.0)
            Health(30.0)
            HealthMax(60.0)   // 当前值与上限是两个组件（别名 = Max<Health>）
            Mana(100.0)       // 放火球要花法力
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

    // ④ 放一个火球：释放 → 前摇 → 到点。世界在玩家已决策时流动（64 步/秒）
    app.world_mut().write_message(BeginAction {
        actor: hero,
        skill: SkillId::Fireball,
        params: ActionParams::Point {
            at: TilePos::new(3, 1, 4),
        },
    });

    for _ in 0..64 {
        app.update();
        if app.world().get::<Actions>(hero).is_some() {
            break;
        }
    }

    // 前摇是**释放那一刻算定的绝对时刻**，之后不再变（不变量 1）
    let action = app
        .world()
        .get::<Actions>(hero)
        .expect("该有正在进行的行动")
        .iter()
        .next()
        .unwrap();
    let resolves_at = app.world().get::<Action>(action).unwrap().resolves_at;

    // 世界按真实时间走：跑到它到点为止（帧数受墙钟影响，只报游戏时刻）
    let mut frames = 0;
    while app.world().resource::<Resolved>().0.is_empty() {
        app.update();
        frames += 1;
        assert!(frames < 10_000, "火球该到点了");
    }

    report_action(&app, hero, resolves_at, frames);
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

/// 自检：火球走完前摇那一套。
fn report_action(app: &App, hero: Entity, resolves_at: Duration, frames: u32) {
    let world = app.world();
    let clock = world.resource::<Time<Fixed>>();
    let resolved = &world.resource::<Resolved>().0[0];
    let mana = world.get::<Mana>(hero).unwrap().raw();
    let step = clock.timestep();

    println!();
    println!("火球（落在 (3, 1, 4)）");
    println!(
        "  前摇 26 步（0.4 秒 ÷ {step:?} 四舍五入），释放时算定到 {resolves_at:?}"
    );
    println!(
        "  到点时游戏时刻 {:?}（时间轴上第 {} 步），这一局走了 {frames} 帧（自检节拍 1 步/帧）",
        clock.elapsed(),
        resolves_at.as_nanos() / step.as_nanos().max(1),
    );
    println!(
        "  到点的技能 {:?}；行动实体已删，法力 {mana}（GP-26：先扣不退）",
        resolved.skill
    );
    println!(
        "  世界时钟：{}　（Freeze 三个触发都没挂 → 世界在走）",
        if world.resource::<Time<Virtual>>().is_paused() {
            "已暂停"
        } else {
            "在走"
        }
    );
}
