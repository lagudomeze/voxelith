//! L2 内容管线测试：五份 `.ron` **真的能加载**，并生成可战斗的实体。
//!
//! 这些断言的价值在于：内容配错（引用了没登记的池 / 属性 / 状态）在**启动期**就会炸，
//! 而不是等到某次战斗里静默失效。用无头 App 跑，不需要窗口。

use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use bevy::time::TimePlugin;
use voxelith_axiom::atoms::actor::{
    ActorState, ActorTag, ActorTags, Faction, Monster, Player, Resources, Stats,
};
use voxelith_axiom::behaviors::combat::CombatPlugin;
use voxelith_axiom::behaviors::content::{SkillCatalog, StatusCatalog, Vocab};
use voxelith_axiom::behaviors::phase::{AvailableSkills, CombatPhase};
use voxelith_prime::content::ContentPlugin;

/// 组装一个无头 App：内容层 + 机制层（不含渲染 / 窗口）。
///
/// `RawContent` 在真程序里由 `main` 解析好再 `insert_resource`；测试里补上同一步，
/// 否则 `setup_content_resources` 会因为没有输入而 panic。
fn headless_app() -> App {
    let mut app = App::new();
    // CombatPlugin 内部已经装配了 ActionPlugin 等子域。
    app.add_plugins((TimePlugin, StatesPlugin, CombatPlugin, ContentPlugin));
    app.insert_resource(voxelith_prime::content::parse_raw().expect("`.ron` 内容该配好"));
    app
}

#[test]
fn content_files_load_and_install_the_vocabulary() {
    let mut app = headless_app();
    // Startup 系统在第一次 update 时跑：它会加载五份 RON 并注入 Resource。
    app.update();

    let vocab = app.world().get_resource::<Vocab>().expect("词汇表已注入");
    assert!(vocab.resource("hp").is_ok(), "vocabulary.ron 里有 hp");
    assert!(vocab.resource("reaction").is_ok(), "反制技能要用 reaction");
    assert!(vocab.stat("strength").is_ok());
    assert!(vocab.status("guarding").is_ok());

    assert!(
        app.world().get_resource::<SkillCatalog>().is_some(),
        "技能目录已注入"
    );
    assert!(
        app.world().get_resource::<StatusCatalog>().is_some(),
        "状态目录已注入"
    );
}

#[test]
fn player_is_spawned_from_content_not_hardcoded() {
    // 这条是"加内容不改代码"的直接检验：玩家的**属性数值**和**特性**都来自
    // `players.ron`。曾经这些值硬编码在 `spawn.rs` 里，改平衡要动 Rust。
    let mut app = headless_app();
    app.update();

    let (pools, stats, traits) = {
        let mut players = app
            .world_mut()
            .query_filtered::<(&Resources, &Stats, &ActorTags), With<Player>>();
        let row = players.iter(app.world()).next().expect("有玩家");
        (row.0.clone(), row.1.clone(), row.2.clone())
    };

    // `players.ron`：hp 100 / mana 50 / action 1 / reaction 3；力量 12 / 护甲 4 / 敏捷 8。
    let hp = app.world().resource::<Vocab>().resource("hp").expect("hp");
    assert_eq!(pools.max(hp), 100.0, "池上限来自 players.ron");
    let strength = app
        .world()
        .resource::<Vocab>()
        .stat("strength")
        .expect("力量");
    assert_eq!(
        stats.get(strength),
        12.0,
        "属性数值来自 players.ron（不是 Rust 里的字面量）"
    );
    assert_eq!(traits.0, vec![], "玩家没有特性标签（内容说了算）");
}

#[test]
fn demo_actors_are_spawned_with_pools_and_stats() {
    let mut app = headless_app();
    app.update();

    // 先在世界里查一遍（`query` 需要可变借用，所以分两段写）。
    let (player_pools, player_stats, monster_pools) = {
        let mut players = app
            .world_mut()
            .query_filtered::<(&Resources, &Stats), With<Player>>();
        let (pools, stats) = players
            .iter(app.world())
            .next()
            .map(|(pools, stats)| (pools.clone(), stats.clone()))
            .expect("有玩家");

        let mut monsters = app
            .world_mut()
            .query_filtered::<&Resources, With<Monster>>();
        let monster_pools = monsters.iter(app.world()).next().cloned().expect("有怪物");
        (pools, stats, monster_pools)
    };

    assert!(
        player_pools.max(voxelith_axiom::behaviors::content::ResourceId(0)) > 0.0,
        "玩家的池上限来自 vocabulary.ron"
    );
    assert!(
        player_stats.get(voxelith_axiom::behaviors::content::StatId(0)) > 0.0,
        "玩家属性来自内容层"
    );
    assert!(
        monster_pools.max(voxelith_axiom::behaviors::content::ResourceId(0)) > 0.0,
        "怪物血量来自 monsters.ron"
    );
}

#[test]
fn content_assigns_faction_and_traits_separately() {
    let mut app = headless_app();
    app.update();

    // 阵营：PC 是玩家侧，哥布林是怪物侧。
    // `Name` 现在是**显示名**（`players.ron` 的 `name`），HUD 直接拿它当标题。
    let rows: Vec<(Faction, String)> = {
        let mut factions = app.world_mut().query::<(&Faction, Option<&Name>)>();
        factions
            .iter(app.world())
            .map(|(faction, name)| {
                (
                    *faction,
                    name.map_or_else(String::new, |name| name.to_string()),
                )
            })
            .collect()
    };
    assert!(
        rows.contains(&(Faction::Player, "冒险者".to_owned())),
        "PC 是玩家阵营：{rows:?}"
    );
    assert!(
        rows.contains(&(Faction::Monster, "哥布林".to_owned())),
        "哥布林是怪物阵营：{rows:?}"
    );

    // 特性与阵营是两条独立的轴：哥布林是"怪物阵营"**且**"野兽"。
    let tags = {
        let mut traits = app
            .world_mut()
            .query_filtered::<&ActorTags, With<Monster>>();
        traits
            .iter(app.world())
            .next()
            .cloned()
            .expect("怪物有特性集合")
    };
    assert!(
        tags.has(ActorTag::Beast),
        "monsters.ron 里的 traits 生效：{tags:?}"
    );
    assert!(
        !tags.has(ActorTag::Undead),
        "特性没有跟阵营混在一起：{tags:?}"
    );
}

#[test]
fn content_keeps_the_combat_schedule_running() {
    let mut app = headless_app();
    app.update();

    // 玩家空槽 + 有可用技能 → 相位切到"等输入"，时间冻结（这是设计要的行为）。
    app.update();
    app.update();
    assert_eq!(
        *app.world().resource::<State<CombatPhase>>().get(),
        CombatPhase::AwaitingInput,
        "空槽的玩家应该让战斗停下等输入"
    );

    // 可用技能由内容层填充（至少"普通攻击"）。
    let available = app.world().resource::<AvailableSkills>();
    assert!(
        !available.is_empty(),
        "内容加载完就应该有可用技能：{available:?}"
    );

    // 玩家实体仍然带着状态槽与行动槽（关系组件不报错）。
    let mut slots = app
        .world_mut()
        .query_filtered::<&ActorState, With<Player>>();
    assert!(slots.iter(app.world()).next().is_some());
}

/// **引擎的每条分支都要有内容在走**（这几条 TODO 存在的全部理由）。
///
/// ## 为什么这条值得写
///
/// Stacking 有四种、Formula 有三种，**引擎全部支持且有单测**。
/// 但单测只能证明"分支本身对"，证明不了"内容真的用到它" ——
/// 于是会出现"引擎有这个能力、游戏里永远碰不到"的悬空分支。
///
/// 这个坑实际发生过：Formula::RollUnder 与 Stacking::{Ignore, Replace}
/// 在 work/TODO.md 里挂了很久，写着"内容里还没有用它的技能"。
///
/// 所以这里把"内容覆盖度"钉成断言：**加一条引擎分支，就必须有内容走它**。
/// 将来有人删掉某个用例状态/技能时，这条会立刻亮。
#[test]
fn content_exercises_every_engine_branch() {
    use voxelith_axiom::behaviors::contest::Formula;
    use voxelith_axiom::behaviors::status::Stacking;

    let mut app = headless_app();
    app.update();

    // ---- Stacking：四种规则都要有状态在用 ----
    let statuses = app.world().resource::<StatusCatalog>();
    let defs: Vec<&Stacking> = statuses
        .iter()
        .filter_map(|(_, entity)| {
            app.world()
                .get::<voxelith_axiom::behaviors::status::StatusDef>(entity)
        })
        .map(|def| &def.stacking)
        .collect();
    assert!(!defs.is_empty(), "该解析出至少一个状态定义");

    let has_refresh = defs.iter().any(|s| matches!(s, Stacking::Refresh));
    let has_stack = defs.iter().any(|s| matches!(s, Stacking::Stack { .. }));
    let has_ignore = defs.iter().any(|s| matches!(s, Stacking::Ignore));
    let has_replace = defs.iter().any(|s| matches!(s, Stacking::Replace));
    assert!(has_refresh, "Stacking::Refresh 没有内容在用");
    assert!(has_stack, "Stacking::Stack 没有内容在用");
    assert!(
        has_ignore,
        "Stacking::Ignore 没有内容在用 —— 引擎分支悬空（见 staggered 状态）"
    );
    assert!(
        has_replace,
        "Stacking::Replace 没有内容在用 —— 引擎分支悬空（见 urning 状态）"
    );

    // ---- Formula：三种对抗式都要有技能在用 ----
    let skills = app.world().resource::<SkillCatalog>();
    let formulas: Vec<&Formula> = skills
        .iter()
        .filter_map(|(_, entity)| {
            app.world()
                .get::<voxelith_axiom::behaviors::skill::Skill>(entity)
        })
        .flat_map(|skill| skill.effects.iter())
        .filter_map(|effect| match effect {
            voxelith_axiom::behaviors::effect::Effect::Contest(contest) => Some(&contest.formula),
            _ => None,
        })
        .collect();
    assert!(!formulas.is_empty(), "该解析出至少一个对抗效果");

    assert!(
        formulas.iter().any(|f| matches!(f, Formula::Difference)),
        "Formula::Difference 没有技能在用"
    );
    assert!(
        formulas.iter().any(|f| matches!(f, Formula::RollUnder)),
        "Formula::RollUnder 没有技能在用 —— 引擎分支悬空（见 shield_bash）"
    );
    // Ratio 目前**故意没有**内容用例：现有数值里 attacker/defender 的比值
    // 没有哪一对是设计上有意义的（写了也是凑数）。所以这里**不断言**它 ——
    // 但留这条注释说明"为什么它缺"，免得下次有人以为是漏了。
}
