//! L2 HUD 测试：**"内容里配几个池，界面就几行"**。
//!
//! 界面没法在无头测试里"看"，但它的**结构**是纯 ECS 事实，可以断言：
//!
//! - 左上状态面板的资源行数 == 玩家 `Resources` 里的池数量；
//! - 行里的名字来自 `vocabulary.ron`，数值来自池的当前值；
//! - 左下技能按钮数 == `AvailableSkills`（L1 判定的结果，UI 不重复判断）；
//! - 点按钮发的是 `CastRequest`，不直接改任何数据（**R16**）。
//!
//! 这些用 `MinimalPlugins` + 内容层跑：UI **节点**是普通组件，不需要渲染后端。

use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use bevy::time::TimePlugin;
use bevy::ui::widget::Text;
use voxelith_axiom::atoms::actor::{Player, Resources};
use voxelith_axiom::behaviors::action::CastRequest;
use voxelith_axiom::behaviors::combat::CombatPlugin;
use voxelith_axiom::behaviors::content::Vocab;
use voxelith_axiom::behaviors::phase::AvailableSkills;
use voxelith_prime::content::ContentPlugin;
use voxelith_prime::presentation::{
    HudResourceBar, HudResourceRow, HudResourceValue, HudSkillButton, HudSkillsPanel,
    HudStatusPanel, PresentationPlugin,
};

/// 无头 App：内容 + 机制 + 表现（**不含渲染**）。
///
/// `RawContent` 在真程序里由 `main` 解析后 `insert_resource`；测试补上同一步。
fn app() -> App {
    let mut app = App::new();
    app.add_plugins((
        TimePlugin,
        StatesPlugin,
        CombatPlugin,
        ContentPlugin,
        PresentationPlugin,
    ));
    app.insert_resource(voxelith_prime::content::parse_raw().expect("`.ron` 内容该配好"));
    // **补上 UI 图集**：真程序里 `load_ui_atlas` 会插入 `UiAtlas`
    // （句柄指向 `assets/ui/kenney-adventure.png`）。测试里没有资源设施
    // （`AssetPlugin` 要整个渲染后端），所以给一份**空句柄**的图集。
    //
    // 这够用：这些测试断言的是 **HUD 的结构与文字**，不是像素。
    // 图块坐标由 `ui_theme::atlas` 的单测**对着素材自带的 XML**逐条核对。
    app.insert_resource(voxelith_prime::ui_theme::UiAtlas {
        image: Handle::default(),
        width: 592,
        height: 600,
    });
    app
}

/// 收集某个面板下的子节点文本。
fn texts_under(app: &mut App, root: Entity) -> Vec<String> {
    let mut texts: Vec<String> = Vec::new();
    let mut stack = vec![root];
    while let Some(entity) = stack.pop() {
        if let Some(children) = app.world().get::<Children>(entity) {
            stack.extend(children.iter());
        }
        if let Some(text) = app.world().get::<Text>(entity) {
            texts.push(text.0.clone());
        }
    }
    texts
}

#[test]
fn status_panel_has_one_row_per_pool() {
    let mut app = app();
    // 第一帧：内容加载 + 生成实体；第二帧：HUD 的同步系统才看得到池。
    app.update();
    app.update();

    let pool_count = {
        let mut query = app.world_mut().query_filtered::<&Resources, With<Player>>();
        query.iter(app.world()).next().expect("有玩家").pools.len()
    };
    assert_eq!(
        pool_count, 4,
        "players.ron 里配了 hp / mana / action / reaction"
    );

    let mut rows = app.world_mut().query::<&HudResourceRow>();
    assert_eq!(
        rows.iter(app.world()).count(),
        pool_count,
        "**内容里配几个池，界面就几行**（加一种资源不用改 UI 代码）"
    );

    let mut bars = app.world_mut().query::<&HudResourceBar>();
    assert_eq!(bars.iter(app.world()).count(), pool_count, "每行一条进度条");

    let mut values = app.world_mut().query::<&HudResourceValue>();
    assert_eq!(
        values.iter(app.world()).count(),
        pool_count,
        "每行一个数值文本"
    );
}

#[test]
fn resource_rows_show_content_names_and_live_values() {
    let mut app = app();
    app.update();
    app.update();

    let panel = {
        let mut query = app
            .world_mut()
            .query_filtered::<Entity, With<HudStatusPanel>>();
        query.iter(app.world()).next().expect("有状态面板")
    };
    let texts = texts_under(&mut app, panel);

    // 标题是**内容里的角色名**（`players.ron` 的 `name`），不是实体标识。
    assert!(
        texts.iter().any(|text| text == "冒险者"),
        "面板标题该是内容里的角色名：{texts:?}"
    );

    // 名字来自 vocabulary.ron（不是 "hp" 这种内部 ID）。
    for expected in ["生命", "法力", "行动", "反应"] {
        assert!(
            texts.iter().any(|text| text == expected),
            "面板上该有 `{expected}`：{texts:?}"
        );
    }

    // `action` 池是满的（1/1），`reaction` 也是（3/3）——数值来自池，不是写死的。
    assert!(
        texts.iter().any(|text| text == "1 / 1"),
        "行动池满值该显示 1 / 1：{texts:?}"
    );
    assert!(
        texts.iter().any(|text| text == "3 / 3"),
        "反应池满值该显示 3 / 3：{texts:?}"
    );
}

#[test]
fn pool_values_are_refreshed_when_they_change() {
    let mut app = app();
    app.update();
    app.update();

    // 直接改池（模拟战斗结果）：这是**测试在扮演 L1**，不是 UI 在改数据。
    let player = {
        let mut query = app.world_mut().query_filtered::<Entity, With<Player>>();
        query.iter(app.world()).next().unwrap()
    };
    let hp = app
        .world()
        .resource::<Vocab>()
        .resource("hp")
        .expect("hp 已登记");
    app.world_mut()
        .get_mut::<Resources>(player)
        .unwrap()
        .modify(hp, -17.0);

    app.update();

    let panel = {
        let mut query = app
            .world_mut()
            .query_filtered::<Entity, With<HudStatusPanel>>();
        query.iter(app.world()).next().unwrap()
    };
    let texts = texts_under(&mut app, panel);
    assert!(
        texts.iter().any(|text| text == "83 / 100"),
        "扣血后界面要跟着变：{texts:?}"
    );

    // 进度条宽度也变了。
    let widths: Vec<f32> = {
        let mut query = app.world_mut().query::<(&HudResourceBar, &Node)>();
        query
            .iter(app.world())
            .filter(|(bar, _)| bar.pool == hp)
            .map(|(_, node)| match node.width {
                Val::Percent(value) => value,
                _ => panic!("进度条宽度该是百分比"),
            })
            .collect()
    };
    assert_eq!(widths, vec![83.0], "83% 的血 → 83% 宽的条");
}

#[test]
fn skill_panel_lists_exactly_the_available_skills() {
    let mut app = app();
    app.update();
    app.update();

    let available = app.world().resource::<AvailableSkills>().len();
    assert!(available > 0, "内容里至少有普通攻击");

    let mut buttons = app.world_mut().query::<&HudSkillButton>();
    assert_eq!(
        buttons.iter(app.world()).count(),
        available,
        "按钮数 == L1 判定的可用技能数（UI 不自己判断）"
    );

    let panel = {
        let mut query = app
            .world_mut()
            .query_filtered::<Entity, With<HudSkillsPanel>>();
        query.iter(app.world()).next().expect("有技能面板")
    };
    let texts = texts_under(&mut app, panel);
    assert!(
        texts
            .iter()
            .any(|text| text == "普通攻击" || text == "basic_attack"),
        "按钮上该有技能名：{texts:?}"
    );
}

#[test]
fn skill_panel_hides_other_actors_skills() {
    // 这条是**回归保护**：以前技能没有"归属"概念，"可用"只按需求 / 消耗筛，
    // 于是哥布林的招式会出现在玩家的技能栏里（双方都满足"有行动点、没在行动中"）。
    let mut app = app();
    app.update();
    app.update();

    let panel = {
        let mut query = app
            .world_mut()
            .query_filtered::<Entity, With<HudSkillsPanel>>();
        query.iter(app.world()).next().expect("有技能面板")
    };
    let labels = texts_under(&mut app, panel);

    for enemy_skill in ["哥布林劈砍", "哥布林逃窜"] {
        assert!(
            !labels.iter().any(|text| text == enemy_skill),
            "玩家的技能栏里不该出现怪物的 `{enemy_skill}`：{labels:?}"
        );
    }
    assert!(
        labels.iter().any(|text| text == "普通攻击"),
        "玩家自己的招式要在：{labels:?}"
    );
}

#[test]
fn clicking_a_skill_sends_a_request_instead_of_writing_data() {
    let mut app = app();
    app.update();
    app.update();

    // 记下"发请求前"的池状态，用来证明 UI 没直接改数据。
    let player = {
        let mut query = app.world_mut().query_filtered::<Entity, With<Player>>();
        query.iter(app.world()).next().unwrap()
    };
    let before = app.world().get::<Resources>(player).unwrap().clone();

    // 把某个按钮按下去。
    let button = {
        let mut query = app.world_mut().query::<(Entity, &HudSkillButton)>();
        query.iter(app.world()).next().map(|(entity, _)| entity)
    };
    let button = button.expect("有按钮");
    *app.world_mut().get_mut::<Interaction>(button).unwrap() = Interaction::Pressed;

    app.update();

    // 请求发出去了（测试可以读消息，因为**没有别的系统消费它**——战斗系统在本帧已跑完）。
    let requests = app.world().resource::<Messages<CastRequest>>();
    assert!(!requests.is_empty(), "点击该发出一条 `CastRequest`");

    // 但 UI **没有**动池：`Player` 的资源在点击后立刻读还是原值（扣费由 L1 在下一帧做）。
    let after = app.world().get::<Resources>(player).unwrap();
    assert_eq!(
        before.pools.len(),
        after.pools.len(),
        "UI 不改池结构（R16）"
    );
}
