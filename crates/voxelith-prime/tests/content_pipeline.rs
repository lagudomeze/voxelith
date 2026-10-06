//! L2 内容管线测试：六份 `.ron` **真的能装载**，并生成可战斗的实体。
//!
//! 这些断言的价值在于：内容配错（引用了没登记的池 / 属性 / 状态）在**启动期**就会炸，
//! 而不是等到某次战斗里静默失效。用无头 App 跑，不需要窗口。
//!
//! **走的是真实装载路径**：`ContentPlugin` 在 `PreStartup` 里经 `ContentManifest`
//! （`SceneComponent`）从 `assets/data/*.ron` 装载，再翻译注入。所以这里要装
//! `AssetPlugin` —— 它顺带提供 IO 线程池，`AssetServer` 的装载就在那上面跑。

use bevy::app::TaskPoolPlugin;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use bevy::time::TimePlugin;
use voxelith_axiom::atoms::actor::{
    ActorState, ActorTags, Faction, Monster, Player, Resources, Stats,
};
use voxelith_axiom::behaviors::combat::CombatPlugin;
use voxelith_axiom::behaviors::content::{SkillCatalog, SkillId, StatusCatalog, Vocab};
use voxelith_axiom::behaviors::phase::{AvailableSkills, CombatPhase};
use voxelith_axiom::behaviors::skill::Skill;
use voxelith_prime::content::{ContentAssetKind, ContentManifest, ContentPlugin};

/// 资产根：工作区根的 `assets/`（与 `main.rs` 用的是同一处）。
const ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");

/// 组装一个无头 App：内容层 + 机制层（不含渲染 / 窗口）。
fn headless_app() -> App {
    let mut app = App::new();
    // CombatPlugin 内部已经装配了 ActionPlugin 等子域。
    // `AssetPlugin` 要在 `ContentPlugin` 之前：后者要 `init_asset` 六份配置类型。
    app.add_plugins((
        TaskPoolPlugin::default(),
        bevy::asset::AssetPlugin {
            file_path: ASSETS.to_string(),
            ..default()
        },
        bevy::scene::ScenePlugin,
        TimePlugin,
        StatesPlugin,
        CombatPlugin,
        ContentPlugin,
    ));
    app
}

/// 六份配置的路径**只有一处真出处**：`ContentManifest::scene()` 里的字符串。
///
/// `ContentAssetKind::file()` 是同一批文件名的第二处写法（报错信息要用字面量），
/// 这条测试拿**真实句柄**把两者钉在一起——抄错了会红，不会静默错位。
#[test]
fn the_manifest_points_at_the_six_config_files() {
    let mut app = headless_app();
    app.update();

    let (kind_paths, handle_paths) = {
        let mut query = app.world_mut().query::<&ContentManifest>();
        let manifest = query.iter(app.world()).next().expect("清单实体在");
        let server = app.world().resource::<bevy::asset::AssetServer>();
        (
            [
                ContentAssetKind::Vocabulary.file(),
                ContentAssetKind::Skills.file(),
                ContentAssetKind::Statuses.file(),
                ContentAssetKind::Players.file(),
                ContentAssetKind::Monsters.file(),
                ContentAssetKind::World.file(),
            ],
            manifest.paths(server),
        )
    };

    for (expected, actual) in kind_paths.iter().zip(&handle_paths) {
        assert!(
            actual.ends_with(expected),
            "清单装载的路径与标签对不上：标签 {expected}，实际 {actual}"
        );
    }
}

#[test]
fn content_files_load_and_install_the_vocabulary() {
    let mut app = headless_app();
    // `PreStartup` 会**阻塞**等到六份配置就绪，所以这一次 update 之后内容已经在世界上了。
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

    // `players.ron`：hp 100 / 力量 12；特性来自数据（ToME4 移植后是 `humanoid`）。
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
    // 特性是**词汇 ID**，所以照 `vocabulary.ron` 查（而不是写死一个枚举变体）——
    // 与下面哥布林那条同样的写法：写死就等于把"人形排第几"抄进测试，反而测不出映射。
    let humanoid = app
        .world()
        .resource::<Vocab>()
        .tag("humanoid")
        .expect("vocabulary.ron 登记了 humanoid");
    assert!(
        traits.has(humanoid),
        "players.ron 里的 traits 生效：{traits:?}"
    );
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
    // 特性是**词汇 ID**：`monsters.ron` 里写的 `"beast"` 由 `vocabulary.ron` 的
    // `tags` 表换成 ID。所以这里走 `Vocab` 查，而不是写死一个 Rust 枚举变体——
    // 写死就等于把"野兽到底排第几"抄进测试，反而测不出映射对不对。
    let (beast, undead) = {
        let vocab = app.world().resource::<Vocab>();
        (
            vocab.tag("beast").expect("vocabulary.ron 登记了 beast"),
            vocab.tag("undead").expect("vocabulary.ron 登记了 undead"),
        )
    };
    assert!(tags.has(beast), "monsters.ron 里的 traits 生效：{tags:?}");
    assert!(!tags.has(undead), "特性没有跟阵营混在一起：{tags:?}");
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

// ------------------------------------------------------------------ 热重载
//
// `file_watcher` 在真机上做的事就是：把新文件解析成资产、**替换 `Assets<A>` 里的值**、
// 发一条 `AssetEvent::Modified`。下面这些测试手工做同样两件事——于是不需要真的动磁盘，
// 却走的是同一条链路。

/// 取清单里那份技能的句柄。
fn skills_handle(app: &mut App) -> Handle<voxelith_prime::content::SkillsAsset> {
    let mut query = app.world_mut().query::<&ContentManifest>();
    query
        .iter(app.world())
        .next()
        .expect("清单实体在")
        .skills
        .clone()
}

/// 改一份配置并宣告"它变了"。
fn edit_skills(
    app: &mut App,
    edit: impl FnOnce(&mut Vec<voxelith_axiom::behaviors::content::SkillRon>),
) {
    let handle = skills_handle(app);
    {
        let mut assets = app
            .world_mut()
            .resource_mut::<Assets<voxelith_prime::content::SkillsAsset>>();
        let mut asset = assets.get_mut(&handle).expect("技能配置已装载");
        edit(&mut asset.0);
    }
    app.world_mut().write_message(
        bevy::asset::AssetEvent::<voxelith_prime::content::SkillsAsset>::Modified {
            id: handle.id(),
        },
    );
}

#[test]
fn editing_a_config_reloads_it_in_place() {
    let mut app = headless_app();
    app.update();

    let first = SkillId(0);
    let before = *app
        .world()
        .resource::<SkillCatalog>()
        .get(first)
        .expect("第一个技能有定义实体");
    let duration_before = app.world().get::<Skill>(before).unwrap().duration;

    edit_skills(&mut app, |skills| skills[0].duration = 7.5);
    app.update();

    let after = *app
        .world()
        .resource::<SkillCatalog>()
        .get(first)
        .expect("重载后仍在目录里");

    // **实体必须是同一个**：`Action` 用 `CastsSkill(Entity)` 指着它，
    // 状态实例用 `ActiveStatus.def` 指着它。换实体 = 悬空引用，而且不报错。
    assert_eq!(
        after, before,
        "热重载必须复用定义实体（同 ID 写在原实体上）"
    );
    assert_eq!(
        app.world().get::<Skill>(after).unwrap().duration,
        7.5,
        "新数值要真的生效"
    );
    assert_ne!(duration_before, 7.5, "原值不是 7.5（否则这条测不出东西）");
}

#[test]
fn removing_a_skill_from_the_config_retires_its_definition() {
    let mut app = headless_app();
    app.update();

    let before = app.world().resource::<SkillCatalog>().len();
    assert!(before > 1, "内容里不止一个技能（否则没得删）");
    let dropped = *app
        .world()
        .resource::<SkillCatalog>()
        .get(SkillId(0))
        .unwrap();

    edit_skills(&mut app, |skills| {
        skills.remove(0);
    });
    app.update();

    assert_eq!(
        app.world().resource::<SkillCatalog>().len(),
        before - 1,
        "目录跟着内容变"
    );
    assert!(
        app.world().get::<Skill>(dropped).is_none(),
        "没人引用的旧定义实体要销毁，不能留在世界里"
    );
}

/// **在配置中间插一条，不该把后面的 ID 整体顶掉一位。**
///
/// 词汇 ID 是按登记顺序发的（`NameTable::register` 拿当前长度当下标）。重新翻译时
/// 若不拿旧词汇表垫底，插入点之后的**所有**技能都会换一个 ID —— 那等于把 A 的定义
/// 悄悄换成 B 的，而且什么都不报。`ReusedDefs::vocab` 就是为这条而存在的。
#[test]
fn inserting_a_skill_keeps_the_other_ids_stable() {
    let mut app = headless_app();
    app.update();

    let before: Vec<(SkillId, Entity, String)> = {
        let catalog = app.world().resource::<SkillCatalog>();
        catalog
            .iter()
            .map(|(id, entity)| {
                let name = app
                    .world()
                    .get::<Skill>(entity)
                    .map(|skill| skill.name.clone())
                    .unwrap_or_default();
                (id, entity, name)
            })
            .collect()
    };
    assert!(before.len() > 1, "内容里不止一个技能（否则插队测不出来）");

    // 在最前面插一条全新的技能：照抄第一条，只改 id / name。
    edit_skills(&mut app, |skills| {
        let mut fresh = skills[0].clone();
        fresh.id = "brand_new_skill".to_owned();
        fresh.name = "全新技能".to_owned();
        skills.insert(0, fresh);
    });
    app.update();

    let catalog = app.world().resource::<SkillCatalog>();
    for (id, entity, name) in &before {
        assert_eq!(
            catalog.get(*id).copied(),
            Some(*entity),
            "`{name}`（{id:?}）被插队顶掉了 ID —— 它现在指向别的技能"
        );
        assert_eq!(
            app.world()
                .get::<Skill>(*entity)
                .map(|skill| skill.name.clone()),
            Some(name.clone()),
            "`{name}` 的定义实体被换成了别的技能"
        );
    }
}

#[test]
fn a_broken_config_keeps_the_old_content_instead_of_panicking() {
    let mut app = headless_app();
    app.update();

    let before = app.world().resource::<SkillCatalog>().len();

    // 引用了词汇表里没有的池 → 加载期的覆盖检查会挡下来。
    edit_skills(&mut app, |skills| {
        skills[0]
            .costs
            .push(voxelith_axiom::behaviors::content::CostRon {
                pool: "definitely_not_a_pool".to_owned(),
                amount: 1.0,
            });
    });
    // **不该 panic**：运行期炸掉用户正在玩的局毫无意义（启动期才该当场炸）。
    app.update();

    assert_eq!(
        app.world().resource::<SkillCatalog>().len(),
        before,
        "翻译失败要保留旧目录"
    );
}
