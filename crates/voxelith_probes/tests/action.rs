//! 探针：**一次完整的行动释放**跑通 —— 模板实体 + 类型化参数 + 同步付费 + 前摇 + 到点。
//!
//! 验的是第二步的形状（`docs/design/04-time-and-actions.md`）：
//!
//! - **技能 = 模板实体**：前摇、代价、强度基准都挂在它身上
//! - **行动 = 实例实体**：类型标记 + 参数 + 归属 + 算定好的 `resolves_at`
//! - **释放 = 一条泛型消息** `BeginAction<A>` → 一个通用系统 `begin_action::<A>`
//! - **代价 = 同步**：读 → 查 → 扣都在 `begin_action` 里；不够 → 拒绝消息，**且不打断当前前摇**
//! - **到点** → `ActionResolved` + 实例消失
//!
//! 这里的类型全是本地的：**验证通过之后搬进 `voxelith_rules`** ✓
//! 复用的是已经定下来的东西：`Cost<Q>` / `Mana` / `TilePos` / `change` / `try_pay` / `round_to_step`。

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_ecs::query::QueryData;
use bevy_ecs::system::SystemParam;
use bevy_time::{Fixed, Time, TimePlugin, TimeUpdateStrategy};
use core::time::Duration;
use voxelith_core::prelude::*;
use voxelith_rules::prelude::{Costs, RejectReason, round_to_step, try_pay};

const STEP: Duration = Duration::from_micros(15625); // 1/64 秒

// ==================== 技能模板（内容侧）====================

/// 挂在模板实体上：这是技能定义，不是行动实例。
#[derive(Component, Clone, Copy, Debug)]
struct SkillTemplate;

/// 前摇（内容写秒，进代码是 `Duration`；以后用自定义 `Template` 让作者写 `{ seconds: 0.4 }`）。
#[derive(Component, Clone, Copy, Debug)]
struct Windup(Duration);

/// 强度基准（`SY-40`：属性 + 技能威力 + 修正；属性那半还没做）。
#[derive(Component, Clone, Copy, Debug)]
struct BasePotency(f32);

// ==================== 一种行动＝标记组件 + 参数组件 ====================

/// 行动标记：**模板上必须有，实例上会挂**。
#[derive(Component, Clone, Copy, Debug, Default)]
struct Fireball;

/// 意图参数：**调用者给的那一份数据**（打哪）。挂在实例上，效果能直接读。
#[derive(Component, Clone, Copy, Debug)]
struct FireballAt {
    at: TilePos,
}

/// 一种行动：它只接受哪一种参数。**一个类型只能实现一次** → 1:1 由 Rust 保证。
trait ActionKind: Component + Default + Sized {
    /// 意图参数。
    type Params: Component + Clone;
    /// 日志 / UI 用的名字。
    const NAME: &'static str;
}

impl ActionKind for Fireball {
    type Params = FireballAt;
    const NAME: &'static str = "火球";
}

// ==================== 行动实例 ====================

/// **那一次行动**：生成时一次算定，之后不变。
#[derive(Component, Clone, Copy, Debug)]
struct Action {
    resolves_at: Duration,
    /// 从哪个模板来的（效果那块要读模板上的效果定义）。
    skill: Entity,
}

/// 这次行动是谁在做（0 或 1 个；关系天然满足 `GP-03` 一次只做一件事）。
#[derive(Component, Clone, Copy, Debug)]
#[relationship(relationship_target = Actions)]
struct ActionOf(Entity);

#[derive(Component, Default)]
#[relationship_target(relationship = ActionOf, linked_spawn)]
struct Actions(Vec<Entity>);

/// 这次行动的强度（`SY-41` 的"此刻"）：可修正的量。
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
#[require(Total<Potency>)]
struct Potency(f32);

impl Value for Potency {
    fn raw(&self) -> f32 {
        self.0
    }

    fn raw_mut(&mut self) -> &mut f32 {
        &mut self.0
    }
}

// ==================== 消息 ====================

/// 释放：**按行动类型分消息**，参数类型由 `ActionKind::Params` 说。
#[derive(Message, Clone, Copy, Debug)]
struct BeginAction<A: ActionKind> {
    actor: Entity,
    /// 技能模板实体（不是行动实例）。
    skill: Entity,
    params: A::Params,
}

/// 前摇走完。
#[derive(Message, Clone, Copy, Debug)]
struct ActionResolved {
    actor: Entity,
    action: Entity,
    skill: Entity,
}

/// 没放成：**游戏内正常失败**。
#[derive(Message, Clone, Copy, Debug)]
struct ActionRejected {
    actor: Entity,
    skill: Entity,
    reason: RejectReason,
}

// ==================== 系统参数：给长查询起名字 ====================

/// 模板上这次要用的定义（`QueryData`：字段有名，加一项只改一处）。
#[derive(QueryData)]
#[query_data(mutable)]
struct TemplateParts {
    skill_template: &'static SkillTemplate,
    windup: &'static Windup,
    mana_cost: Option<&'static Cost<Mana>>,
    base_potency: Option<&'static BasePotency>,
}

/// 释放要用到的世界侧东西（`SystemParam`：免得 `begin_action` 的签名一长串）。
#[derive(SystemParam)]
struct CastContext<'w, 's> {
    clock: Res<'w, Time<Fixed>>,
    mana: Query<'w, 's, &'static mut Mana>,
    stamina: Query<'w, 's, &'static mut Stamina>,
    templates: Query<'w, 's, TemplateParts>,
    casting: Query<'w, 's, &'static Actions>,
    rejected: MessageWriter<'w, ActionRejected>,
    commands: Commands<'w, 's>,
}

// ==================== 系统 ====================

/// 释放：校验 → 付费 → 取消旧行动 → 算定前摇 → 生成实例。
fn begin_action<A: ActionKind>(
    mut begins: MessageReader<BeginAction<A>>,
    marks: Query<(), With<A>>,
    mut ctx: CastContext,
) {
    for begin in begins.read() {
        // ① 模板上必须有这种行动的标记（编程错误 → panic，说得出名字）
        assert!(
            marks.get(begin.skill).is_ok(),
            "技能模板 {:?} 上没有 {} 这个标记",
            begin.skill,
            A::NAME
        );

        // ② 读模板（不是技能模板 → panic）
        let Ok(template) = ctx.templates.get(begin.skill) else {
            panic!("{:?} 不是技能模板（缺 SkillTemplate / Windup）", begin.skill);
        };
        let windup = template.windup.0;
        let base_potency = template.base_potency.map_or(0.0, |p| p.0);

        // ③ 代价：同步事务。不够 → 拒绝，**并且不动当前那次行动**
        let costs = Costs {
            mana: template.mana_cost.map_or(0.0, Cost::amount),
            stamina: 0.0,
        };
        if let Err(reason) = try_pay(begin.actor, &costs, &mut ctx.mana, &mut ctx.stamina) {
            ctx.rejected.write(ActionRejected {
                actor: begin.actor,
                skill: begin.skill,
                reason,
            });
            continue;
        }

        // ④ 取消当前那次行动（`GP-03`），再生成新的
        if let Ok(current) = ctx.casting.get(begin.actor) {
            for old in current.iter() {
                ctx.commands.entity(old).despawn();
            }
        }

        // ⑤ 前摇一次算定：模板的 Windup × 各种因素 → 四舍五入到整步（`D-011`）
        let resolves_at = ctx.clock.elapsed() + round_to_step(windup, ctx.clock.timestep());

        // ⑥ 生成实例：类型标记 + 参数 + 强度 + 归属
        ctx.commands.spawn((
            Action {
                resolves_at,
                skill: begin.skill,
            },
            begin.params.clone(),
            A::default(),
            Potency(base_potency),
            ActionOf(begin.actor),
        ));
    }
}
/// 到点：发消息 + 删实例。
fn resolve_actions(
    clock: Res<Time<Fixed>>,
    mut resolved: MessageWriter<ActionResolved>,
    mut commands: Commands,
    actions: Query<(Entity, &Action, &ActionOf)>,
) {
    let now = clock.elapsed();
    for (action, state, of) in &actions {
        if now >= state.resolves_at {
            resolved.write(ActionResolved {
                actor: of.0,
                action,
                skill: state.skill,
            });
            commands.entity(action).despawn();
        }
    }
}

// ==================== 测试 ====================

#[derive(Resource, Default)]
struct Resolved(Vec<ActionResolved>);

#[derive(Resource, Default)]
struct Rejected(Vec<ActionRejected>);

fn collect(
    mut resolved: MessageReader<ActionResolved>,
    mut rejected: MessageReader<ActionRejected>,
    mut out_resolved: ResMut<Resolved>,
    mut out_rejected: ResMut<Rejected>,
) {
    out_resolved.0.extend(resolved.read().copied());
    out_rejected.0.extend(rejected.read().copied());
}

fn the_app() -> App {
    let mut app = App::new();
    app.add_plugins((TaskPoolPlugin::default(), TimePlugin));
    app.init_resource::<Resolved>();
    app.init_resource::<Rejected>();
    app.add_message::<BeginAction<Fireball>>()
        .add_message::<ActionResolved>()
        .add_message::<ActionRejected>()
        .add_systems(
            FixedUpdate,
            (begin_action::<Fireball>, resolve_actions).chain(),
        )
        .add_systems(PostUpdate, collect);
    // 自检节拍：每帧正好一个逻辑步，与墙钟无关
    app.insert_resource(TimeUpdateStrategy::ManualDuration(STEP));
    app
}

fn template(app: &mut App, windup_ms: u64, mana: f32, potency: f32) -> Entity {
    app.world_mut()
        .spawn((
            SkillTemplate,
            Fireball,
            Windup(Duration::from_millis(windup_ms)),
            Cost::<Mana>::new(mana),
            BasePotency(potency),
        ))
        .id()
}

fn caster(app: &mut App, mana: f32) -> Entity {
    app.world_mut().spawn(Mana(mana)).id()
}

fn only_instance(app: &App, actor: Entity) -> Entity {
    app.world()
        .get::<Actions>(actor)
        .expect("应该有在飞的行动")
        .iter()
        .next()
        .expect("名单里应该有一条")
}

/// 完整闭环：模板 → 释放 → 付费 → 前摇 → 到点（实例消失、收到消息）。
#[test]
fn one_full_cast() {
    let mut app = the_app();
    // 第一帧的 delta 是 0，固定步长不跑 → 先热身一帧
    app.update();

    let skill = template(&mut app, 400, 10.0, 6.0);
    let hero = caster(&mut app, 30.0);
    app.update();

    app.world_mut().write_message(BeginAction::<Fireball> {
        actor: hero,
        skill,
        params: FireballAt {
            at: TilePos::new(3, 0, 4),
        },
    });

    // 跑到实例出现（生成与读取之间可能隔一帧的 flush）
    let mut frames = 0;
    while app.world().get::<Actions>(hero).is_none() {
        app.update();
        frames += 1;
        assert!(frames < 10, "该生成实例");
    }

    // ① 实例上：类型标记 + 参数 + 归属 + 强度，前摇对齐整步
    let action = only_instance(&app, hero);
    let world = app.world();
    assert!(world.get::<Fireball>(action).is_some(), "实例带类型标记");
    assert_eq!(
        world.get::<FireballAt>(action).unwrap().at,
        TilePos::new(3, 0, 4),
        "实例带参数（效果能直接读）"
    );
    assert_eq!(world.get::<ActionOf>(action).unwrap().0, hero, "归属写对了");
    assert_eq!(world.get::<Potency>(action).unwrap().0, 6.0);
    let resolves_at = world.get::<Action>(action).unwrap().resolves_at;
    assert_eq!(
        resolves_at.as_nanos() % STEP.as_nanos(),
        0,
        "前摇对齐到整步"
    );

    // ② 代价：先扣，且不退
    assert_eq!(world.get::<Mana>(hero).unwrap().raw(), 20.0, "扣了 10 点法力");

    // ③ 跑到点：消息到手、实例消失
    let mut frames = 0;
    while app.world().resource::<Resolved>().0.is_empty() {
        app.update();
        frames += 1;
        assert!(frames < 200, "该到点了");
    }

    let record = app.world().resource::<Resolved>().0[0];
    assert_eq!(record.actor, hero);
    assert_eq!(record.action, action);
    assert_eq!(record.skill, skill, "消息里带得回模板，效果那块要用");
    assert_eq!(
        app.world().resource::<Time<Fixed>>().elapsed(),
        resolves_at,
        "到点判定与时间轴对齐"
    );
    assert!(
        app.world().get::<Actions>(hero).is_none(),
        "实例没了，空名单也一起消失"
    );
}

/// 付不起 → 拒绝 + 不生成 + **不打断当前前摇**。
#[test]
fn rejection_does_not_cancel_the_current_windup() {
    let mut app = the_app();
    app.update();

    let skill = template(&mut app, 400, 10.0, 6.0);
    let hero = caster(&mut app, 15.0); // 只够一次
    app.update();

    // 第一次：成功
    app.world_mut().write_message(BeginAction::<Fireball> {
        actor: hero,
        skill,
        params: FireballAt {
            at: TilePos::new(1, 0, 1),
        },
    });
    app.update();
    let first = only_instance(&app, hero);

    // 第二次：法力只剩 5，付不起
    app.world_mut().write_message(BeginAction::<Fireball> {
        actor: hero,
        skill,
        params: FireballAt {
            at: TilePos::new(2, 0, 2),
        },
    });
    app.update();

    assert_eq!(
        app.world().resource::<Rejected>().0.len(),
        1,
        "应该有一条拒绝"
    );
    let rejection = app.world().resource::<Rejected>().0[0];
    assert_eq!(rejection.actor, hero, "拒绝消息带着谁");
    assert_eq!(rejection.skill, skill, "拒绝消息带着哪个技能");
    assert_eq!(
        rejection.reason,
        RejectReason::NotEnough(voxelith_rules::prelude::ResourceKind::Mana)
    );
    assert_eq!(
        app.world().get::<Mana>(hero).unwrap().raw(),
        5.0,
        "拒绝时不扣钱"
    );
    let still = only_instance(&app, hero);
    assert_eq!(still, first, "被拒的那次不该打断当前前摇");
}

/// 模板上没有这个行动的标记 → 当场 panic（说得出名字）。
#[test]
#[should_panic(expected = "火球")]
fn template_without_the_marker_panics() {
    let mut app = the_app();
    app.update();

    // 模板缺 Fireball 标记
    let skill = app
        .world_mut()
        .spawn((
            SkillTemplate,
            Windup(Duration::from_millis(400)),
            Cost::<Mana>::new(10.0),
        ))
        .id();
    let hero = caster(&mut app, 30.0);
    app.update();

    app.world_mut().write_message(BeginAction::<Fireball> {
        actor: hero,
        skill,
        params: FireballAt {
            at: TilePos::ZERO,
        },
    });
    app.update();
}
