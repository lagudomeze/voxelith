# 时间与行动（Bevy 实现）

模型见 [`../04-time-and-actions.md`](../04-time-and-actions.md)。这一篇只讲**用 Bevy 的什么、踩过哪些坑**。

---

## 一、用哪两个钟

| 用谁 | 干什么 |
| --- | --- |
| `Time<Virtual>` | **暂停开关**。世界该不该走，就是 `pause()` / `unpause()` |
| `Time<Fixed>` | **逻辑步长**。逻辑系统住 `FixedUpdate`，每步恒 1/64 秒 |

**不自己写时钟** ✓ 理由不只是省事：动画、音效、粒子这些 Bevy 侧的东西读的就是同一个钟 ✓ 世界冻结、慢放它们自动跟着 ✓ 自建时钟会和它们脱节 ✗

`Time`（不带参数的那个）在 `FixedMain` 里装的就是 `Time<Fixed>` ✓（源码：`run_fixed_main_schedule` 跑 schedule 前先 `*Time = Time<Fixed>::as_generic()`）✓ —— **但不要用它** ✗ 见下面的纪律 ✓

## 二、暂停 `Time<Virtual>` 真的能停住固定步长（实测）

`Time<Fixed>` 自己没有暂停开关 ✓（`pause` 只长在 `Time<Virtual>` 上）✓ 但它只从 Virtual 取时间：

```rust
// bevy_time/src/fixed.rs
pub fn run_fixed_main_schedule(world: &mut World) {
    let delta = world.resource::<Time<Virtual>>().delta();      // ← 唯一输入
    world.resource_mut::<Time<Fixed>>().accumulate_overstep(delta);
    while world.resource_mut::<Time<Fixed>>().expend() {        // 攒够一步才跑
        … FixedMain 跑一次 …
    }
}
```

探针（每帧喂"刚好一步"的假时间）：

```
FixedUpdate 跑过：暂停前 1 步 / 暂停 5 帧后 1 步（一步没跑 ✓）/ 恢复 3 帧后 4 步 ✓
Time<Fixed> timestep = 15.625ms（= 1/64 秒）
```

⇒ **`pause()` 一次 Virtual = 整个固定步长世界停住** ✓ `elapsed` 也不涨 ✓
⇒ `SY-57`（状态剩余时长、每跳、耐久恢复全停）**自动成立** ✓ 不需要任何额外机制 ✓

顺带实测到的两条：

- `Time<Virtual>` 的 `max_delta` 默认 **250 ms** ✓ 一帧最多推进约 16 步 ✓ 打断点调试时世界不会"快进一天" ✓
- `FixedUpdate` 里 `delta()` **恒等于** `timestep`（15.625ms）✓ 逻辑系统永远不用读真实帧长 ✓

## 三、时间轴基准：`Time<Fixed>::elapsed()`，不是 Virtual 的

| | `Time<Virtual>::elapsed()` | `Time<Fixed>::elapsed()` |
| --- | --- | --- |
| 累加 | 按真实帧长连续累加 ✗ | **整数个 tick**（每次 `expend` 加一个 15.625ms）✓ |
| "前摇到点"翻转发生在哪个 tick | 随帧划分浮动 ±1 tick ✗ | **确定的 tick** ✓ |
| 暂停时 | 不涨 ✓ | 不涨 ✓ |

方差只有 ±15.6ms，但它正好落在"反制是竞速"（`SY-41`）这种判定上 ✗ 而固定步长的全部价值就是消掉它 ✓

所以：**行动的 `resolves_at` 与所有"到点"判定，一律与 `Time<Fixed>::elapsed()` 比较** ✓

## 四、纪律（每条都对应一个实测或一个坑）

1. **显式写 `Res<Time<Fixed>>`，不写 `Res<Time>`** ✓
   `Res<Time>` 的语义**随系统所在的 schedule 变** ✗：在 `FixedUpdate` 里是 Fixed ✓ 挪到 `Update` 就悄悄变成 Virtual 时钟 ✗ dt 从 15.6ms 变成真实帧长 ✗
2. **不要在 `Update` 里读 `Time<Fixed>` 当"这帧过了多久"** ✗
   它在 `FixedMain` 之外是上一次步进留下的 `timestep` ✓ 语义是"一步多大"、不是"帧多久" ✓ 帧时长要用 `Time<Virtual>` 或 `Res<Time>` ✓
3. **内容里时间写秒** ✓ 内部换算成 tick：`四舍五入到最近 tick` ✓（0.2s → 12.8 tick → 13 tick = 0.203125s）✓
4. **推进权只有一个**：别处不许 `unpause()` ✓（除 `apply_freeze` 那个系统）✓ 也不许按墙上时间给自己加时间 ✗

## 五、冻结怎么落地

```rust
/// 世界为什么停。三个触发可以同时成立，挂一个就停。
#[derive(Resource, Default)]
pub struct Freeze {
    pub waiting_for_player: bool,   // 玩家没有在飞的前摇
    pub threat: bool,               // 有指着玩家的行动悬着（GP-32）
    pub manual: bool,               // 玩家按的暂停（可以下指令，代价照付）
}

/// 把结论施加到时钟上。放在 RunFixedMainLoop 之前（PreUpdate）。
fn apply_freeze(freeze: Res<Freeze>, mut virtual_time: ResMut<Time<Virtual>>) {
    if freeze.any() { virtual_time.pause() } else { virtual_time.unpause() }
}
```

**一拍延迟**：当帧的 `delta` 在 `First` 里就算完了 ✓ 所以这一帧改动 `pause` 最多让世界多跑一拍 ✓（15.6ms ✓ 可忽略）→ `apply_freeze` 摆在 `PreUpdate` 即可 ✓ 不需要跟 `RunFixedMainLoop` 抢顺序 ✓

## 六、行动实体的 Bevy 形状

```rust
#[derive(Component)]
pub struct Action { pub resolves_at: Duration, pub skill: SkillId, pub params: ActionParams }

/// 行动 → 谁在做（照 ChildOf / Children 那个模式）
#[derive(Component)]
#[relationship(relationship_target = Actions)]
pub struct ActionOf(pub Entity);

#[relationship_target(relationship = ActionOf, linked_spawn)]
pub struct Actions(Vec<Entity>);   // 角色没了 → 行动也没（linked_spawn）

/// 威胁：生成时算一次，存成关系
#[derive(Component)]
#[relationship(relationship_target = Threats)]
pub struct Threatens(pub Entity);

#[relationship_target(relationship = Threatens)]
pub struct Threats(Vec<Entity>);   // 不加 linked_spawn：目标没了，在飞的行动不必跟着没
```

实测要点（都验过）：

- **关系组件带额外字段要标注**：`#[relationship(relationship_target = X, allow_self_referential)]` + 字段上 `#[relationship]` ✓ 目标那边同理 ✓
- **`FromTemplate` 与"名单写法"二选一**：组件带 `#[derive(FromTemplate)]` 时，`bsn!` 里用名单挂它会报 `Failed to specify an entity for this EntityTemplate` ✗；不带派生则 `{ who: #X }` 那种写法编不过 ✗ → **行动/威胁这类由代码生成的关系，一律用 Rust 代码建** ✓ 内容里不写 ✓
- **名单会自动消失**：最后一个成员没了，目标上那个组件本身也没了 ✓ 所以读的人要用 `Option<&Actions>` ✓
- **`linked_spawn` 只管"主人没了"这一个方向** ✓ 删行动实体不会影响角色 ✓
- **不给它们派生 `FromTemplate`** ✓ 行动与威胁都由代码生成、内容里不写 ✓ 而且带派生的关系组件在 `bsn!` 的名单写法下会报"没指定实体" ✗ 两件事只能选一件 ✓
- **两个系统都住 `FixedUpdate`**（`begin_actions` → `resolve_actions`）✓ 与时间轴同频 ✓ 所以一切都按 tick 对齐 ✓
- **第一帧的 `delta` 是 0** ✓ 所以第一帧固定步长不跑 ✓ 测试里要空跑一帧热身 ✓（真实游戏里第一条输入远在这之后 ✓）

## 七、`begin_action` 为什么不是 `&mut World` 函数

模型里写的是 `begin_action(actor, skill, params)` ✓ 落地是一条**消息** ✓：

```rust
#[derive(Message)] pub struct BeginAction { actor: Entity, skill: SkillId, params: ActionParams }
#[derive(Message)] pub struct ResolveAction { actor: Entity, action: Entity, skill: SkillId, params: ActionParams }
```

理由：AI 与玩家输入在同一帧可能都想动手 ✓ 消息天然排好序 ✓ 而且代价走的是 `Adjust`（改值的唯一通路 ✓）✓ 一切都在 system 里 ✓ 不需要 `&mut World` 那种半出体系的东西 ✓

## 八、消息的可见性：不是"延迟一帧"

`Message` 写下去之后，**同一帧的后续系统读得到** ✓ 真正的问题是"**谁先读到取决于系统顺序**" ✗
所以：**同一个系统里看不到自己刚写的那条** ✓

代价就踩在这个点上 ✗："读当前值 → 检查 → 发 `Adjust` 消息" ✗ 检查看到的是**旧值** ✓
同一帧第二条请求照样通过 ✓ → 超扣 ✓

所以定了：**改值只有一个函数**（`voxelith_core::change`，带实体）✓

- **代价**：在付费那个系统里同步读完、查完、扣完 ✓ 中间没有消息 ✓
- **伤害 / 治疗 / 每跳**：仍走 `Adjust` 消息 ✓ 由 `apply_adjustments` 读消息后调同一个函数 ✓

## 九、那个"一取就脏"的坑会复发

[`01`](01-attributes-and-resources.md) 记过：把 `Mut<T>` 转成 `&mut T` 会立刻标记变更 ✗
**用 `QueryData` 时会换个面貌复发** ✗：字段写成 `&'static mut Modifier<To>` 之后，
`item.field.as_deref_mut()` 同样是 `DerefMut` ✓ → 每帧都标记 ✓ 下游被连锁触发 ✓

正确写法：**`as_mut()` 只读比较，值真的变了才写** ✓（写那一下才触发标记）✓
实测：错法下 `Changed<Modifier<…>>` 每帧 +1 ✗；改对之后稳定 ✓
