# Bevy 事件机制参考：Message vs Event（R33–R38 的落地依据）

> 本文解决一个具体问题：**什么场景用 `Message`，什么场景用 `Event`。**
> 依据是本地 vendored 的 `bevy_ecs 0.19.1` 源码（`src/message/*`、`src/event/mod.rs`、`src/observer/*`）与 Bevy 官方示例（`examples/ecs/message.rs`、`examples/ecs/observers.rs`）。
> 本项目规则（R33–R38）里的"事件"，如果不特别说明，指的是 **Message**。

## 1. 两套机制的本质区别

| 维度 | **Message**（缓冲消息） | **Event / EntityEvent**（观察者事件） |
|---|---|---|
| 消费模型 | **拉取（pull）**：系统在调度点主动 `read()` | **推送（push）**：`trigger` 当场调用 observer |
| 执行时机 | 固定的调度点（`Update` 等 schedule 运行时） | **立即**，属于 `trigger`/`Commands::trigger` 调用的一部分 |
| 触发方式差异 | 无 | `world.trigger(..)` **立即**执行 observer；`commands.trigger(..)` 是**延迟**触发，等命令缓冲 apply 时（`ApplyDeferred`）才执行 |
| 数据存活 | 默认存活 **2 次 `Messages::update`**；没人读则静默丢弃 | 不缓冲；本次触发结束即消失 |
| 多读者 | ✅ 天然支持：每个 `MessageReader` 有独立游标，各自读全量 | ✅ 支持：所有匹配的 observer 都会跑 |
| 并行性 | ✅ `MessageReader` 是只读参数，可与其它系统并行 | ❌ observer 触发的执行是串行的、阻塞调用方 |
| 可否改写 | ✅ `MessageMutator<M>` 可读可写（拦截式转发） | ✅ `On<E>` 实现 `Deref`/`DerefMut`，observer 可改事件数据 |
| 实体绑定 | 手动把 `Entity` 写进字段 | ✅ `EntityEvent` 自带 `event_target`，可按实体分发 |
| 层级冒泡 | ❌ 需自己实现 | ✅ `#[entity_event(propagate)]` + `ChildOf`，可逐级向上、可中断 |
| 注册方式 | `app.add_message::<M>()`（必需，否则无人 `update`） | `app.add_observer(..)` / `commands.observe(..)`（不需要 `add_message`） |
| 参数形式 | `MessageReader<M>`（只读，**可与其它系统并行**）/ `MessageWriter<M>`（需独占）/ `MessageMutator<M>`（读写） | `On<E>`、`On<E, B>`（`B` 限定组件，如 `On<Add, Mine>`） |
| 顺序保证 | 需要显式排序（`before`/`after`/`chain`），否则读写同帧有竞态 | **多个 observer 之间顺序任意**，Bevy 不提供排序 API；只有"触发时机"由调用方决定 |
| 典型代价 | 需要每帧 `update`，漏读会丢消息 | 立即执行，深调用栈会卡当前帧 |
| 承载跨帧状态 | 不承载（数据本身不推进） | **不承载**。跨帧动画/计时一律放**组件**，由每帧系统推进 |

## 2. 判定规则（按顺序问，第一个"是"就定案）

| 顺序 | 问题 | 是 → | 为什么这一个问题就够 |
|---|---|---|---|
| 1 | 这是 Bevy **内建**的生命周期变化吗？（组件被加/删、实体被生成/销毁） | **Event**（`On<Add, C>` / `On<Remove, C>` / `On<Despawn, C>`） | 内建只提供 Event，没有 Message 版本；这类"结构变化"必须即时反应 |
| 2 | 反应必须绑定**某个实体**，并且要沿 **`ChildOf` 向上冒泡**吗？ | **EntityEvent** + `#[entity_event(propagate)]` | Message 没有目标实体、没有层级遍历，自己实现等于重写 Bevy 的 traversal |
| 3 | 触发方必须**在同一个调用栈内**拿到反应结果吗？ | **Event**（`world.trigger_ref`） | Message 只在调度点被拉取，跨不过调用栈 |
| 4 | 以上都不是 | **Message** | 见下面 §2.1 —— 这才是本项目的默认选择 |

### 2.1 默认选 Message 的四个理由

Voxelith 的通信绝大多数是"**核心数据变化了，请所有关心的人处理**"，正好命中 Message 的四个能力，而这四个能力 Event 全都没有：

| 能力 | Message | Event | 对 Voxelith 的意义 |
|---|---|---|---|
| 多系统**并行**各读全量 | ✅ 每读者独立游标 | ❌ 串行阻塞 | 战斗结算、UI、日志、统计可同时观察一次伤害 |
| **可控调度点** + 显式排序 | ✅ schedule + `.chain()` | ❌ **顺序任意，且无排序 API** | 结算顺序可复现、可测试（R54 公式链依赖它） |
| **广播 + 缓冲**（可合并、可延迟） | ✅ 双缓冲、存活 2 帧 | ❌ 转瞬即逝、无人接就没了 | 区块脏标记可以攒着一起处理（R70、R72） |
| **拦截改写** | ✅ `MessageMutator<M>` | ✅ `On<E>` 也能改，但只在调用栈内 | 公式层改 `amount` 后放行（R54） |
| **承载跨帧表现** | ✅ 可攒多帧后统一处理 | ❌ 不承载（事件是瞬时的） | 但**动画状态本身应存组件**，见 §2.4 |

### 2.1.1 容易判错的场景

| 看起来像 | 实际应该 | 原因 |
|---|---|---|
| "特效是跨帧的，所以不能用 Event" | **分层看**：通知用 Event，动画用组件（§2.4） | observer 只跑一瞬间，不承载动画 |
| "Event 更『快』，高频就用 Event" | **Message** | 高频热路径用 Message 更省：缓冲批处理没有"每事件一次 observer 调度"开销 |
| "玩家受击了" → 一句话断言 Event | 拆两层：数据用 **Message**，表现启动用 **Event** | 改血量是数据，受击闪烁是实体表现 |
| "状态切换" → Event | **都不是**：用 `State` / `SubStates` | 状态是一等公民，包成事件会丢掉状态机转换语义 |
| "输入变化" → Event | **都不是**：`ButtonInput` 资源轮询 | 输入是采样，不是事件 |
| "因为 observer 写起来短" | **Message** | 便利的代价是丢掉调度、排序、过滤能力 |
| "多个 observer 按注册顺序跑" | **不要这样假设** | Bevy 官方：顺序任意（bevy#14890）。需要顺序 → Message + 显式排序 |

### 2.2 红线：核心数据变化一律 Message

**规则：凡是会改变 L0/L1 核心数据（血量、速度、位置、体素）的通信，只能用 Message，禁止用 Event。**

两个理由，都是硬伤：

1. **消息没被读会静默丢弃。** `Messages` 是双缓冲，读者超过 1 帧不读就永久失去这条数据（源码原话 "dropped silently"）。核心结算丢一条 → 数据永久错。
2. **observer 之间的顺序是任意的，而且没有排序 API。** Bevy 源码原话：*"Currently, Bevy does not provide a way to specify the relative ordering of observers watching for the same event. Their ordering is considered to be arbitrary. It is recommended to make no assumptions about their execution order."*（`observer/distributed_storage.rs`，bevy#14890）

   > ⚠️ 不是"按注册顺序"。是**未定义**。所以只要"谁先跑"会影响结果（叠加、抵消、优先级、先算减伤再算暴击），就必须用 Message + 显式 `.chain()`/`.before()`/`.after()`。

**推论**：observer 的职责边界是——**结构变化通知 + 表现的"启动"**；**不做核心结算，也不承载跨帧生命周期**（见 §2.4）。

### 2.3 "立刻"到底是什么

| 说法 | 真实含义 | 应对 |
|---|---|---|
| `world.trigger(...)` | **真·立即**：observer 在 `trigger` 这行代码里同步执行完才返回 | 适合需要当场结果；重活会卡当前帧 |
| `commands.trigger(...)` | **延迟**：排进命令队列，等命令 apply（`ApplyDeferred` / 下一个同步点）才跑 | 别假设它已经执行完，见 §6 陷阱 7 |

"立刻"≠"更早"、≠"优先级更高"、≠"不会被别的系统插入"。

**关键澄清：这份"同步"的代价只在"通知处理"那几微秒，与跨帧动画无关。** 见下一节。

### 2.4 跨帧表现 vs 通知：两件事，别混

最常见的误判是把"特效/动画是跨帧的"当成"所以不能用 Event"。**其实这两者不在同一个层面**：

| | 通知（Event 或 Message 负责） | 跨帧表现（**组件 + 每帧系统**负责） |
|---|---|---|
| 内容 | "此刻这个实体死了" | "闪 3 帧 / 渐隐 0.5 秒 / 播完动画再销毁" |
| 时长 | 瞬时，一次 | 若干帧，有进度 |
| 载体 | `EntityEvent` / `Message` | 组件（`EffectFade { frames_left }`）+ 推进系统 |
| 谁推进 | 无需推进 | 每帧运行的系统 |

**observer 里只做两件事：起一个特效实体（或往实体上插组件）、写初始数值。之后每一帧由普通系统推进。**

实测结论（见 [`crates/voxelith-axiom/tests/effect_lifecycle.rs`](../crates/voxelith-axiom/tests/effect_lifecycle.rs)）：

| 问题 | 实测结果 |
|---|---|
| observer 的同步执行会卡住动画吗？ | **不会**。observer 只跑一瞬间；实测特效实体在 observer 里创建后，正常活满 3 帧并被系统逐帧推进、自行消散 |
| 特效能比"触发它的实体"活得久吗？ | **能**。实测死者当帧被 despawn，特效作为独立实体继续存活到生命周期结束 |
| observer 里 `commands.spawn/insert` 同帧可见吗？ | **可见**。命中当帧的后续系统就能查到该特效实体（依赖 schedule 边界的同步点） |
| 那 Event 的问题到底在哪？ | **在"顺序"和"丢不丢"**：多个 observer 顺序未定义；observer 不缓冲、没人接就没了。所以它不适合做结算 |

**所以"特效展示用哪个"的正确问法是两层**：
1. **通知这一层**：死亡/命中该由 Event 还是 Message 通知？→ 按 §2 判定（表现启动通常 Event 更自然，因为绑定实体）。
2. **动画这一层**：动画状态永远在组件里，与选 Event 还是 Message 无关。

> 反面写法：在 observer 里直接做"等待 0.5 秒后销毁"（比如 observer 内死循环/阻塞），那才会卡帧。**observer 里禁止阻塞、禁止重计算。**

## 3. 决策树

**先问"要传的是通知，还是状态？"** —— 状态（含跨帧动画）永远不进事件，进组件。

```
要传的东西是什么？
├─ 跨帧状态（动画进度、渐隐计时、Buff 剩余时间）
│   └─ 组件 + 每帧系统推进   ← 不进事件，与 Message/Event 之争无关
└─ 一次性的"发生了什么"（通知）
    ├─ 是 Bevy 内建的生命周期变化吗？（组件增删、实体销毁）
    │   └─ 是 → Event：On<Add, C> / On<Remove, C> / On<Despawn, C>
    ├─ 需要绑定某个实体并沿 ChildOf 冒泡吗？
    │   └─ 是 → EntityEvent + #[entity_event(propagate)]
    ├─ 触发方必须在同一次调用内拿到反应结果吗？
    │   └─ 是 → Event（world.trigger_ref）
    └─ 其余全部情况 → Message        ← 本项目默认
        ├─ 需要中途改写数值 → MessageMutator<M>
        └─ 需要确定性顺序   → 显式 .chain() / .before() / .after()
```

## 4. 本项目（Voxelith）逐场景判定

| 场景 | 用哪个 | 理由 |
|---|---|---|
| 释放请求 `CastRequest` | **Message** | L2 输入 / AI 发出，L1 消费；请求是异步的，不该立即执行（R33、R50） |
| 区块脏标记 `ChunkDirtyMessage` | **Message** | 唤醒异步网格化任务，不需要立即执行；可合并多帧修改（R70） |
| 体素选中变化 `VoxelSelectedMessage` | **Message** | 表现层每帧读最新状态即可（R77） |
| 状态实例到期/被摘 `DetachStatusMessage` | **Message** | 结算系统与生命周期系统解耦：发的一方不关心谁执行（R33） |
| 血量归零 `DeathEvent` | **EntityEvent** | 目标明确（阵亡实体），需要绑定实体分发；表现与掉落可 `observe` |
| **特效/动画的"启动"信号**（受击闪白、死亡特效用 observer 起） | **EntityEvent**（通知层） | 通知只需一瞬间、且天然绑定"被击中的那个实体"。**动画本身住在组件里，与这里的选择无关**（§2.4） |
| 特效的"推进与结束"（闪 3 帧、渐隐 0.5 秒） | **都不是** | 这是跨帧状态：存组件 + 每帧系统推进。不要用事件承载生命周期（§2.4） |
| UI 点击 / 悬停 | **EntityEvent（+propagate）** | 需要按实体绑定并沿 `ChildOf` 冒泡 |
| `Lifetime` 到期销毁 | **EntityEvent** | 目标是那个实体，销毁动作要立即且确定 |
| 组件增删触发的初始化/清理 | **Event（`On<Add, C>` / `On<Remove, C>`）** | Bevy 内建生命周期事件，没有 Message 版本 |
| 输入（键盘/鼠标状态） | 都不是 | 用 `ButtonInput` 资源轮询，不要包成事件 |
| 状态机切换（`State`） | 都不是 | 用 Bevy `State` / `SubStates`，不要包成事件 |
| **战斗日志 `CombatLog`** | **都不是（Resource）** | 它是"追加 + 每帧读完清空"的缓冲；效果执行器拿不到 `MessageWriter`，所以用 Resource 降低耦合 |

> ⚠️ 注意上面把**同一件事拆成了两行**：通知（Event）与动画推进（组件+系统）。
> 把它们混成一行，就会得出"特效跨帧所以不能用 Event"这种错误结论。

## 5. 代码模板

### 5.1 Message：广播 + 多读者

```rust
#[derive(Message, Debug, Clone, Copy)]
pub struct CastRequest { pub caster: Entity, pub skill: Entity, pub target: Option<Entity> }

// 注册（发出方模块的 Plugin，R34）
app.add_message::<CastRequest>();

// 写入
fn player_input(mut out: MessageWriter<CastRequest>) {
    out.write(CastRequest { caster, skill, target: Some(enemy) });
}

// 读取（可以有任意多个系统各读一遍全量）
fn cast_requests(mut reader: MessageReader<CastRequest>, /* ... */) {
    for request in reader.read() { /* ... */ }
}
```

### 5.2 MessageMutator：拦截式改写（对应 R54）

```rust
// ⚠️ 同一系统里同时用 MessageReader + MessageWriter 会资源冲突，
//    读写同一消息类型必须用 MessageMutator。
// 现行设计里，数值改动不再靠"拦消息"，而是 `Contest` 的结果分支
// （见 docs/combat-design.md §4）——所以这个模板目前没有代码用到，
// 保留它是为了说明"需要就地改写时该怎么做"。
fn amplify(mut requests: MessageMutator<CastRequest>) {
    for request in requests.read() {
        // 就地改，后续读者看到的是改后的值
    }
}
```

### 5.3 EntityEvent：绑定实体 + observer

```rust
#[derive(EntityEvent, Debug, Clone, Copy)]
pub struct DeathEvent {
    #[event_target]          // 或直接用名为 entity 的字段 / 单字段元组结构体
    pub entity: Entity,
}

// 触发（L0 唯一写血量的系统里）
commands.trigger(DeathEvent { entity });

// 监听（L2 表现层）
app.add_observer(|death: On<DeathEvent>, mut commands: Commands| {
    commands.entity(death.entity).despawn();
});
```

### 5.4 生命周期事件：组件增删

```rust
// 组件被添加时初始化（不需要注册任何 Message）
app.add_observer(|add: On<Add, Mine>, mut index: ResMut<SpatialIndex>| {
    index.insert(add.entity);
});
```

### 5.5 层级冒泡

```rust
#[derive(EntityEvent)]
#[entity_event(propagate)]            // 沿 ChildOf 向上传播
pub struct VoxelClicked { #[event_target] pub entity: Entity }

// observer 里可控制是否继续冒泡
fn on_click(click: On<VoxelClicked>) {
    if click.propagate(true) { /* 继续上传给父实体 */ }
}
```

### 5.6 特效：通知 + 组件驱动动画（本项目推荐写法）

**分工**：observer 负责"起特效并给初值"（一瞬间），组件 + 每帧系统负责"播多久"（跨帧）。

```rust
/// 跨帧表现的载体：纯数据，不含任何渲染类型（R10）。
/// 真实项目里这里是"动画剩余时长 / 已播放帧数"，由渲染层读取并据此画。
#[derive(Component)]
pub struct HitFlash {
    frames_left: u32,
}

// ✅ 通知层：observer 只做"起特效 + 写初值"，不阻塞、不做重计算
fn on_death(death: On<DeathEvent>, mut commands: Commands) {
    commands.entity(death.entity).despawn();
    // 特效是独立实体（可以比触发它的实体活得久）
    commands.spawn(HitFlash { frames_left: 3 });
}

// ✅ 表现层：每帧推进，靠系统而不是靠事件
fn tick_hit_flash(mut commands: Commands, mut q: Query<(Entity, &mut HitFlash)>) {
    for (entity, mut flash) in &mut q {
        if flash.frames_left <= 1 {
            commands.entity(entity).despawn();
        } else {
            flash.frames_left -= 1;
        }
    }
}

// 注册
app.add_systems(PostUpdate, tick_hit_flash);
app.add_observer(on_death);
```

❌ 反面写法：

```rust
// ✗ 想"0.5 秒后消失"却用事件/命令去延迟 → 依赖调度细节，不可预测
// ✗ 在 observer 里阻塞、sleep、死循环 → 整帧卡住
// ✗ 把动画进度塞进事件数据里传 → 事件是瞬时的，进度无处存放
```

## 6. 顺序与生命周期陷阱（务必注意）

1. **不注册 = 没人 update**：`Message` 必须 `add_message`，否则缓冲区永不清理，可能无限增长。
2. **同帧读写有竞态**：writer 和 reader 之间若没有顺序约束，消息可能落在这帧或下帧的边界之后。要求同帧可见就加 `.chain()`；不要求就接受延迟。
3. **2 帧即丢**：`Messages` 是双缓冲，每次 `update` 交换并清掉最旧的一半。**读得比每帧一次慢的系统会静默丢消息**（Bevy 源码明示 "dropped silently"）。这条是 §2.2 红线的来源。
4. **读写冲突**：同一系统里 `MessageReader<M>` + `MessageWriter<M>` 会冲突，改用 `MessageMutator<M>`。
5. **别用 observer 做重活**：observer 是立即执行、阻塞调用方的。在 observer 里做网格化/寻路这类重活会卡住当前帧。
6. **别用 Message 表达"结构变化"**：组件被加了、实体被销毁这类事有时机要求，用生命周期 `Event`，不要自己发消息（会出现"消息还在排队，实体已经没了"）。
7. **`commands.trigger` 不是立即执行**：它把触发排进命令队列，等 apply 时才跑 observer。需要"触发者当场看到 observer 的修改"时用 `world.trigger_ref`。
8. **observer 之间没有顺序保证、也没有排序 API**：不要靠"注册顺序"推理（Bevy 官方：顺序任意，bevy#14890）。顺序有语义 → 改用 Message + 显式排序。
9. **别用事件承载跨帧生命周期**：想"0.5 秒后消失"就存组件 + 每帧递减（§2.4）。若用 `commands.trigger` 延迟触发去"等一会儿"，会依赖调度细节、且不可预测。
10. **别在 observer 里阻塞/睡等**：observer 卡住 = 触发方卡住 = 整帧卡住。要延迟就用组件记时间。

## 7. 命名约定（已确认）

**Message 以 `Message` 结尾，Event 以 `Event` 结尾。** 后缀即机制，读代码不用跳定义：

| 机制 | 后缀 | 例 |
|---|---|---|
| `Message`（缓冲、拉取） | `...Message` | `DetachStatusMessage`、`ChunkDirtyMessage`、`VoxelSelectedMessage` |
| 请求型 `Message` | `...Request` | `CastRequest`（仍是 Message，只是语义上是"请求"） |
| `Event` / `EntityEvent`（观察者、推送） | `...Event` | `DeathEvent`、`VoxelClickedEvent` |
| Bevy 内建生命周期 | 用内建类型 | `On<Add, C>`、`On<Remove, C>`、`On<Despawn, C>` |

判定与后缀必须一致：**后缀写错了，说明机制也选错了。** 代码注释里应写明为什么选这套机制（见 [`atoms/health.rs`](../crates/voxelith-axiom/src/atoms/health.rs) 的示例）。

## 8. 一页速查

| 我要做的 | 用 |
|---|---|
| 广播一个数据变化给所有关心的人 | `Message`（`...Message`） |
| 让别人能拦住并改我的数值 | `Message` + `MessageMutator` |
| 保证 A 系统处理完 B 系统才跑 | `Message` + 显式排序 |
| 让某个实体"立刻"响应 | `EntityEvent` + observer（`...Event`） |
| 起一个特效（"这里被打了"） | `EntityEvent` 通知 + **组件存动画状态** |
| 让特效播 3 帧 / 渐隐 0.5 秒 | **组件 + 每帧系统**（不是事件） |
| 点击向上冒泡到父级 | `EntityEvent` + `#[entity_event(propagate)]` |
| 组件被添加/移除时做初始化/清理 | `On<Add, C>` / `On<Remove, C>` |
| 读键盘/鼠标 | `ButtonInput` 资源（都不是） |
| 改状态机 | `State` / `SubStates`（都不是） |

## 9. 参考

- `bevy_ecs 0.19.1` → `src/message/mod.rs`（Message trait 文档：缓冲、拉取、并行读取）
- `bevy_ecs 0.19.1` → `src/message/messages.rs`（双缓冲、"dropped silently"、`MessageMutator` 冲突说明）
- `bevy_ecs 0.19.1` → `src/event/mod.rs`（Event 立即执行、EntityEvent、propagation）
- `bevy_ecs 0.19.1` → `src/observer/system_param.rs`（`On` 的 `Deref`/`DerefMut`、`propagate`）
- `bevy_ecs 0.19.1` → `src/observer/distributed_storage.rs`（**observer 顺序任意、无排序 API**，bevy#14890）
- Bevy 示例：`examples/ecs/message.rs`、`examples/ecs/observers.rs`、`examples/ecs/observer_propagation.rs`
- 本项目实测证据：[`crates/voxelith-axiom/tests/effect_lifecycle.rs`](../crates/voxelith-axiom/tests/effect_lifecycle.rs)
