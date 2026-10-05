# ✅ L0 收拢：零依赖的**实例组件与关系**搬进 atoms

## 先量了一遍，结论和"L0 太小"不完全一样

```
atoms/      629 行   ← 搬之前
behaviors/ 7742 行
world/     1677 行
```

`behaviors/` 里 **5058 行没有任何世界访问**（不取 `Res` / `Query` / `Commands`）。
但**其中大部分不该动**：

- `value/` / `contest.rs` / `requirement.rs` / `targeting.rs` 是**公式**，
  而 `layers.md`（R54 / R59）明确写着"L1 可以在不知道具体层的情况下做判定、公式、筛选"
  —— 公式**本来就归 L1**；
- `content/`（描述结构 + 加载管线，约 2400 行）是内容管线，更不是 L0。

所以"L0 小"不是病。真正错位的是一小撮**零依赖的组件**。

## 进 L0 的判据（写进 `atoms/mod.rs` 了）

> **它认不认识别的东西？**

| 认不认识 | 放哪 | 例 |
|---|---|---|
| 谁都不引用（只有 `Entity` / 词汇 ID / 姊妹类型） | **L0** | `Action` / `ActiveStatus` / `DecisionSlot` / `Faction` |
| 引用**公式簇**（`Requirement` / `Effect` / `Value`） | L1（跟着公式） | `Skill` / `StatusDef` / `MonsterDef` |
| 要**世界**才算得出来 | L1（系统） | `tick_actions` / `monster_decide` |

好处是**判据会被编译器执行**：给 L0 组件加一个引用 L1 的字段，立刻多出一条
`atoms → behaviors` 的边——看得见，而不是悄悄长在 L1 里没人管。

## 搬了什么

| 新位置 | 内容 |
|---|---|
| `atoms/action.rs` | `Action` + `CastsSkill`/`CastingSkill` + `InitiatedBy`/`ActiveActions` + `ResolveNow`/`ReadyToResolve` + `Threat` + `active_of` |
| `atoms/status.rs` | `ActiveStatus` + `AttachedTo`/`Statuses` |
| `atoms/decision.rs` | `AiDecision` + `DecidedBy`/`DecisionSlot` + `SettledBy`/`Settles` + `decision_of` |

L1 那三个模块（`action` / `status` / `monster`）用 `pub use crate::atoms::…` **转出**，
所以 `behaviors::action::Action` 这类老路径照旧可用——改动只落在**定义处**。

`Threat` 也搬了：它标的是**行动**（反制窗口要取消的就是那条 action），
挂在 `monster` 域只是因为 `monster_act` 登记它。

**结果**：`atoms/` 629 → **1079 行**，7 个文件。

## 没搬的，和不搬的理由

- **公式簇**（约 2900 行）：R54 / R59 划给 L1，搬了就是改规则。
- **`Skill` / `StatusDef` / `MonsterDef`**：它们引用公式簇。搬它们等于把公式一起搬。
- **`world/`**：它是第二个 L0 域，但物理收敛要改公开路径，上一轮问过，还等你定。

## 还没做的拆分（有数据，等你点）

没有文件超过 R26 的 500 行，所以下面这些都是**可读性**问题，不是规则问题：

| 文件 | 行 | 混了什么 |
|---|---|---|
| `behaviors/requirement.rs` | 551 | 定义（`Requirement`/`Condition`/`Cost`/`Targeting`）+ `CasterContext` + 判定函数 |
| `behaviors/content/descriptor/mod.rs` | 483 | 技能 / 状态 / 角色 / 数值曲线四组描述结构 |
| `behaviors/status.rs` | 398 | 定义 + 3 个消息 + 5 个生命周期系统 + 修饰符派生 |
| `behaviors/effect/mod.rs` | 372 | `Effect` 枚举 + `EffectContext` + **一个 135 行的大 match**（可按小抄 §六 拆成 resource/status/action/meta 四个 resolver） |

`effect/mod.rs` 那个大 match 最贴近"把 resolver 按效果类别分开"的写法，
要拆的话建议从它开始。

## 证据

**295 测试全绿、守卫 10/10**；启动截图哈希 `52E53BE05531205A` 与改动前**逐像素相同**
——这次是纯搬家的行为中性改动。

---

# ✅ 决策槽：怪物 AI 的"决定"与"执行"拆开，槽用**一对一关系**

## 改了什么

`monster_tick`（一个系统里"攒能量 → 选招 → 生成行动"一条龙）拆成三步：

```text
monster_decide         能量满 → 选招 → 写进决策槽（只决定，不出手）
ApplyDeferred          让刚 spawn 的决策实体本帧可见
monster_act            决策槽有货 + 威胁窗口空 → 生成威胁行动 + 登记 PendingThreat
```

关系（全在 `behaviors/monster/decision.rs`）：

| | 源 | 目标 | 为什么 |
|---|---|---|---|
| 决策槽 | `DecidedBy(monster)` | `DecisionSlot(Entity)`，`linked_spawn` | 一个怪物**同时只有一条**决策 → 字段是单个 `Entity` 而不是 `Vec` |
| 生命周期 | `SettledBy(action)` | `Settles(Entity)`，`linked_spawn` | **行动一没，决策跟着没** |

## 为什么要拆（不是"拆着好看"）

原来那一个 `if` 里，"决定"没有落脚点：

1. **决定会被丢弃重掷**：威胁窗口只有一格，被别的怪占着时只能下一帧重选一遍；
   而条件随时在变（残血 / 中毒 / 换目标），那实际上是"每帧重掷、留下最后一次"。
   现在决定留在槽里等窗口，**条件再变也不改主意**。
2. **决定不可观察**：行动还没生成时，世界上没有任何东西能回答"这只怪打算干什么"。

## 一对一关系的两个坑（都不报错，只是沉默）

1. **槽的"空"不是"长度为 0"，是没有 `DecisionSlot` 组件。** 0.19 的 `RelationshipTarget`
   不支持 `Option<Entity>`，所以"没决定"只能表达为拆掉关系；拆掉时**组件本身的清理是
   排队命令**——同一帧里组件还在、集合已经空了。所以读槽一律走 `decision_of()`（内部
   `iter()`），连 `monster_decide` 的闸门也问"槽里真的有决策吗"而不是"组件在不在"。
2. **"换一条决策"不会销毁旧决策实体。** 一对一在新源顶掉旧源时只做**解绑**。所以
   `monster_decide` **不给它顶掉的机会**（槽不空就不写新决策），从根上删掉这条泄漏路径。

## 清空槽的三条路径（都是关系级联，没有"反推"）

| 路径 | 机制 |
|---|---|
| 行动正常结算 | `resolve_actions` despawn 行动 → 级联 despawn 决策 |
| 行动被反制取消 | 同上（`cancel_actions` 也只是 despawn 行动） |
| 怪物自己没了 | `DecisionSlot` 级联 despawn 决策 |

**为什么不能"每帧扫一遍 `PendingThreat` 反推谁该退役"**：`Effect::DispelAction` 把
`PendingThreat` **连 `source` 一起**清空，反推根本无从下手 —— 决策会永久卡在槽里，
于是怪物再也不出手，而且**什么都不报**。

## 代价（写进注释与 docs，别当零成本）

- 费用与冷却在**决定那一刻**付（不是出手那一刻）。不这样，等窗口的这几帧里池子变了
  就会得到一条"付不起"的决策，而付不起的决策会永远卡在槽里。
- 多一个系统 + 一个 `ApplyDeferred`（原来是 2 个，现在 3 个）。

## 方法论：先证明测试会红

5 个变异，每个都被指定的测试抓到（不是"改完能过"就算）：

| 变异 | 抓到它的测试 |
|---|---|
| 去掉"槽里已有决策 → 跳过" | `a_waiting_monster_does_not_rewrite_its_decision_every_frame` |
| 不退役"执行不了"的决策 | `a_decision_whose_skill_vanished_does_not_jam_the_slot` |
| 决策不挂到行动上 | `a_monster_decides_before_it_gets_to_act` 等 3 个 |
| `DecisionSlot` 去掉 `linked_spawn` | `a_monster_takes_its_decision_down_with_it` |
| `monster_act` 不看窗口 | `a_monster_decides_before_it_gets_to_act` |

## 差点踩的坑：测试不该依赖"时间有没有被冻结"

第一版用 `step(&mut app, 1.5)` 推进时间，`cargo test -p voxelith-axiom` 全过，
`cargo test --workspace` **必挂**。逐帧打印探针查出来：两个 feature 集下
`keep_resolving` 里 `CastRequest → ActiveActions` 的 `Commands` 落地差了**一帧**，
相位于是在 `Resolving` / `AwaitingInput` 之间摆动，倍率有时是 0。

修法不是"调宽时间"，而是**让结论与时间无关**：直接把 `ActionEnergy.current` 顶到阈值
（`ActionEnergy::tick` 是"先累加再判"，`delta = 0` 的帧照样出手），全部走 `app.update()`。
这恰好也是设计自夸的那条性质——**冻结对逻辑系统零感知**。

---

# ✅ 分层修正：词汇原子下沉到 L0；角色特性从 Rust 枚举改成词汇 ID

你问了两件事，查完之后发现它们**同一个根因**。

## 根因：一批通用原子被放在了 L1

L0 的组件要拿这些类型当字段：

```text
atoms/actor  ──►  behaviors::content::{ResourceId, SkillId, StatId}   ✗ L0 依赖 L1
world        ──►  behaviors::content::{NameTable, UnknownName}        ✗ 同上
```

`Resources` 的键、`Cooldowns` 的键、`Stats` 的键全是这些 ID —— **砖块向搭砖规则要尺寸**。
`atoms` 显得空，是因为本该在它这里的东西住在 L1（它们的第一个用户是战斗词汇表）。

**修法**：新增 `atoms/vocabulary.rs`（五种 `u16` 词汇 ID + `NameTable` + `UnknownName`），
`behaviors/content/vocabulary.rs` 只留 `Vocab`（聚合 + 解析），旧路径用 `pub use` 转出。
**两条反向依赖消失。**

## `world/`：第二个 L0 域，却没住在 L0

`world/` **1677 行，一行系统都没有**（`WorldPlugin` 只 `init_resource` + `add_message`），
全是数据（`Voxel` / `Chunk` / `VoxelStore`）与纯计算（噪声生成 / DDA 射线）——
**比 `atoms/` 还纯粹的 L0**。

而它：

- 在顶层当 `atoms` 的兄弟 ⇒ 顶层混着**层名**（`atoms` / `behaviors`）与**域名**（`world`）；
- **`architecture.md` §5 的目录表里根本没有它**（文档落后于代码）；
- 自己的模块文档只含糊地写"属于 L0/L1"。

已在 `architecture.md` 里写明"它是第二个 L0 域"。
**物理收敛（`atoms/world/`）没做**：那要改 `voxelith_axiom::world::…` 这个公开路径，
是纯机械改动，但属于"要不要"的选择，等你定。

## 特性（亡灵 / 野兽）不该是 Rust 枚举

你的直觉对，而且比"放错文件"更严重：它**违反了项目自己的规则**。

`layers.md` §9 的判据是**"这个集合会随游戏内容增长吗"** → 会就走 `.ron` + ID。
"亡灵 / 构装体 / 野兽"明显会（种族、类型），却做成了 `enum ActorTag`——
**每加一种种族都要改引擎并重编**。

更糟的是 `combat-design.md` §11 把它列成显式例外（"✅（一行枚举）"），
而**同一节往下两段**的标准写的正相反。规则文档自己打架。

**改成词汇 ID**：`ActorTagId` + `vocabulary.ron` 的 `tags:` 表 + `traits: ["beast"]`。

**判据不是"是不是标签"，而是"带不带规则"**：

| | 是什么 | 为什么 |
|---|---|---|
| `Faction` | **仍是枚举** | `hostile_to` 是一条引擎级敌对规则 |
| `ActorTagId` | 词汇 ID | 只是用来比较相等性的名字 |

## 证据

| 测试 | 钉住什么 |
|---|---|
| `a_brand_new_trait_needs_no_rust_change` | 造一个引擎**从没见过**的 `"demon"`，只加进 `VocabRon` 就能用 ⇒ "加内容不改代码" |
| `an_unknown_trait_name_is_reported` | 特性名写错报 `UnknownName{kind:"tag"}`，**不静默丢** |
| 变异：把没登记的名字静静当成 ID 0 | 被第二条抓到 |

**真机**：BRP 读到哥布林的 `ActorTags = [2]`（`undead=0, construct=1, beast=2`）✓；
启动截图哈希 `52E53BE05531205A` 与改动前**逐像素相同**。

顺带：`descriptor.rs` 越过 R26 → 切成 `descriptor/{mod,vocabulary}.rs`。

---

# ✅ 查询写法与组件存储：`QueryData` / `QueryFilter` / `SparseSet`

## 先做了一遍普查，再动手

你要的是两件事：**用 `QueryData`/`QueryFilter` 简化查询**、**枚举值用 `SparseSet`**。
动手前先把"哪里真的用得上"查了一遍，结果和预期不一样，值得记下来：

| 候选 | 结论 |
|---|---|
| `Faction`（枚举组件） | **不改**。它生成时写一次、之后永不改 ⇒ `SparseSet` 一点好处都没有，反而拖慢 `With<Faction>` |
| `ReadyToResolve` / `ResolveNow` / `Threat` | **不改**。它们是**过滤器**（`With<ReadyToResolve>`），`SparseSet` 让过滤更慢；而且只插一次 |
| `DecisionSlot` | ✅ 改。每个决策周期插一次摘一次，怪物身上十来个组件 |
| `PendingChunkMesh` | ✅ 改。临时标记，区块实体带着 `Mesh3d` / `MeshMaterial3d` 一串 |
| `Moving`（标记） | ❌ **删掉**：**从没有人 insert 过它**，`Has<Moving>` 恒为 false 且不报错 |

**结论写成了判据**：问的不是"它是不是枚举"，而是"**它会不会被反复插删**"。
`Faction` 是枚举但零增删；`DecisionSlot` 不是枚举却每周期增删。详见
[docs/bevy-queries.md](../docs/bevy-queries.md) §3.2。

## 改了什么

**`QueryData`（3 处，都满足"元组 ≥ 4 字段"或"要挂方法"）**

| 类型 | 在 | 它买到了什么 |
|---|---|---|
| `ReadyAction` | `behaviors/action` | 四元组 `(Entity, &CastsSkill, &InitiatedBy, &Action)` 有了名字 |
| `DecidingMonster` + `is_busy()` | `behaviors/monster` | 六元组；"它是不是正忙"从散落的条件变成方法 |
| `CameraSnapshot` + `kind()` / `projection_label()` | `voxel_render/camera` | 六元组；诊断函数体的 `match` 收进方法 |

**`QueryFilter`（2 个类型，4 处使用）**

```rust
pub struct InputDriven { player: With<Player> }      // 引擎角色轴：听输入的
pub struct AiDriven    { not_player: Without<Player> } // 引擎角色轴：自己 tick 的
```

它筛的是"谁听输入"，**不是**"谁是自己人"——一个 AI 驱动的友方 NPC 是
`Faction::Player` 却不该被 `InputDriven` 选中。裸的 `With<Player>` 读起来分不出这两件事，
而本项目的三条正交轴正是靠这种区分活着的。顺带把 `update_phase` 里**没被读的**
`&Player` 字段去掉了。

**其余地方不写**：两三个字段的元组包成结构体只是多一层跳转，没有信息增益。

## 一个把我说服了的细节：判据是 **table**，不是 archetype

第一版验证测试断言"插上 `SparseSet` 组件后 **archetype** 不变"——**它红了**，
而组件确实已经是 `SparseSet`。

Bevy 的真实模型比"参与 / 不参与 archetype 身份"更细：

- `Archetype` = 一组组件；
- `Table` = 真正存组件行的地方，**多个 archetype 可以共用一张 table**；
- 插 `SparseSet` 组件**仍然会换 archetype**，只是**不换 table**。

省掉的正是"把那一整行 `Table` 组件拷到新地方"。所以断言要写在
`archetype().table_id()` 上。写在 `archetype().id()` 上会得出"没生效"的**错误结论**
——我差点因此去翻 Bevy 的 bug。

## 每条存储选择配两条测试

| 测试 | 防什么 |
|---|---|
| `the_decision_slot_is_registered_as_a_sparse_set` | 有人顺手改回 `Table`：**什么都不报，只是变慢** |
| `the_decision_slot_does_not_move_the_monster_between_tables` | 属性写了但没生效（被别的 derive 属性吃掉） |
| `the_pending_marker_does_not_move_the_chunk_between_tables` | 同上，区块侧 |

变异验证：把 `#[component(storage = "SparseSet")]` 删掉 → 两条都红。

## 顺手修的与顺带发现的

- **删掉死标记 `Moving`**：声明了、写进了查询（`Has<Moving>`）、**从来没被 insert 过**，
  于是恒为 false 且不报错。判据早就全走 `Movement::is_moving()`。
- `atoms/actor.rs` 越过 R26 的 500 行 → 按**语义**切成
  `atoms/actor/mod.rs`（数值：池 / 属性 / 冷却 / 状态槽 / 能量）
  + `atoms/actor/axes.rs`（三条正交轴 + 两个过滤器），重导出保持旧路径不变。
- 派生宏要**显式 `use`**：`bevy_ecs::query::{QueryData, QueryFilter}`——
  prelude 只给 trait 不给 derive 宏，只靠 prelude 会得到一堆
  "cannot find derive macro / not a valid Query data" 的连环报错。

## 证据

**290 测试全绿、守卫 10/10。** 真机跑一遍：截图哈希 `52E53BE05531205A`
**与改动前逐像素相同**，`log_cameras`（换成了 `QueryData`）输出的相机诊断一字不差
——这次重构是行为中性的，不是"改了顺便修了点别的"。

---

# ✅ 静态配置改成 `Asset`：用 `SceneComponent` 装载六份 `.ron`

## 改了什么

`include_str!` 把六份 RON 编进二进制 → 现在是**真正的 `Asset`**，走 `AssetServer`：

```text
ContentManifest（SceneComponent，路径全写在 scene() 里）
  vocabulary / skills / statuses / players / monsters / world.ron
    → PreStartup：world.spawn_scene(bsn! { @ContentManifest })
    → 阻塞泵 handle_internal_asset_events 直到六份 LoadState::Loaded
    → 拼成 RawContent，insert_resource
    → Startup 照旧（图集 / 地形 / 精灵表 / HUD **一行没改**）
```

**路径只有一处**：`ContentManifest::scene()` 里的六个字符串。`bsn!` 把 `"data/skills.ron"`
转成 `HandleTemplate::Path`（装载时调 `AssetServer::load`），调用方只写 `@ContentManifest`。

## 为什么在 `PreStartup` **阻塞**（而不是改成异步状态机）

`Startup` 里一串系统（图集 / 材质 / 地形 / 精灵表 / HUD）都假设"内容已经在"，
而 `atlas::build_atlas` / `terrain::build_initial_terrain` 的参数是
`Option<Res<ContentData>>` —— **读不到就静默建一张空图集**，战场空掉却不报错。
这正是本项目反复踩的那类"沉默失败"。

所以守住的不是"加载快"，而是不变式：**`Startup` 一定发生在内容就绪之后**。
阻塞安全：装载跑在 `IoTaskPool`（独立线程），主线程泵事件就能收到，两边不会互相等。
10 秒超时 + `LoadState::Failed` 立刻 panic（**带文件名**）兜底。

## 热重载（换成 Asset 的唯一理由）

`bevy/file_watcher` 只在 debug 构建开（`watch_for_changes_override`）。
改 `.ron` → `AssetEvent::Modified` → 原地重新翻译。两条不变式：

1. **同 ID 的定义实体原地更新，不换实体**。
   `Action` 用 `CastsSkill(Entity)` 指着技能定义实体，`ActiveStatus` 用 `def` 指着状态定义
   实体。换实体 = 悬空引用，而**悬空引用不报错**，只是那一招从此画不出来。
2. **词汇表拿旧的当底再登记**（`build_vocab_into`）。
   词汇 ID 按登记顺序发（`NameTable::register` 用当前长度当下标）。不垫底的话，
   在 `skills.ron` 中间插一条会让它后面**所有**技能的 ID 整体后移一位——
   等于把 A 的定义悄悄换成 B 的。**这条是第一版测试红出来的**（我原本以为只要复用
   定义实体就够了）。

改错了（语法错 / 引用了没登记的池）**不 panic**：打一条 error 保留旧目录。
启动期才该当场炸；运行期炸掉用户正在玩的局毫无意义。

**不做的**：不重新生成角色、不重建地形。那是场景编排不是内容——重跑会把 PC 和怪物
再 spawn 一份、地形网格再叠一层。

## 顺手补的一个"静默不一致"

HUD 的技能按钮只在**可用技能集合**变化时重建。于是热重载改了 `skills.ron` 的 `name`
之后按钮一直显示旧名字，**而内容确实已经换了**。真机验证时截图哈希一模一样才发现的。
修法：`sync_skill_buttons` 的早退多加一个条件（`Changed<Skill>`），
`sync_resource_rows` 同理（`Labels::is_changed()`）。

## 真机验证（不是"测试绿了"就算）

| 步骤 | 证据 |
|---|---|
| 启动 | 截图：HUD 显示"冒险者 / 生命 100/100 / 技能栏 5 项"，全部来自 `.ron` |
| 内容→生成 | BRP `world.query`：`Monster` 实体 2 个 |
| 热重载 | 改 `skills.ron` 的 `name` → 日志 `Reloaded data\skills.ron` + `配置已热重载：9 个技能 / 6 个状态的定义实体原地更新` |
| 可见 | 截图哈希 `52E53BE0…` → `443D6D3B…`，按钮文字变成"普通攻击（热重载证明）" |

**截图哈希相同**那次是最有价值的一次：日志说重载成功了，画面却一模一样——
于是挖出了上面那个 HUD 缓存问题。只看日志会以为"做完了"。

## 变异验证

| 变异 | 抓到它的测试 |
|---|---|
| 不拿旧词汇表垫底 | `inserting_a_skill_keeps_the_other_ids_stable` |
| 每次都 spawn 新定义实体 | `editing_a_config_reloads_it_in_place` |
| 不销毁没人引用的旧定义实体 | `removing_a_skill_from_the_config_retires_its_definition` |
| 按钮不理会标签变化 | `relabelling_a_skill_refreshes_its_button` |

## 代价 / 新增依赖

- `bevy/file_watcher`（拉进 `notify`）。**编进来 ≠ 一直开着**：只在 debug 构建
  `watch_for_changes_override: Some(cfg!(debug_assertions))`。
- `src/` 里的单元测试（图集 / 地形 / 网格）仍然用同步的 `parse_raw()`——它现在
  只在 `cfg(test)` 里存在，**出货的二进制里没有 `include_str!`**。
  集成测试（`content_pipeline` / `hud`）走**真实装载路径**，不再有"测试跑的是一条
  游戏永远不走的路"这种隐患。

---

# ✅ 修复：展示台与地板**不在同一层** → 两套东西永远看不到一起

## 现象（用户发现的）

"我看地板范围和 glb 的范围没有重叠。感觉是放错位置了？"

**对，是我放错了。** 展示台在 `y = 40`（天上 40 格），地板在 `y = 0`。
相机要么看得到地板、要么看得到模型，**永远不能同时看到** ——
于是**没法判断模型有没有对齐格子**，而那恰恰是这个工具最该回答的问题。

## 为什么会这样（旧理由失效了）

`ModelGalleryConfig::origin` 原本写的是 `(0, 40, 0)`，注释是
"抬到地形上方（地形在 y = ±2 附近）" ——
那是**体素地形**时代为了不和地形重叠。

但后来默认地面换成了 **GLB 地板网格**（`floor_grid`），
它只在 `y = 0` 的一个薄层上。**"抬到地形上方"这个理由就不成立了**，
而代码没跟着改 —— 于是抬到天上去的那 40 格纯粹是浪费。

## 修法

1. `origin` 改成 `y = 0`（贴地）。
2. 往 `+X` 挪 `GALLERY_X_OFFSET` 让模型**站在格子上**：
   - 第一版取 `80`（"地板半边 48 + 展示台半宽 56 ⇒ 别重叠"）——
     **又是错的**：两者完全不挨着，模型站在地板外面的一片空地上；
   - 改成 `24`：展示台范围 `-32 .. +80`，其中 `-32 .. +48`
     **落在地板上**（80 格宽）⇒ 画面里既能看到成片格子，
     也能看到模型踩在格子上的样子。
3. `aim_camera_at_gallery` 改成**同时框住地板与展示台**：
   取两者在世界 XZ 平面上的**并集范围**，注视点取并集中心。
   只框展示台的话，地板完全在画面外 —— 又回到"看不见格子"。

## 教训

**改动一个系统的前提时，要回头检查依赖那个前提的代码。**
"抬到地形上方"是**体素地形**时代的前提；地形换成地板网格之后，
这个前提消失了，而 `y = 40` 变成了纯粹的错位。
这种"前提失效但代码没改"的错位，**不会报任何错**，只会让画面看起来莫名其妙。

---

# ✅ 新增：世界坐标轴指示器（`SHOW_AXES=1`）

## 为什么需要

等距视图下**方向极易看错**：俯角 45° + 方位角 45° 之后，
屏幕上的"右"既不是 `+X` 也不是 `+Z` —— **`+X` 与 `+Z` 在画面上是对称的两条斜线**。

实测（`work/shots/axes_on.png`）：

- **红 `+X`** 指向画面右下
- **绿 `+Y`** 竖直向上
- **蓝 `+Z`** 指向画面左下

调地形、判断"墙该往哪边加"时，光看画面**分不出这两个方向**，而后果完全不同。

## 实现

`voxel_render/world_axes.rs`：用 **Bevy 内置的 `Gizmos::axes(transform, base_length)`**，
不用自己搭箭头几何。它按 `Transform` 画三根轴，颜色固定 **X=红 / Y=绿 / Z=蓝**。

`GizmoPlugin` / `GizmoRenderPlugin` 已经包含在 `DefaultPlugins` 里，**不用额外注册**。

| 环境变量 | 含义 |
|---|---|
| `SHOW_AXES=1` | 打开（默认**关**，它是排查工具） |
| `AXES_LENGTH=8` | 每根轴的长度（世界单位，默认 8） |

3 条测试：默认关、长度为正、默认长度是**整数格**（这样箭头尖端正好落在格线上）。

## 现有的排查工具一览

| 开关 | 作用 |
|---|---|
| `SHOW_AXES=1` | 原点坐标轴（X/Y/Z 朝向） |
| `ORBIT_CAMERA=1` | 中键平移 / 滚轮缩放 / 右键转视角 / R 复位 |
| `MODEL_GALLERY=1` + `MODEL_GALLERY_KIT=dungeon\|cave\|platformer` | 素材浏览器 |
| `FLOOR_GRID=0` | 切回体素地形（默认是 GLB 地板网格） |
| `FLOOR_GRID_EXTENT=96` | 地板边长（格） |
| `ILLUM` / `AMBIENT` / `LIGHT_POS=x,y,z` | 光照调参 |
| `cargo run -p glb-probe --bin reproduce` | 复现"两个共面三角形谁赢"的隔离实验 |

---

# ✅ 已解决：GLB 模型**坐标原点不统一** → 整体偏半个到两个格子

## 现象（用户发现的）

"glb 分配坐标和底座好像差一半" —— 对。按"底面中心"摆放时，
**墙类模型整体偏出格子**，拼房间时墙会从格线上岔开。

## 根因：局部原点约定不统一

| 模型 | 包围盒（世界单位） | 原点在哪 |
|---|---|---|
| `template-floor` | `(-2, 0, -2) → (2, 0, 2)` | 底面**中心** ✅ |
| `room-small` | `(-6, 0, -6) → (6, 0, 6)` | 底面**中心** ✅ |
| `corridor` | `(-2, 0, -2) → (2, 0, 2)` | 底面**中心** ✅ |
| **`template-wall`** | `(-2, 0, -1.99) → (2, 4.15, 0)` | 底面**边缘**（中心在 `z = -1.0`） |
| **`stairs`** | `(-2.2, 0, -6.2) → (2.2, 8.55, 2.2)` | 中心在 `z = -2.0` |
| `template-wall-corner` | — | 中心在 `(-0.5, 0, -0.5)` |

**dungeon 39 个模型里 11 个**的原点不在底面中心，最大的偏**整整一个格子**。

## 修法：构建期生成的偏移清单

1. **`tools/gltf_offsets.py`**：从 GLB 字节里读出每个模型的 `POSITION` 包围盒，
   算出"把底面中心对到摆放点"要补的位移，写成 `.ron`。
   **三套各一份**：`assets/models/{dungeon,cave,platformer}/offsets.ron`。
2. **`voxel_render/model_offset.rs`**：读清单，`lookup(kit, name) -> Vec3`。
3. **`model_gallery`** 摆放时加上这个偏移。

**为什么用生成的清单而不是运行期算**：运行期要读 `Mesh` 顶点，
而 GLB 是**异步**加载的 —— 拿到顶点之前不知道该往哪摆，画面会先抖一下。

## 防回归测试（`model_offset.rs` 的 5 条）

- **三套清单逐套独立解析**，每套都要求 `>= 30` 条
  （手写解析器最怕"格式一变就静默解析出零条" ⇒ 所有偏移变 0 ⇒ 又偏了，且不报错）；
- **对着真实 GLB 逐个核对**每个模型的偏移（读 `accessor.min/max` 现算，与清单比）；
- 已知偏原点的模型值正确（`template-wall` = `z -1.0`、`stairs` = `z -2.0` …）；
- 原点本来就在底心的**不该**被写偏移（`template-floor` / `room-small` / …）；
- 未知模型退化成零偏移，不 panic。

## 我犯的错（值得记）

**生成脚本的路径写死了 `dungeon`** —— 三套互相覆盖，
最后跑的 `platformer` 把 `dungeon` 的清单盖掉了。
症状是"偏移全丢了"，**不报任何错**。
⇒ 生成类脚本的**输出路径必须由输入推导**，不能写死。

---

# ✅ 已解决：GLB 模型发黑 → 根因是**双面重叠三角形 z-fighting**

## 根因（一句话）

Kenney 的 quad 类模型（`template-floor` / `template-wall` …）在 GLB 里有
**两组三角形**：一组法线朝上 `(0,1,0)`、一组朝下 `(0,-1,0)`，
**两组的位置包围盒完全相同**。GLB 标了 `doubleSided: true`
⇒ Bevy 给材质 `cull_mode = None` ⇒ **两面都画** ⇒ 两个三角形**共面 z-fighting**，
而**朝下的那个赢了** ⇒ 光照按"法线朝下"求值 ⇒ `N·L ≤ 0` ⇒ **近黑**。

## 修法

`voxelith-prime/src/voxel_render/gltf_material.rs`（新模块）：
把**来自 glTF 场景**的材质统一改成 `unlit = true`。

**这不是"绕过问题"，而是正确做法**：Kenney 把明暗**烘焙在调色板里**了
（那些色带本身就是"岩石的亮面/暗面"），再叠一层实时光照是**重复着色**。
我们自己的体素图集也是同一个思路。

**⚠️ 不要改成"把模型翻转 180°"** —— 那只是让**另一个**三角形赢，
素材一更新或胜者一变就会翻回去。`unlit` 与 z-fighting 胜负**无关**。

## 实测证据（全部来自 `probe/glb` 的 `reproduce` bin）

| | 平放的瓦片 | 立起的瓦片 |
|---|---|---|
| 原样（走光照） | `(15,16,21)` **近黑** | `(111,117,143)` |
| 改成 `unlit` | **`(89,95,120)`** ✅ | **`(89,95,120)`** ✅ |

贴图原色 `(90,96,123)` ⇒ `unlit` 之后**完全正确**。

**防回归测试**（`gltf_material.rs`）：**直接读 GLB 字节**验证根因 ——
`doubleSided == true`、法线只有**两种取值**且**互为反向**、
朝上与朝下的**位置包围盒完全相同**。不是断言我自己的 bool。

## 这一条解释了此前**所有**互相矛盾的观测

| 此前的观测 | 用这个根因解释 |
|---|---|
| 平行光调 **400 倍**、环境光调 **250 倍**，画面**纹丝不动** | `N·L ≤ 0`，光强乘多少都是 0 |
| 灯移到**正上方**也不变 | 同上 |
| **拿掉贴图就正常**（纯灰 `(49,49,49)`） | 纯色时两个三角形颜色一样，谁赢都一样 |
| **"发黑但顶部有白色高光"** | 高光走另一条路径，不受 `N·L` 正负影响 |
| 贴图**明明在用**（换品红模型跟着变品红） | 贴图确实在采样，只是采的是背光那一份 |
| 隔离场景里"有时亮有时黑" | 取决于**姿态**：立起的 quad 正好让朝上的那面赢 |

## 我走过的弯路（都值得记）

1. **一路都在查"光照"**：光强、环境光、光位置、金属度、`base_color`、
   色彩空间、贴图路径、法线方向、near/far 裁剪……**全部不是原因**。
   真正的根因是"两个重合三角形谁赢"，**和光照几乎无关**。
2. **我把"探针正常"当成了既定事实**，其实探针里也是黑的 ——
   我把那个"深色钻石形状"读成了背景。因为这个误判，
   我花了很多轮去追一个**不存在的"探针 vs prime 差异"**。
3. **关键转折是"翻转 180° 就变亮"** —— 一条实验一次性把
   "光照/材质/色彩空间"整条排除掉，直接指向几何。
   **做能一次排除一整类假设的实验，比逐个调参数有效得多。**
4. **诊断脚本本身出错**：改素材的脚本把备份自己消费掉了，
   导致探针贴图被改成品红且没还原；`ImageGrab` 抓的是屏幕最上层窗口，
   连续截到浏览器。→ 已改为 `PrintWindow(hwnd, mem, PW_RENDERFULLCONTENT)`
   按 PID 找句柄（`probe/glb/capture_window.py`）。

## 顺带修好的两件事

- **`png` feature**：探针缺它 ⇒ 没有 PNG 解码器 ⇒ 外链贴图**永远停在 `Loading`、
  且不报错**。教训：**Bevy 缺加载器 ≠ 报错**。
- **`AmbientLight` 误用**：Bevy 0.19 里它 `#[require(Camera)]`
  （是"挂在相机上的覆盖值"），我当全局光 spawn ⇒ 每帧 `render graph` 警告
  + 凭空多一个假相机。改用 `GlobalAmbientLight`（Resource）。

# TODO

- [x] **地形正确渲染了**（有截图实证）。累计修掉 **6 个真缺陷**：
  1. `VoxelPalette::is_opaque` 把空气当实心 ⇒ 所有面被剔、零几何
  2. 空网格也建 `Mesh3d` ⇒ `slab_allocator: Use-after-free`
  3. `CameraPlugin` 从未注册 ⇒ 场景里只有 egui 的 `Camera2d`
  4. `near: -500` 把地形裁碎
  5. `VoxelNames` ID 偏移 ⇒ `grass` 解析成空气
  6. **`AtlasImage` 用 0-based 下标查 1-based `VoxelId`** ⇒ 贴图整体错位一格、
     最后一格越界 ⇒ `tile_index(..)?` **静默剔掉整排面** ⇒ 画面成片黑洞
- [x] **纸片人的机制全部就位**（35 项单测绿）：
  - `spritesheet.rs`：真实 CC0 素材（`assets/sprites/hero.png`，128×48，`AssetServer` 加载）
    + **数据驱动的 `SheetLayout`**（格子边长 / 行数 / 每行帧数 / 帧率 / 是否翻转）
  - `Facing::{South,North,East,West}` → 行号映射（朝西复用朝东行 + 水平翻转）
  - `Movement`（表现层位置 / 速度 / 期望方向）+ `DemoPatrol`（演示巡逻）
  - `advance_animation`（按朝向选行、按速度决定走不走、按帧率翻帧）
  - `sync_sprite_rect`（写 `rect` 与 `flip_x`）+ `sync_sprite_transform`
- [x] **精灵确认能渲染了**（截图里能看到那个 16×16 的描边小英雄，在画面正中）。
  `Sprite` 在 Bevy 0.19 的 required components 是 `Transform, Visibility, VisibilityClass,
  Anchor` —— **没有缺组件**，之前"完全不显示"是被相机取景/缩放掩盖了。
- [x] **`Sprite::custom_size` 其实一直是好的（这条曾误判为 bug）**
  **当时的"实证"错在测量方法**，不是代码：
  - 只测了 `2.2 → 20` 这一段小范围 ⇒ 比例误导，得出"不生效"；
  - 用**两次截图的差异包围盒**量精灵尺寸 ⇒ 差异只捕捉**变化环**，不是精灵本身。
  改成大幅程测（`100 → 59px`、`250 → 147px`，**精确线性**）后立刻清楚：
  `custom_size` 一直生效，且与像素数严格成正比。
  **教训**：不要把"我的测量结果不符合预期"直接升级成"引擎有 bug"。
- [x] **斜 45° 等距视角生效**（用户要求）。相机位置 `(0, 62.2, 62.2)` ——
  y 与 z 相等，正是 45° 的特征。`pitch_degrees` 本来就是 `CameraConfig` 里的数据，
  改一个数就够；顺带把理由写进了文档注释：`sin45 = cos45`，所以方块的两个可见面
  （顶面与侧面）在屏幕上被压缩的比例相同，格子看起来是"正"的。
- [x] **⭐ 等距（斜正方形）取景：加了「方位角」**（用户反馈"应该是斜正方形块组成的地面"）
  **根因**：`CameraConfig` 只有**俯角**，方位角恒为 0 ⇒ 相机永远正对网格 ⇒
  地面格子看起来是**正正方形**，方块只有一个面可见。
  **光调俯角永远得不到等距** —— 必须绕竖直轴转方位角。
  加了 `azimuth_degrees`（默认 **45°**）：
  ```text
  offset = distance · ( sin(az)·cos(pitch),  sin(pitch),  cos(az)·cos(pitch) )
  ```
  检算：`pitch 45° / az 0°` ⇒ `(0, d·0.707, d·0.707)`（与改造前一致）；
  `pitch 45° / az 45°` ⇒ `(d·0.5, d·0.707, d·0.5)` —— 相机落在 `+X+Z` 象限，
  世界 `+X`/`+Z` 在屏幕上分别指向右下 / 左下，**格子成菱形**。
  **精灵也跟着转**：`Transform::from_rotation_y(-azimuth)`，否则精灵会**侧对相机**
  （看起来像一张纸的边）。
  **加载中心也跟着走**：`CameraConfig::center_chunk()`，把"取景"与"加载范围"
  用同一个值绑定 —— 这条不变量必须永远成立：**看得见的地方一定有地形**。
- [x] **⭐ 动画其实一直是坏的：`sync_sprite_rect` 查错了组件**（用户反馈"移动没有动画"）
  **根因**：第 5 轮把角色组件从 `Sprite` 换成了 `SpriteMesh`（为了 `alpha_mode: Blend`），
  但 `sync_sprite_rect` 的查询**没跟着改**，还是查 `Sprite`。于是：
  - `advance_animation` 一直在正常推进 `ActorVisual::frame`（单测是绿的）；
  - `rect` **从没被写进去过** ⇒ 画面上角色**永远定格在第 0 帧**。
  **为什么测试没抓到**：原来那条测试是**自己 spawn 了一个 `Sprite`** ——
  测的是测试搭的场景，不是产品代码的真实形状。
  **修法**：查询改成 `SpriteMesh`（保留 `Sprite` 兼容分支），并加一条
  **走真实生成路径**的回归测试（父实体带 `ActorVisual` + 子实体带 `SpriteMesh` + `ChildOf`）。
  **并且验证过它能抓到 bug**：临时禁掉 `SpriteMesh` 分支 ⇒ 测试失败
  （`帧矩形没被写进 SpriteMesh ⇒ 动画在画面上不会动`）。**测试必须验证它会失败。**
- [x] **地形扩到 7×7 区块（224×224）**。原因是斜视的几何：
  原来 3×3（96×96）太小，45° 下相机沿视线很快越过地形尽头、直接看到清屏色。
  **边长必须显著大于"视野能看到的距离"**，否则斜视必然看到边界。
- [x] **修掉根因：图集 UV 用了"格边界"而不是"格内部"** ⭐ 本轮最关键的 bug
  **症状**：整片地形是棕色色块，看不出 3D；草方块顶面本该是**绿色**，却渲染成侧面那张棕色。
  **定位过程**（每一步都是量化证据，不是猜）：
  1. 直方图显示两个可见面的亮度只差 **12%** ⇒ 没有体积感（这是"看起来一堆色块"的**数学定义**）。
  2. `偏绿像素 (G > R+15) = 0.00%` ⇒ 顶面根本没显示成绿色（而它占 88% 画面）。
  3. 把**顶面那几张图集格**单独涂红 ⇒ **88.2% 的画面变红** ⇒ 顶面**确实在渲染、且占绝大多数**。
     所以不是几何问题、不是剔除问题（`cull_mode: None` 下依然 0% 绿）。
  4. 网格里插探针：`PROBE +Y here=VoxelId(1) face=Top tile=Some(1)` 出现 **23183 次**
     ⇒ 面生成了、格号也对。
  5. 图集本身查像素：`grass 顶面格=1 像素=(104,156,68)` 是绿 ⇒ 图集也对。
  6. **把顶面顶点的 UV 反算回像素**：
     `uv=[0.5, 1.0] -> 像素(32,0) -> 格(列2,行0)` ← **溢到隔壁格了**。
  **根因**：UV 用格的**边界**算（`u1 = 32/64 = 0.5`），右边界换算成像素正好是 `x = 32`
  —— **那是右边邻居格的第 0 个像素**。`nearest` 采样把邻居颜色混了进来，
  于是**每一格都被右邻居与下邻居污染**：草顶格被旁边的泥土格染成棕色。
  **修法**：新增 `AtlasImage::tile_uv`，采样区间取**纹素中心到纹素中心**
  （两端各内缩 `0.5 / 图集边长`）。修后 `偏绿像素` 从 **0.0% → 90.2%**，
  主色 `(99,143,67)` 正是草顶的绿。
  **教训**：这个 bug 的可怕之处是**所有"结构"测试全绿**（面在、法线对、绕序对、UV 在图集范围内），
  而颜色就是不对。**"UV 合法"和"UV 正确"是两件事** —— 断言要落到"落在哪一格"上。
- [x] **面明暗改为烘焙进图集**（不再靠光照）
  两条路都试过并失败，记录在这里避免重走：
  | 做法 | 结果 |
  |---|---|
  | 真实光照（方向光 + 环境光） | 难调。实测顶面/侧面只差 12%；把主光调大又整片过曝 |
  | `unlit` + 逐顶点色 | **不可行**：`StandardMaterial` 不读顶点色（`bevy_pbr` 里没有 `ATTRIBUTE_COLOR`），所有面同一亮度 |
  最终做法：图集本来就有"每个方块三张格"（顶/侧/底），在**生成那三张格时**把明暗乘进去
  （`shade` 模块，`FaceShade::MC_STYLE`：顶 `1.0` / 侧 `0.62` / 底 `0.45`）。完全确定、逐面精确、零运行期开销。
- [x] **纸片人尺寸解决了**（`custom_size` 是生效的，之前是我测错了）
  **结论**：`custom_size` **一直是生效的**，而且是**线性**的。之前误判是因为：
  - 只在 `2.2 → 20` 这个**小尺寸区间**测，比例会误导；
  - 用"两次截图的差异包围盒"当指标 —— 差异只捕捉**变化环**，不是精灵本身。
  **实测标定**（有地形、含全部精灵）：
  | `custom_size` | 角色高度 |
  |---|---|
  | 100 | 59 px |
  | 250 | 147 px |
  250/100 = 2.5，147/59 = 2.49 ⇒ **完全线性**。
  **但比例与投影算术不符**：`viewport_width 62` / 1280 像素 ⇒ 该是 20.6 像素/单位，
  实测约 **2.4 像素/单位**，差约 8.6 倍。我核对过 `Projection`（日志确认
  `FixedHorizontal { viewport_width: 62.0 }`、`scale: 1`）与 `ScalingMode` 的实现
  （确实是 `width = viewport_width`），**没有找到解释**。
  ⇒ 所以 `SheetPlacement::world_size` 的文档注释里**写明了它不是世界单位**，
  并注明"改之前先用截图标定"。现在值是 `115.0`（约 130 像素高），
  已挪进 `world.ron` 的 `sprite` 段，**不在代码里写死**。
- [x] **换了 `Sprite` → `SpriteMesh`**
  Bevy 0.19 里 `SpriteMesh` 是独立组件，比 `Sprite` 多一个 **`alpha_mode`** 字段
  （默认 `Mask(0.5)`，会把半透明边缘丢掉）。精灵素材有抗锯齿边缘，
  所以显式用 `SpriteAlphaMode::Blend`。
- [x] **地形里的"黑色缺口" —— 已解决（结论：一直是清屏色，见下文 round 3 的破案）**
  **症状**：绿顶 + 红/棕台阶立面之间，有约 **7.3%** 的画面是深灰 `(43,44,47)`，
  **看起来像一把渲染坏了的黑方块**。
  **当时排除的（都有实测）**：
  - **不是着色错误**：把图集按面朝向涂三色后，**顶面占 87.8%**、侧面 2.3%、
    底面 0.3%、三色之和 **90.4%**，剩下 7.3% 就是那块深灰。
  - **不是世界边缘**：把加载半径从 3 提到 **5（121 个区块）**，暗块反而
    从 7.5% 涨到 **8.4%** ⇒ 与地形边界无关。
  - **不是近平面裁剪**：`OrthographicProjection::default_3d()` 的 `near = 0.1`，
    相机距地形约 90 单位 ⇒ 深度范围绰绰有余。
  - **不是区块边界**：黑块不规则，且不沿 32 格网格对齐。
  **当时的关键推理是错的**："地形起伏只有 ±2 格，所以不可能是向下的洞" ——
  方向没错（确实不是洞），但**结论跳到了"几何缺失"**，而真相是"看到了地形之外"。

- [x] **网格化被证明是正确的（本轮推翻上一条的结论）**
  写了一条**逐列顶面**测试（`terrain_gap_tests.rs`）：拿真实 `world.ron` 高度图，
  遍历**相机视野覆盖的 3×3 个区块**（>2000 列），对每一列断言
  "该列最高实心块的上表面被某个朝上的四边形覆盖"。

  **结果：0 列缺失。** 所以**网格里没有缺口** —— 上一轮"几何真的缺失"的推论是错的。

  这条测试本身踩了三个坑，都记在文件里：
  1. **假阴性①**：查"顶点是否落在格中心"。贪心网格会把同一层面**合并**成
     一个大四边形，只记 4 个**角**。→ 1024 列全被判缺失。改成**查点是否落在四边形内部**。
  2. **假阴性②**：以为面的 `y` 是"实心格 `y + 1`"（想成上表面的高度）。
     实测与 `store` 对照后才确认：**面的 `y` 就等于实心格自身的 `y`**。
     → 又是 1024 列全判缺失。**教训：先打对照表确认坐标约定，再写断言。**
  3. **假阳性**：`surface_height` 可能低于 `floor_y`，那种列的最高实心格是
     **地板**而不是地表，它的上表面被上方土壤盖住是**正确**的。
     → 只对 `y >= 0` 的列断言。
- [x] **那 5.5% 清屏色是"视角看到地形之外"** ← **这一条的结论已被推翻，见下**
  排除 HUD 后清屏色占 **5.5%**（不是 7.5%，之前把 HUD 的深色背景算进去了）。
  **俯角判别**：45° 时 5.5%，抬到 **75°** 时降到 **4.17%**，而且暗块被**推到画面底边**
  —— 正是"斜视看到地形近端之外"的位置。
  ⇒ 当时判为**取景问题，不是渲染缺陷**。
- [x] **⭐⭐ 黑区破案了：它一直是清屏色，只是我没改成功**（round 3）
  ## 真相
  那片 `(43,44,47)` 是 **Bevy 的默认清屏色**。约 **7.3%** 的画面是"相机看到了地形之外"。

  ## 为什么查了这么多轮都没查出来
  **因为我两次试图改它都无效，而我没意识到"改不动"本身就是最重要的线索。**
  我一度把它当成"渲染出来的深色几何"，于是去查网格、查图集、查光照、查面明暗 ——
  **全在错误的方向上**（网格确实完整、图集确实对、材质确实是 unlit）。

  ## 三种写法的实测结果
  | 写法 | 结果 |
  |---|---|
  | 往相机实体插 `ClearColor(color)` 组件 | ❌ **无效**（组件插上了，但没人读） |
  | `app.insert_resource(ClearColor(..))` 世界资源 | ❌ **无效** |
  | **`Camera { clear_color: ClearColorConfig::Custom(..) }`** | ✅ **生效** |

  默认值是 `ClearColorConfig::Default` = "取世界 `ClearColor` 资源"，
  而那条路在这个项目里没走通；`Custom(..)` 显式指定就绕开了资源查询。

  ## 决定性判别手段（以后怀疑"这是背景吗"直接照做）
  **把清屏色设成洋红 `(1, 0, 1)` 重编译再截图**：洋红像素应占 ~7.3%。
  实测洋红 **66309 像素 = 7.29%**，而原来那块 `(43,44,47)` **一个不剩**。

  ## 我在这上面犯的两个方法论错误（写下来防复发）
  1. **一次"证伪"实验没生效，我却当成结论用了。**
     洋红实验（第一次）返回 **0 个洋红像素**，我据此写下"黑区不是清屏色"。
     但 **0 个洋红像素恰恰说明实验没生效** —— 如果 `ClearColor` 真控制了背景，
     天空区域**必然**全是洋红。**"预期现象完全没出现"要先怀疑实验本身，不是先推翻假设。**
  2. **改不动的效果，先问"我这个改法有没有被读到"。**
     两种写法都没效果，我却继续换假设（去查几何），而不是**换一种写法验证同一个效果**。
     round 3 做的正是后者 —— 一次就破了。

  ## 副产品
  - 顺带确认**网格是完整的**：四个完整性测试 + `ChunkMeshIndex.loaded` = **49/49**。
  - 修掉一个**我自己在 round 2 引入的真缺陷**：`is_probably_empty` 采样区块中心
    （高度图世界里永远是空气）⇒ 49 个区块只建了 39 个网格。

- [x] **相机取景（斜 45°）已定**：`pitch 45° / distance 88 / view_width 62 / 环境光 900`。
  地形颜色正确（草方块侧面 `134,110,76` 的棕色）、格子与台阶都看得见。
- [x] **截图验证防陈旧 —— 做成脚本了**（`scripts/shot.ps1`）
  上一轮我把 BRP 截图写失败、读到的却是**上一次的旧 PNG**，于是误判"截图返回缓存"。
  现在把"防陈旧"做成机械检查，不再靠人记得：

  1. 截图前**先删目标文件** ⇒ 旧帧不可能被误读；
  2. 截图后确认**文件真的出现**且非空（等最多 5 秒）；
  3. 打印 **sha256** ⇒ 改了可见的东西之后哈希必须变；没变就是"改动没有可见效果"；
  4. 进程没跑 / 端口没监听 / 请求失败 ⇒ **明确的退出码**（1 / 2），
     并在 stdout 写"**不要**在这时读图"。

  **实测**：没进程时退出码 1 并指明原因；同一帧连拍两次哈希相同
  （`35666A1A35B4CF07`）—— 说明截的确实是当前帧。

  **顺带修了一个真缺陷**：`CameraConfig` 是**资源**，我上一轮只给组件加了
  `#[reflect(Component)]`。资源要 `#[reflect(Resource)]` + `register_type`，
  否则 BRP 报 `Unknown resource type` —— **又一种静默失败**（你以为读不到是因为别的原因）。

- [x] **已修掉的渲染缺陷**（都写进了代码注释，避免再踩）：

      1. **`VoxelPalette::is_opaque` 把空气当实心** —— "没登记过的 ID 按实心处理"这条

         *安全默认* 把 `VoxelId(0)`（空气）也吞了，于是贪婪网格化认为**每个面都被挡住**，

         所有区块产出空网格。表现：**地形完全画不出来且零报错**。

         教训：安全默认值要按"最常见的输入"选——世界里最多的是空气。

         已在 `docs`/代码里留下注释，`world::voxel` 有回归测试。

      2. **空网格也建 `Mesh3d` 实体** —— 全埋住的区块产出空顶点数组，塞进 `Assets<Mesh>`

         会让 Bevy 报 `slab_allocator: Use-after-free`（每块 2 条），**不崩溃、只是画不出**。

         现在空网格直接返回 `None`，不建实体。

      3. **`CameraPlugin` 从未被注册** —— 写了相机插件却在 `VoxelRenderPlugin` 里没 `add_plugins`，

         于是战场上**只有 egui 的 `Camera2d`**。诊断日志一打就露馅（场景里只有一个相机）。

      4. **`near: -500` 把地形裁碎** —— 以为"正交下 near/far 只是裁剪面，给足就行"，

         结果深度映射反了，地形被裁成碎片和斜条纹。**默认值是对的，别乱动。**

      另外两条已确认无关的：egui 相机清屏（已改成 `clear_color: None`）、

      `ScalingMode::Fixed { height: 0.0 }` 退化投影（已改成 `FixedHorizontal`）。

- [x] **`raycast_voxels` 的边界疑点：补了边界测试，未能复现**（原本记成"有 bug"）
  原记录说 `t_max` 初值"用的是 `origin.floor()` 之后的绝对边界而不是相对偏移，
  会让起点不在格中心时多走/少走一格"，**并要求用非中心起点补测试再修**。

  **照做之后没能复现**：从格边界起射（`x = 2.0`）、朝正负两个方向、
  靶子分别放在起点格与下一格 —— 四条断言（命中位置 + **命中面法线**）**全部通过**。
  `origin.floor()` 与方向相关的 `boundary` 组合起来，对边界起点给出的仍是正确结果。

  **所以这条注释里的"bug"是错的**，已在 `raycast.rs` 源码注释里标注"别再照着它改"。

  **顺手清掉一个真问题**：DDA 循环里有一句 `println!`（每步都打印）——
  那是调试残留，**在热路径上**，已删。

  **留下的资产**：4 条边界回归测试（正负方向 × 起点格是否实心）。
  它们现在全绿，但把"边界约定不能随便改"钉住了。
  **教训**：TODO 里写的"已知 bug"也要当成**待验证的假设**，不是结论。
- [x] **角色精灵已上屏**（`ActorSheet` 生成 + `ActorVisualIntent` 挂上 + 真实渲染）。
  截图里能看到那个描边小英雄站在地形上；走路动画也修好了
  （根因是 `sync_sprite_rect` 查错了组件，见上文）。
- [ ] **空间维度**（**这一条要人拍板，见下方 344 行那条的详细说明**）
  角色**没有位置组件**，所以"移动动画"目前没有真实位移可播。
  **不能靠猜**：`docs/combat-design.md` 定义的战斗是**位置无关**的，
  先要定"位置要不要进战斗模型"。

      加载期"字符串 → 词汇 ID" + 六项启动期检查（未知名 / 未使用池 / 未知技能 / 重复 id /

      怪物没 AI / 漏写 `role`）；**玩家也是内容**，与怪物共用同一条生成路径

- [x] **引擎级配置**：`CombatConfig`（随机种子）、`VirtualTimeConfig`（单帧虚拟时间上限，

      由 `TimeControlPlugin` 落地到 `Time<Virtual>`）；其余可调量都在 `.ron` 里

- [x] **L2 接线**：`content`（读文件 + 反序列化 + 注入 + 生成角色）+

      `presentation`（只读相位 / 可用技能 / 行动进度 / 战斗日志）

- [x] **HUD**：左上 PC 状态面板（**内容里配几个池就有几行**，含标题 / 数值 / 进度条）、

      左下技能列表（每帧按 `AvailableSkills` 重建，点击发 `CastRequest`）；

      自带 CJK 字体（Bevy 默认字体没有汉字字形，会渲染成豆腐块且不报错）

- [x] **技能归属**：`Skill::roles`（空 = 谁都能用）——不然哥布林的招式会出现在玩家技能栏里

- [x] **体素世界数据层**（`axiom::world`，**纯数据无渲染**）：`ChunkPos`/`Chunk`（32³）、

      两层存储（程序化 value-noise + 已加载区块，改动只落区块）、`TerrainParams`/`NoiseParams`、

      `VoxelStore::{get,set}` + `ChunkDirtyMessage`、**DDA 射线**（`raycast_voxels`）

- [x] **体素表现层**（`prime::voxel_render`）：程序化方块图集（颜色 + 图案，无需美术资源）、

      **贪婪网格化**（同贴图矩形合并）、地形材质 + 平行光、俯视角正交相机（俯角 / 距离 / 视野可配）

- [x] **角色 2D 纸片人**（`prime::actor_render`）：**真实 CC0 素材**
  （`assets/sprites/hero.png`，`AssetServer` 加载）
  + **数据驱动的 `SheetLayout`**（格子边长 / 行数 / 每行帧数 / 帧率 / 朝向行映射 / 翻转）
  + `Movement` + `Facing` + 帧推进 + `SpriteMesh`（`Blend` alpha）。
  **摆放参数也在内容里**（`world.ron` 的 `sprite` 段：`world_size` / `height`）。

- [x] **世界内容化**：`assets/data/world.ron`（地形参数 + 方块表 + 纸片人摆放，名字 → ID 在加载期完成）

- [x] **守卫与测试全绿**：`10/10`、**192 个测试**

## 素材（2026-10-01 已下载到 `D:\AI\assets`，全部 CC0、无需署名）
- 四个 Kenney 包我核过页面标签 + 下载解包验证，**结论与预想有一处重要出入**：
- ✅ **`ui-pack-adventure`（130 个 2D UI 元素）** —— 唯一能**端到端直接**用上的。
  有按钮 / 面板 / 滑条 / 边框，正好替换 HUD 现在的纯色矩形。
- ⚠️ **`platformer-kit` / `modular-cave-kit` 是 3D 模型包，不是贴图包。**
  原以为能拿它们的贴图当方块用——**不行**。解包后 `Models/Textures/variation-a.png`
  是 512×512 的**平色渐变调色板**（只有左上角一小块表面细节），
  表面观感全靠**模型几何**而不是贴图 ⇒ 与我们"体素 + 图集"的架构**不通用**。
  可用的只有：拿它**取色**，喂给现有的程序化图集（草 / 雪的色阶）。
- ⚠️ **`roguelike-characters`（450 张 16×16 角色）是静态单帧、侧视图、无走路帧。**
  当"角色种类库"很好，但**满足不了"移动有动画"**。
  `roguelikeChar_transparent.png` 是 918×203 的**索引色**图（`P` 模式），用前要转 RGBA。
- **真正能交付"走路动画"的是这两个**（我从 OpenGameArt 直接下到，同 CC0）：
  `characters/walkcycle-various-8char.png`（768×474，8 角色 × 多帧，**4 视图**）与
  `characters/hero-base-16x16.png`（128×48 = **8 列 × 3 行** 的 16×16 网格，四方向 + 走路帧，裸模）。
  建议：**纸片人用后者的走路帧**（引擎已有 `Motion::{Idle,Walk}` + `ActorSheet`，接真实表即可），
  **`roguelike-characters` 当静态形象库**（NPC 站桩 / 头像）。

## ✅ 验证结论：**贴图确实被用上了**（附一处对源码的纠正）

### 用户的判断经 A/B 实验证实
把探针里所有 `colormap.png` 换成**纯品红 `(255,0,255)`**，模型区域的颜色随之变成
**`(49, 5, 49)`**（暗品红）。

⇒ **贴图在用**，不是"没加载"。而且被**乘性压暗到约 19%**：

| 输入 | 输出 | 比例 |
|---|---|---|
| `(255, 0, 255)` | `(49, 5, 49)` | **19%** |
| `(90, 96, 123)` | `(14, 15, 21)` | 16–22% |
| 拿掉贴图（纯白 albedo 无贴图） | `(49, 49, 49)` | 正常 |

**这个 ~19% 的固定乘性因子**才是真正的问题，
而"贴图路径"只影响**贴图能不能找到**（那是探针 `png` feature 的问题，已修）。

### ⚠️ 对源码的纠正（我之前的推断错了）
我曾推断「Bevy 把 glTF 的 `uri` 解析成相对 **assets 根**」。**读了源码，不是这样**：

```rust
// bevy_gltf loader：resolve_embed_str → resolve_internal
let mut base_path = PathBuf::from(self.path());   // "models/gate.glb"
if replace && !self.path.to_str().unwrap().ends_with('/') {
    base_path.pop();                               // → "models"
}
// 再拼上 "Textures/colormap.png" ⇒ "models/Textures/colormap.png"
```

**Bevy 是相对 GLB 自己解析的**（符合 glTF 规范）。
所以探针里正确的位置本来就是 `probe/glb/assets/models/Textures/colormap.png`。

把贴图同时放到 assets 根是一种**可行的规避**，但要注意：

> **三个套件的 GLB 都写 `uri: "Textures/colormap.png"`，所以按 assets 根解析时
> 它们会共用同一张图** —— `assets/Textures/colormap.png` 只能同时服务一套。
> 想三套都对，得用 `AssetServer` 手动加载并注入材质，而不是靠目录约定。

### 还没解决的：那个 ~19% 的乘性压暗
已排除（都有实测）：光照强度（400 倍无变化）、环境光（250 倍无变化）、
光位置（`LIGHT_POS` 无效）、金属度、`base_color`、背面剔除、near/far 裁剪、
几何法线（拿掉贴图就正常）。

**剩下的方向**：贴图采样出来之后到写进屏幕之间，有一次固定的 ~19% 乘法。
最像 **`baseColorTexture` 的色彩空间处理**（`sRGB→线性` 的典型量级就是 0.2 上下）。
`bevy_gltf` 源码里明确算了 `is_srgb = !linear_textures.contains(&texture.index())`
并传给 `Image::from_buffer` —— 下一步该确认 `linear_textures`
会不会把这些**本该是 sRGB 的 baseColorTexture 误判成线性**。

### 我的方法错误（记下来）
1. **对照实验两边都要从干净状态起。** 我的 A/B 脚本直接抓"已经在跑的实例"，
   而那可能是上一轮实验的残留（贴图还是品红）⇒ A、B 结果完全相同，白跑一轮。
2. **备份文件被脚本自己消费掉了**（`write_bytes(backup.read_bytes())` 之后就 `unlink`，
   而恢复没执行完）⇒ 探针的贴图被我改成品红且没还原，最后要从 prime 拷回来。
   **改素材的脚本一定要先把备份放到别的地方，别和原地恢复耦合。**
3. `ImageGrab` 抓的是**屏幕最上层**窗口，不是我要的那个 ⇒ 连续截到浏览器。
   改用 `PrintWindow(hwnd, mem, PW_RENDERFULLCONTENT)` 按 PID 找句柄才可靠。

## GLB 发黑：**已定位到 glTF 贴图这一环**（决定性实验）

### 三个决定性实验（都是实测数值）

| 实验 | 结果 | 排除了什么 |
|---|---|---|
| **拿掉贴图** | 模型变成 **`(49,49,49)` 灰，正常** | 光照、法线、几何**全都正常** |
| **光位置放很高** (`1,2,0.6` → `2,20,2`) | 平均亮度 `(70,90,116)` → `(69,89,115)`，**无变化** | 不是光方向（平行光只看方向、不看位置，符合预期） |
| **拿掉贴图后换纯色贴图** | 见下表 | — |

### 纯色贴图测量：**渲染管线本身是正确的**

用我自己造的纯色 PNG，材质设 `unlit`，量渲染出的像素：

| 贴图字节 | 实测渲染值 | 结论 |
|---|---|---|
| `(90,96,123)` | **`(89,95,120)`** | ✅ 几乎完全一致 |
| `(184,80,30)` | `(170,79,36)` | ✅ 接近 |
| `(255,255,255)` | `(196,196,196)` | 略暗（可能是 tonemap/亮度） |

⇒ **色彩空间、贴图上传、着色器采样都是好的。**

### 🎯 关键实验：**同一张图，手动加载正常，经 glTF 就变暗**

| 贴图来源 | 渲染值 |
|---|---|
| GLB 自带的 `colormap`（经 glTF 加载） | **`(14,15,21)` 几乎全黑** |
| **同一个文件**，用 `assets.load()` 手动加载 | **`(89,95,120)` 正确** |

**同一张 PNG、同一个 Bevy 版本、同一个渲染管线、同一个 `StandardMaterial`，
唯一的差别是"贴图是 glTF 加载器给的还是手动加载的"。**

而 `block-grass.glb` 的 UV（`u 0.469-0.969, v 0.525-0.725`）采样的区域，
用 PIL 算出来均值是 **`(179,153,151)` 亮色**，最多的是纯白 `(255,255,255)`。
手动加载这个文件渲染出 `(89,95,120)`，也证实了区域是亮的。

⇒ **结论：问题在 `bevy_gltf` 给这张贴图设置的某个属性上。**

### 下一步要查的（明确）
对比"glTF 加载出来的贴图"与"手动加载的贴图"的差异：
1. **`Image::texture_descriptor.format`** —— 是不是 `Rgba8Unorm` 而不是 `Rgba8UnormSrgb`
   （格式不对 ⇒ 少一次 sRGB 解码 ⇒ 画面偏暗，正是观测到的现象）。
   *更新*：prime 的贴图转储里有 `512x512 Rgba8UnormSrgb`，但**那也可能是 UI 图集**，
   必须按 `AssetId` 对上 GLB 材质引用的是哪一张。
2. `Image::sampler`（bglTF 可能设了不同的 sampler）。
3. `Image::asset_usage` / mipmap 设置。

**排查方法**：在实验里把"glTF 材质实际引用的那个 `Handle<Image>`"的
`texture_descriptor` 打出来，与手动加载同一文件的对照 —— **按 `AssetId` 对齐**，
不要再靠"列表里有一张 512x512"这种间接证据。

### 本轮的方法论教训（又一条）
**"同一张图、两条加载路径、结果不同"是最有价值的对照。**
我在这之前做了十几次"改一个参数看画面"的实验，都不如这一条有信息量 ——
因为它一次性把"渲染管线"整条排除掉了，把搜索空间缩到"glTF 的贴图处理"一个点。

## GLB 发黑：本轮排查结果（含一条**纠正**）

### ⚠️ 先纠正我之前写错的
我曾在 TODO 里判断"**外链贴图没加载**、`Loading` 永远不完成"。
**那只对探针成立，对 prime 不成立。**

prime 的贴图转储（`[展示台] 贴图转储`）显示：
```text
[9] 512x512 格式=Rgba8UnormSrgb   ← colormap，加载成功，格式正确
```
⇒ **prime 里贴图是好的、色彩空间也是对的**。我那条结论是**从探针错误外推的**。

### ✅ 真 bug：探针缺少 `png` feature
`probe/glb` 的 Cargo.toml 原本是 `features = ["bevy_gltf", "debug"]`。
外链贴图是 PNG，**没有 `png` 就没有 PNG 解码器**，而 Bevy 的表现是：

- **不报错**
- `load_state` **永远停在 `Loading`**
- `Assets<Image>` 里**永远不出现**这张图

⇒ 探针里模型黑的原因就是这个。**已修**（feature 加上 `png`）。

**这条值得单独记：Bevy 缺加载器 ≠ 报错。**
以后遇到"外链资产加载不出来、又完全没有错误日志"，第一件事就该查 feature。

### 还没解决的（prime 里模型仍偏暗）
**关键矛盾**：材质字段全部正常（`base_color` 纯白、`unlit=false`、
`metallic=0`、有贴图且格式是 `Rgba8UnormSrgb`），但：

| 实验 | 结果 |
|---|---|
| 平行光 `12_000 → 2_000_000`（400 倍） | **亮度几乎不变** |
| 环境光 `60 → 15_000`（250 倍） | **亮度几乎不变** |
| 拿掉贴图（探针里） | **模型变亮成灰 `(49,49,49)`** |
| 强制 `base_color = 白`、`metallic = 0` | **无变化** |
| 正交 near/far | 远在范围内，**不是裁剪** |

**"光照完全不起作用"是最强的线索。** 一个几何体完全不吃光，
通常意味着**法线朝向反了**：这些 GLB 是 `doubleSided: true`，
如果法线朝内，我们看到的其实是**背面**，光照按朝外法线求值 ⇒ 永远背光 ⇒ 发黑，
而高光（另一条路径）照常出现 —— **正好符合观测**。

**下一步（明确的实验，本轮没跑完）**：
把这些 GLB 的**法线翻转**（或 `cull_mode = None` + `double_sided` 对照）看画面是否变亮。
在 `probe/glb` 里最省事：`diagnose_materials` 的开关里已经列了 `cull_mode = None`。

### 方法论教训（本轮）
1. **不要把"最小探针里的现象"直接当成"产品里的现象"。**
   探针缺 feature、缺插件，它的行为**不代表 prime**。我因此浪费了很多轮。
2. **截图工具会骗人。** `SetForegroundWindow` 在这里不可靠，我两次截到别的窗口。
   截图后要**先确认窗口标题**。
3. **诊断系统本身要能用 `Option<T>` 包参数**，否则它一 panic 就把整个 App 干掉
   （本轮犯了两次，每次都表现为"程序莫名退出"）。
4. **`0x08`（退格）会混进 PowerShell here-string 里的 ``。**
   `Cargo.toml` 因此两次报 `invalid comment character`。
   **写含 ``/反斜杠的内容一律用 `write`/`edit` 工具，不要用 here-string。**

## ✅ 已修：`Camera` 无渲染图警告（Bevy 0.19 的 `AmbientLight` 语义变了）

### 症状
每帧刷一条：
```text
WARN bevy_render::camera: Entity 461v0 has a `Camera` component,
     but it doesn't have a render graph configured.
```

### 根因（用 BRP 查出来的，不是猜的）
`world.query(Camera + Name)` 显示**三个**"相机"实体：

| 实体 | 名字 | order | clear_color |
|---|---|---|---|
| …834 | **`terrain-ambient`** | 0 | Default |
| …793 | `battlefield-camera` | 0 | Custom(天空蓝) |
| …833 | `egui-camera` | 1000 | None |

**`terrain-ambient` 根本不是相机** —— 它是环境光实体，却带着 `Camera`。

原因在 `bevy_light::ambient_light`：

```rust
/// It can be added to a camera to override `GlobalAmbientLight`...
#[require(Camera)]
pub struct AmbientLight { .. }
```

Bevy **0.19** 把 `AmbientLight` 改成了**挂在相机上的覆盖值**（并且 `#[require(Camera)]`）。
我按老习惯把它当"全局环境光"spawn 成独立实体 ⇒ **凭空造出一个没有渲染图的裸相机**。

### 修法
全局环境光改用 **Resource** 形式 `GlobalAmbientLight`：
```rust
*ambient = GlobalAmbientLight { color, brightness, .. };
```
（`LightPlugin` 已经插了这个资源，直接改字段即可。）

**三处都改了**：`materials.rs`、`probe/glb/src/main.rs`、`probe/glb/src/bin/texture_vs_plain.rs`。

### 验证
- `render graph` 警告：**每帧 1 条 → 0 条**
- BRP 复查：只剩 `battlefield-camera` 与 `egui-camera` 两个真相机
- 地面渲染不受影响

### 教训
**"名字是灯、内容也是灯、却带着 `Camera`"** 这种矛盾值得当场查 ——
而这次是**日志里的一条 WARN 直接指出来的**。
之前我把它当成"次要问题"跳过了好几轮，实际上它是个**真 bug**（多了一个会参与渲染的相机实体）。

## 格子地面（GLB 地板瓦片）+ 可交互相机

### 已完成
1. **`ground_style.rs`**：地面表现的**唯一决策点**（体素地形 / GLB 地板网格）。
2. **`floor_grid.rs`**：一格一个 Kenney 地牢地板瓦片（`template-floor.glb`）。
   - **尺寸换算**：`template-floor` 在 GLB 里宽 **4.0**（`POSITION` `-2..2`，是个平面四边形），
     而一格是 **1 世界单位** ⇒ `TILE_SCALE = CELL_SPACING / GLB_TILE_WIDTH = 0.25`。
   - **格线**：不画线。瓦片按 `TILE_INSET = 0.06` 内缩，露出深色衬板（`BACKDROP`）。
   - **棋盘**：同一套模型只改缩放（GLB 的材质是**共享**的，改 `base_color` 会同时改掉墙和门）。
   - `FLOOR_GRID_EXTENT` 可调边长（默认 96 ⇒ 9216 个瓦片）。
   - 4 条测试（缩放正好一格、内缩范围、默认开、范围盖住视野）。
3. **`orbit_camera.rs`**：类编辑器镜头（`ORBIT_CAMERA=1`）。
   中键平移 / 滚轮缩放（乘法）/ 右键转视角 / R 复位。7 条测试。
4. 地形与地板**互斥**：体素地形在 `GroundStyle::TileGrid` 时直接 return。

### 踩过的坑（都值得记）
1. **两个模块各自读同一个环境变量，判据写反了。**
   `floor_grid` 用 `map_or(true, ...)`（未设 ⇒ 开），`terrain` 用 `is_ok_and(...)`（未设 ⇒ 不跳过）
   ⇒ **两边都跑了**，地板被绿色地形整个盖住。
   **修法**：把决策收敛到一个类型 `GroundStyle`，环境变量只在一处读。
   *教训：同一个开关被两处解读 = 迟早不一致。*
2. **`TILE_SCALE` 的含义搞错了。** 我写成 `0.5`，心里想的是"Kenney 网格单位是 2 世界单位"——
   那把**网格单位**和**格子间距**混为一谈了。正确的推导是
   `缩放 = CELL_SPACING / GLB_TILE_WIDTH = 1/4`。
   用 `0.5` 的结果是瓦片宽 **2 格**、重叠 88% ⇒ **格子完全看不见**，全屏只剩瓦片。
   *教训：比例类常量必须写清楚"分子分母各是什么"，并让测试按定义验证。*
3. **衬板的顶面不能和瓦片同高。** 第一版 `y = -0.02`、厚 `0.04` ⇒ 顶面正好在 `0`，
   整个画面只剩深灰衬板。现在 `BACKDROP_Y = -0.10`、厚 `0.08` ⇒ 顶面 `-0.06`。
4. **`screen_basis` 的符号写反了**（方位角 45° 往右拖，注视点往 `(+x, -z)` 走）。
   测试抓到了 —— 所以那 7 条相机测试不是装饰。
5. 编译前必须 `Stop-Process voxelith-prime`，否则 exe 被锁（`os error 5`）。**又犯了。**

### ⚠️ 当前状态：格子对了，但瓦片**颜色**还是暗的
`floor_grid7.png`：格纹、尺度、疏密都正确（一格约 20 px，和参考图一致），
但瓦片本身渲染成深色 —— 这就是下面那个**尚未解决**的 GLB 渲染问题，
与"格子铺得对不对"是两件事。

**结论：地形部分已经完成；颜色问题是一个独立的、待解决的渲染问题。**

## GLB 模型渲染调研（用户要求：新开一个最小 crate 验证）

### 做了什么
1. 用户建议参考 [Kenney 的 Bevy 导入说明](https://kenney.nl/knowledge-base/game-assets-3d/importing-3d-models-into-game-engines)：
   Bevy **原生支持 GLB**，`AssetServer` 直接加载，不需要转码。
2. 加了 `bevy_gltf` feature（一行），
3. 装了 **3 套** Kenney 模型到 `assets/models/`：
   - `dungeon`（**默认**，39 个，用户推荐的地牢主题）
   - `cave`（40 个）
   - `platformer`（81 个方块）
4. 新增 `voxel_render/model_gallery.rs`：**一个一个方格摆出来**。
   - `WorldAssetRoot` + `GltfAssetLabel::Scene(0)`（**Bevy 0.19 的写法**；
     0.19 不再叫 `SceneRoot`/`SceneInstance`，见 `bevy_gltf` 的 crate 文档）
   - 取景**按网格跨度算**，不写死数字（写死会让"看不全"被误判成"没加载"）
   - 环境变量开关：`MODEL_GALLERY=1` / `MODEL_GALLERY_KIT=dungeon|cave|platformer`
   - 5 条测试（清单与磁盘核对、贴图存在、间距够大、未知套件回退、行数）
5. 新增独立 crate **`probe/glb`**（自带 `[workspace]`，复用主 target）：
   最小 glTF 渲染 + 可交互相机（中键平移 / 滚轮缩放 / 右键转视角 / R 复位）。

### ✅ 结论：不是我们项目的问题
最小 probe（只有 `DefaultPlugins` + 一个 GLB，**零 voxelith 代码**）**同样偏暗**。
所以问题在 **Bevy 的 glTF 材质/贴图路径上**，不在我们的地形/相机/光照配置里。

### 实测数据（同一套灯、同一个场景）
| 物体 | 材质 | 渲染值 |
|---|---|---|
| 地面立方体 | 我们自己的 `StandardMaterial`，`base_color 0.6` 灰 | **`(108,108,109)`** ✅ 正常 |
| `template-floor.glb` | GLB 的 `colormap` 材质 | **`(18,19,25)`** ❌ 暗约 **4.5 倍** |

### 已经排除的（都有实测）
- **不是光照**：平行光 `12000 → 5,000,000`（400 倍）、环境光 `60 → 15,000`（250 倍），
  模型的亮度**几乎不变**（而地面立方体在同一实验里正常）。
- **不是贴图没绑上**：把 colormap 换成品红/白色/橙色，模型**跟着变色**
  （白 → `(57,57,57)`；橙 `(184,80,30)` → `(40,15,8)`），
  即**贴图在起作用，只是被乘性压暗到约 22%**。
- **不是特定素材**：cave / dungeon / platformer 三套都黑。
- **不是 normal 缺失**：GLB 的 primitives 有 `POSITION/NORMAL/TANGENT/TEXCOORD_0`。

### 🔍 材质转储（决定性的一步）：**GLB 的材质是 `unlit = true`**
用反射遍历 `Assets<StandardMaterial>` 打出来的实际值（`probe/glb` 的 `dump_materials_once`）：

| # | base_color | **unlit** | 有贴图 | double_sided |
|---|---|---|---|---|
| 0 | 红 | **true** | false | false |
| 1 | 红 | **true** | **true** | true |
| 2 | 洋红 | false | false | false |

**结论**：GLB 的材质（第 1 个，有贴图的那个）**是 `unlit` 的** ——
它完全不走光照，直接用贴图颜色。
这与"平行光调亮 400 倍毫无变化"**完全一致**，也解释了为什么地面立方体正常而模型不亮。

⇒ **"模型偏暗"的框架从一开始就是错的**：它不是被照暗的，
而是 `unlit` 材质直接把贴图色输出，而贴图色**在一个环节被压暗了约 4.5 倍**。

### ⛔ 我停在这里了：连续多轮没有进展
**必须如实说**：下面这些实验做完之后，我**仍然没有找出根因**，
而且已经在同一个问题上绕了太多轮。继续猜下去是浪费。

**当前确认的事实（都是实测）**：

| | 值 |
|---|---|
| GLB 材质的字段 | `base_color` 纯白、`unlit = false`、`metallic 0`、`roughness 1`、**有贴图** |
| 我们自己的立方体材质 | 同一场景下**渲染正常**（`(108,108,109)`） |
| GLB 模型 | **发黑但有大量白色高光** ⇒ 光照在算，是 albedo 出不来 |
| 平行光 `12000 → 2,000,000` | 模型**亮度不变** |
| 正交 near/far | 展示台深度 197.6，远在 `0.1..1000` 内 ⇒ **不是裁剪** |
| `probe/glb` 最小场景 | **同样发黑** ⇒ 与 voxelith 的代码无关 |

**我犯的方法论错误（值得记下来）**：
1. **把"探针正常"当成了既定事实太久**。实际上我第一次看到探针截图时它就是黑的 ——
   那个"深色钻石形状"就是模型。我错误地把它读成了背景/天空盒。
   因为这个误判，我花了大量轮次去追一个**不存在的"probe 与 prime 的差异"**。
   **教训：看截图要先用像素值确认"我看到的这块是什么"，不要凭形状猜。**
2. **反复换假设，而不是做能一次分辨多个假设的实验**。
   直到最后才做出"同一模型、两种材质、并排"的对照 —— 那才是应该更早做的。
3. `SetForegroundWindow` 在这个环境里不可靠，截图经常截到别的窗口。
   **截图后应该先确认窗口标题**（我最后才加这一步）。

**下一步的正确做法（还没做）**：
在 `probe/glb` 里把 `block-grass.glb` 的**贴图加载状态**打出来 ——
`AssetServer::load_state::<Image>(handle)` 或 `is_loaded_with_dependencies`。
"有贴图句柄"不等于"贴图加载完成"；
如果贴图**永远停在 Loading**，那块颜色就会按"未加载"处理（黑）。

### 还没查完的两个疑点
1. **`base_color` 是红色 `(1,0,0)`**。GLB 里没写 `baseColorFactor`（默认该是白）。
   红色从哪来？**这很可能就是压暗的机制** —— 如果 `base_color` 参与乘法，
   红通道乘 1、绿蓝乘 0，画面就该**只剩红色**。但实测画面是蓝灰的，
   所以更像"转储打出来的不是实际生效的那份材质"（例如存在多份、或它被后续系统覆写）。
2. `unlit` 是谁设的？项目里 `materials.rs` 的**地形材质**确实是 `unlit: true`，
   但那是地形，不是 GLB。**GLB 的 unlit 来源未确认** —— 需要查 `bevy_gltf` 的材质导入。

### 下一步（明确）
在 `probe/glb` 里**每帧**打印 `MeshMaterial3d` 指向的材质 id + `unlit` + `base_color`，
**并且和 `Assets` 里那份对照** —— 确认"转储看到的"与"渲染用的"是不是同一份。
`probe/glb/src/main.rs` 里已有一个（未被注册的）`diagnose_materials` 排查函数，
把它接上 `add_systems` 就能做"强制改材质看画面"的对照。

### 更早的猜测（保留，按可能性排）
那个约 **22% / 4.5 倍**的乘性暗化来自哪里。下一步该查（按可能性排）：
1. **`KHR_texture_transform`**：这些 GLB 的材质里带这个扩展
   （`baseColorTexture.extensions.KHR_texture_transform`）。
   Bevy 的 glTF 加载器**只支持特定扩展列表**，不支持的会被忽略 ——
   但"忽略"如果导致 UV 或色彩空间处理不同，就可能整体偏暗。
2. **glTF 贴图的色彩空间**：Bevy 是否把 `baseColorTexture` 按 sRGB 解码；
   如果按线性解码，颜色会**偏亮**（不是偏暗），所以这条可能性低。
3. **材质的 `baseColorFactor`**：GLB 里没写，默认该是 `[1,1,1,1]`；
   值得用反射打印实际值确认。

**排查方法**：在 `probe/glb` 里用
`world.query` 查 `MeshMaterial3d` 指向的 `StandardMaterial` 实际字段值
（base_color / base_color_texture / unlit），与 GLB 里写的对照 ——
而不是继续从截图反推。

## 待办
- [ ] 🟡 **AI 出图：环境就绪，ComfyUI 未装**
  已验证 `D:\AI\comfy-venv` + `torch 2.9.1+cu128`，`cuda.is_available()=True`，
  device = RTX 4070 Ti SUPER，capability (8,9)，bf16 支持。
  **镜像要记牢**：PyTorch wheel 走 `https://mirrors.nju.edu.cn/pytorch/whl/cu128`；
  清华 `pypi.tuna` 上那份 torch 是 **CPU 版**（122 MB、无 `nvidia-*` 依赖），别用错。
  pip 用清华源。**ComfyUI 本体与模型都还没装/拉。**
  用途分工：**方块贴图适合 AI 生成**（无缝可平铺）；**走路帧不适合**——扩散模型每帧独立采样，
  帧间一致性是硬伤 ⇒ 走路帧用上面的 CC0 素材。
- [x] **HUD 换成 `ui-pack-adventure` 的九宫格贴图** ✅ 已完成
  左上棕色九宫格面板、左下灰色面板、进度条换成 Kenney 圆头条（按池序号循环三色）。
  **踩坑**：Kenney 面板图块**不透明** ⇒ `BackgroundColor` 被整个盖住、改它无效；
  浅色文字压在亮面板上看不见。正解是给图块加暗色 **tint**（`ImageNode.color` 相乘）。
  （素材获取方式后来改成 `AssetServer`，见下面那条。）

- [x] **素材改成 Bevy 标准做法（`AssetServer`）—— 之前确实搞复杂了**（用户指出）
  ## 之前的做法与代价
  我用 `include_bytes!` / 转码脚本把素材**烘焙成 Rust 源码里的字节数组**：
  - `ui_theme/kenney_ui_rgba/` ≈ **2500 行**字节数组（11 个图块各一个文件）；
  - `actor_render/hero_walk_rgba.rs` **947 行**字节数组；
  - 两个转码脚本（`tools/transcode-*.py`）。

  当时的理由只有一条：**不想开 `png` feature**（一行）。代价是
  "换素材要重跑脚本再重编译"、代码库里躺着几千行没人读的字节、
  11 个小纹理而不是 1 张图集。

  ## 现在
  ```
  assets/                        # 工作区根，Bevy 的标准资产目录
    data/*.ron                   # 内容（本来就是文本，路径跟着挪了）
    fonts/NotoSansSC.ttf
    sprites/hero.png             # 角色精灵表（原来是 947 行字节）
    sprites/roguelike-characters.png
    ui/kenney-adventure.png      # UI 图集（原来是 2500 行字节）
    ui/kenney-adventure.xml      # 素材自带的坐标元数据（留着当事实来源）
  ```
  - `bevy` 开 `png`（**一行**）；
  - `AssetPlugin { file_path }` 指向工作区根的 `assets/`；
  - 精灵表与 UI 图集都走 `AssetServer::load`；
  - **转码脚本与生成数据全部删除**，`tools/` 现在是空的。

  **渲染结果与烘焙版逐字节一致**（截图 sha256 相同：`E151A8A407C77FA0`）。

  ## 三个踩到的坑（都写进代码注释了）
  1. **`AssetServer` 的资产根默认是"exe 旁边的 `assets/`"**，不是工作目录。
     报错是 `Path not found: ...\target\debug\assets\sprites/hero.png`。
     正解：`AssetPlugin.file_path` 显式指到工作区根。
     （**发布时**仍要把 `assets/` 拷到 exe 旁边，或者改成"先找 exe 旁、再退回源码树"。）
  2. **加载是异步的** ⇒ 不能在 `Startup` 里"load 完就当它到了"，
     否则拿到的是**空图** —— 角色完全不显示且不报错。
     拆成两步：`load_*`（Startup，只拿句柄）+ `materialize_*`（Update，像素到了才建表）。
  3. **`AssetServer` 参数要包 `Option`**：集成测试只装 `PresentationPlugin`，
     没有资源设施，必填参数会让那些测试在**系统参数校验**阶段就 panic。

  ## 顺带修好的：格子太大（用户反馈）
  实测标定（1280×720）：
  | `view_width` | 一格 | 观感 |
  |---|---|---|
  | `62` | 21 px | 用户："格子太大" |
  | **`150`** | **约 8.5 px** | **当前值** |
  | `200` | 6.5 px | 偏小，地形细节糊了 |

  调参时**没有反复重编译**：临时让 `CameraConfig.is_changed()` 触发重建投影，
  用 BRP `world.mutate_resources` 在线改 `view_width` 并连拍对比。
  **临时通道收尾时已全部删除**（`grep env::var` 确认过）。

  ## 已解决：格子与角色的尺寸对照 Elin 标定好了（用户给了参考图）
  ### 之前错在哪
  **两个尺度同时错了**，所以怎么调都别扭：`view_width` 62 ⇒ 一格 21 px（太大），
  而 `world_size` 115 ⇒ 角色相对格子是个巨人。
  更要紧的是**我以为 `custom_size` 的单位是像素 —— 它其实是世界单位**。
  那个"8.6 倍的未解之谜"就是这么来的：我一直拿像素去对世界单位。

  ### 参考资料（官方规格，不是我猜的）
  - [Elin 的 PCC 精灵是 32×48](https://elin-modding-resources.github.io/Elin.Docs/articles/15_Texture%20Mods/pcc)
    （每行一个朝向、每列一帧）；
  - [Elin 的地面格是 64 宽 × 32 高](https://elin-modding-resources.github.io/Elin.Docs/articles/10_Source%20Sheets/tile)
    （`_idRenderData` 的 `halfblock` / `floor_obj` 都是 64×48 画布）。

  ### 标定结果（**实测**，不是推算）
  | 参数 | 值 | 实测效果 |
  |---|---|---|
  | `CameraConfig::view_width` | **64** | 一格约 **20 px**（1280 窗口 ⇒ 约 64 格可见） |
  | `world.ron` 的 `sprite.world_size` | **26** | 角色约 **20×28 px**，约 1 格宽 |

  **这两个数是一对** —— 换取景或换素材后必须重新标定，
  `world.ron` 的注释里写了怎么标。

  ### 试过的几组（`work/shots/` 留了图）
  | view_width | 一格 | 观感 |
  |---|---|---|
  | 62 | 21 px | 用户："格子太大" |
  | 150 | 8.5 px | 格子小了，但角色几乎看不见 |
  | 20 | 64 px | 太近，只看到 20 格 |
  | **64** | **20 px** | **当前值** |

  ### ⚠️ 一条我没能验证的东西
  用 BRP 注入点击（`brp_extras/move_mouse` + `click_mouse`）与 PowerShell 物理点击，
  **都没能触发体素编辑**。所以"输入真的到达了系统"这一环**仍然只有静态覆盖**
  （7 条单测覆盖了 射线 → DDA → store → 脏消息），没有运行期证据。


## 待办

- [x] **地形已上屏**（见下方缺陷清单）

- [x] **角色精灵已上屏**（截图里有那个描边小英雄站在地形上；走路动画也修好了）。

- [ ] **空间维度**：角色**没有位置组件**（`Moving` 标记也没人写）。

      要接：位置组件 → 移动系统 → 写 `Moving` → `actor_render` 播 Walk。
      地形高度可以查（`TerrainParams::surface_height`）。
      `Targeting::NearestEnemy` 的"最近"也要等它落地才名副其实。

      **但这不是"忘做了"**：`docs/combat-design.md` 定义的战斗是**位置无关**的。
      所以先要定"位置要不要进战斗模型"，还是不猜 —— 见 `docs/OPEN-QUESTIONS.md`。

- [x] **BRP 能查到项目组件了（这条部分解决，见"未解"）**
  之前**一个项目类型都没有 `#[derive(Reflect)]`**，所以 `world.query` 对它们一律返回空。

  **做了什么**：
  1. `vocab_id!` 宏加 `Reflect`（`ResourceId` / `StatId` / `StatusId` / `SkillId`）——
     它们是 `Resources` / `Stats` 的 `HashMap` 键。
  2. `atoms/actor.rs`：10 个类型加 `Reflect` + `#[reflect(Component)]`。
  3. `world/`：`ChunkPos` / `VoxelId` / `Voxel` / `VoxelAppearance` / `VoxelPalette` /
     `TerrainParams` / `NoiseParams` / `FaceShade` / `VoxelHit` 加 `Reflect`。
     **`Chunk` 与 `VoxelStore` 故意不加**：它们含 `Box<[Voxel; 32768]>`，整体反射没有意义。
  4. L2：`Movement` / `ActorVisual` / `Facing` / `ChunkMeshEntity` 等加 `Reflect`。
  5. **`debug::register_debug_types`**：显式 `register_type` 那些类型（含字段类型 `ChunkPos`）。

  **⚠️ 关键认知（我一开始搞错了）**：只加 `#[derive(Reflect)]` 与 `#[reflect(Component)]`
  **不够**。`reflect_auto_register` 的语义是"**被 `register_type` 过的**类型，连带注册
  它的字段类型"——它**不会**把项目类型自动扫进来。必须显式注册。
  实测：加完 derive 之后 `get_type_data::<ReflectComponent>()` 仍是 `None`，
  在 `debug.rs` 里 `register_type` 之后才有。

  **测试**：`tests/debug_visibility.rs` 新增两条 ——
  - `debug_channel_can_reflect_the_project_components`：断言 5 个组件
    **能当组件取**（`ReflectComponent` 类型数据在）。
    **第一版只断言 `registry.get().is_some()`，验证时发现它抓不到 bug**
    （去掉 `Reflect` 后照样通过）——那种断言等于没写。改成查 `ReflectComponent` 后
    **验证过它会失败**（去掉 `#[reflect(Component)]` ⇒ 报出预期消息）。
  - 走**真实注册路径**（`debug::register_debug_types`），不手工搭注册表。

  **实测结果**（跑起来的游戏，BRP HTTP）：
  | 查什么 | 结果 |
  |---|---|
  | `Movement` | ✅ 2 个实体 |
  | `Resources`（L0） | ✅ 2 个实体 |
  | `Name`（Bevy 内置，对照） | ✅ 能查到 `chunk[1,0,-2]` 等 |
  | **`ChunkMeshEntity`** | ❌ **仍返回空** |

  **未解**：`ChunkMeshEntity` 是同一批实体上的组件（`Name` 查得到它们），
  类型也注册了、`ReflectComponent` 类型数据也在（单测证明），
  但 BRP 查它**返回空**。我试过补注册字段类型 `ChunkPos`，无效。
  **下一步怀疑**：BRP 的 `world.query` 对"组件的字段类型未注册"或
  "组件是 `Copy` 小结构体"可能有额外条件；也可能要在 `BrpExtrasPlugin`
  之前注册。**不再猜** —— 下次直接读 `bevy_remote` 的 `query` 实现。


- [ ] **HUD 的其余交互**：反制窗口的"放弃"按钮、目标选择（现在 `CastRequest.target` 恒为 `None`，

      由 `Targeting` 自己找）

- [x] **行动条 / 进度 已画**（`presentation/hud/action_bar.rs`）
  数据源是 L1 已算好的 `Action::progress()`，L2 **只查不问**（R15）。
  行动实体通过关系组件 `ActiveActions` 挂在角色上，所以"谁在做什么"
  是一次关系查询，不需要 L2 自己维护列表；技能名走 `CastsSkill` → `Skill::name`。

  **两个刻意的决定**：
  - **取进度最大的那个**，不是第一个。`ActiveActions` 顺序不保证稳定，
    而界面上只画一条；取最大 ⇒ 与顺序无关 ⇒ 画面不抖。
  - **没有行动时显示"待命"而不是把整行藏掉**：藏起来会让面板高度跳动，
    而且玩家分不清"没事在做"和"界面坏了"。

  **把核心逻辑抽成了纯函数 `pick_action`**，直接单测（4 条）。
  为什么不"点按钮真的触发一次行动"再断言：那测的是 L1 的战斗链路
  （点击 → CastRequest → Action 实体），不归 HUD 管；而且我实测过 ——
  模拟点击按钮**没有产生行动**，那条链路需要真实输入路径，代价高且测错对象。
  `pick_action` 的 4 条里有 3 条**验证过会失败**（把实现改成"取最后一个"，
  顺序无关那条立刻报"该选进度 0.80 的那个"）。


- [x] **`Formula::RollUnder` 的内容用例** —— `skills.ron` 的 `shield_bash`（盾击）。
  读法：`attacker` 是**掷骰上界**，`threshold` 是成功率目标值
  （`roll = attacker * rand()`，`roll < threshold` 成功）。
  所以"力量越高越容易眩晕"是用属性换控制的表达。
- [x] **`Stacking::Ignore` / `Replace` 的内容用例** —— `statuses.ron` 新增
  - `staggered`（踉跄，`Ignore`）：有些减益**不该能被刷时间**，
    否则敌人只要在你快醒时补一下你就永远出不来。
  - `burning`（灼烧，`Replace`）：强度不叠但可以刷新的持续伤害。
  两者由 `cinder_burst`（余烬爆）**一次施加**，这样"叠加规则按每个状态自己的配置走"
  有了真实数据可测，而不是只靠 `status.rs` 的单测。
- [x] **`Faction::Neutral` 的内容用例** —— `monsters.ron` 的 `wandering_merchant`（游商）
  + `skills.ron` 的 `merchant_cower`。
  它带 `TargetIsEnemy` 门控，所以**永远打不出攻击**；这个承诺由
  `requirement.rs::a_neutral_actor_has_no_enemies_at_all` 钉住
  （中立看谁都不是敌人 —— 与已有的"玩家看中立不是敌人"是**两个独立分支**）。
- [x] **⭐ 加了一条"内容覆盖度"守卫测试**（`content_pipeline.rs::content_exercises_every_engine_branch`）
  上面三条 TODO 的共同病根是：**引擎支持某条分支，但没有任何内容走它**，
  于是那条分支在游戏里永远碰不到。单测只能证明"分支本身对"，证明不了这个。
  所以现在断言：`Stacking` 四种规则、`Formula::{Difference, RollUnder}`
  **都必须有内容在用**。删掉任何用例都会立刻亮（**验证过它会失败**）。
  `Formula::Ratio` **故意不断言**：现有数值里没有哪一对比值是设计上有意义的，
  写了也是凑数 —— 测试里留了注释说明"为什么它缺"。

- [x] **体素编辑已接输入**（`voxel_render/voxel_edit.rs`）—— 鼠标左键挖、右键放
  数据流**没有新造任何东西**：`viewport_to_world` → `VoxelStore::raycast`（DDA）
  → `resolve_edit` → `set_and_notify` → `ChunkDirtyMessage` → `rebuild_dirty_chunks`
  → 异步重新网格化。四样既有设施（各有单测）被接起来。

  **`pos + normal` 而不是玩家位置**（**R69**）：斜视时用玩家位置算出来的点
  经常落在方块**内部** —— 看不见，而且看起来像"右键没反应"。

  **护住世界底**：`y <= floor_y` 不让挖也不让放。不挡的话玩家能一路挖穿世界，
  看到清屏色，而且那一层不在任何区块的可见面里、**再也补不回来**。

  **顺带解决的两件事**：
  1. `VoxelStore` 新增 `edited: HashSet<ChunkPos>`（**只有真的变了才记** ——
     写同样的值不算改动，否则"读一遍写回"会伪造出改动记录）。
  2. 有了它，`can_skip` 的优化**重新安全**：`!is_edited && is_probably_empty`。
     round 2 时因为没有改动记录，`can_skip` 只能一律返回 `false`。

  **7 条测试**。其中 3 条**验证过会失败**（挖穿地板 / 顺序无关 / 改过的区块被跳过）。

  ## ⚠️ 未验证：真实 App 里点一下
  **我没能让"在运行中的 App 里点击鼠标"产生编辑**，所以**"输入真的到达了系统"
  这一环没有运行期证据**。试过的都无效：
  - PowerShell `mouse_event`（物理点击）：HUD 按钮有悬停高亮，但无编辑；
  - `brp_extras/move_mouse` + `click_mouse`（正规注入）：`move_mouse` 返回了
    新位置，`click_mouse` 无报错，但 `VoxelStore.edited` 仍为 0。

  **已经证明的部分**：从"一条射线"到"store 被改 + 区块标脏 + 脏消息发出"
  这段（测试直接驱动，见 `a_ray_translates_into_a_real_edit_...`）。
  **没证明的部分**：`just_pressed` 的边沿是否被读到、`Window::cursor_position`
  是否返回了值。这两件任一件坏了，表现都是"点击毫无反应"。

  **下一步怎么查**：在 `edit_with_mouse` 的每个 `return` 前加一条
  `info!`/计数器，用 BRP 读计数确定卡在哪一步。
  （**别再用"猜哪一步"的方式**——这条我已经绕了几轮。）

- [ ] **选中高亮**：`VoxelSelectedMessage` / 高亮线框还没做
  （编辑本身已可用，但玩家看不到"现在会挖到哪一格"）

- [x] **网格化已进 `AsyncComputeTaskPool`**（R72 的目标）—— 新模块 `voxel_render/async_mesh.rs`
  - `dispatch`：把 `(VoxelStore, TerrainParams, VoxelPalette, AtlasImage)` **快照**拷进任务闭包
    （任务必须 `Send + 'static`，不能借用世界），在计算线程池里跑贪婪网格化；
  - `collect`：每帧 `poll_once`，算完才 `meshes.add` + 挂 `Mesh3d`
    （`Assets<Mesh>` 不是线程安全的 ⇒ **落地必须在主线程**）；
  - `PendingChunkMesh` 组件让"哪些区块正在算"成为**可直接查询的世界状态**。

  **`ChunkMeshIndex` 的新不变量**：条目只由 `collect` 写入 ⇒
  **索引里的区块一定有 `Mesh3d`**，查询它的人拿不到半成品。

  **空区块连任务都不派发**：`is_probably_empty` 采样中心 + 6 个面心。
  实测 49 个已加载区块里只派发 **39** 个（10 个完全埋住）。
  判错是安全的 —— `collect` 里还会再判一次 `is_empty()` 并回收实体。

  **实测无回归**：截图对比（HUD 之外区域）深夜色像素 **9.04% → 9.05%**、
  偏绿 **87.63% → 87.50%** —— 异步改动没改变画面。

  **同步路径已删除**：`spawn_chunk_mesh` 没人用，整段删掉
  （它那段"空网格会触发 `slab_allocator: Use-after-free`"的说明搬进了
  `async_mesh::dispatch` 的文档）。

  **已知取舍**：改方块时旧网格先销毁、新网格异步补齐，中间有**短暂空窗**。
  要零空窗得做每区块双缓冲 —— 等有实际需要再加（注释里写了）。

  ### ⚠️ 我在这一段里引入过一个真缺陷（当天发现并修掉）
  `is_probably_empty` 第一版采样**区块中心 + 6 个面心**（`y ∈ {0, 16, 31}`）。
  但地形是**高度图**：地表在 `y ≈ ±2`，所以**区块中心（`y = 16`）永远是空气**
  ⇒ 判空几乎永远为真 ⇒ **有地形的区块被整个跳过**。

  **实测证据**：半径 3 应加载 49 个区块，`ChunkMeshIndex.loaded` 经 BRP 读出来只有 **39** 个。
  修好后回到 **49**（与理论值一致）。

  **修法**：采样**区块底面**（`y` 越低越可能实心），并新增 `can_skip`。

  **但 `can_skip` 现在一律返回 `false`（刻意的保守）**：
  底面为空**不等于**区块为空（悬空平台可能只在上半部），而 `VoxelStore`
  目前不记录"哪些区块被改过"，所以无法安全跳过。
  **宁可多派发一次任务，也不冒"玩家搭的浮空方块永久不显示、且不报错"的风险。**
  接上体素编辑后给 `VoxelStore` 加 `edited: HashSet<ChunkPos>` 就能拿回这个优化。

  **测试**：`a_chunk_holding_the_surface_is_not_reported_empty`
  （**验证过会失败**：把探针改回 `base.1 + half` 立刻报"不该被判成空"）
  + `can_skip_is_deliberately_conservative`。

  **教训**：我上一轮加这个优化时**没给探空写测试**，只写了"不 panic"那种空测试。
  于是缺陷直接进了可运行的构建 —— 而它的症状（少几块地形）很容易被误认成
  "本来就那样"。**想省一次计算的优化，必须有测试证明它没省掉不该省的东西。**


- [x] **存档 已落地**（`crates/voxelith-prime/src/save.rs`）
  ## 最重要的决定：**只存玩家的改动，不存地形**

  两条硬理由：

  1. **地形是推导出来的**。`VoxelStore` 里是"生成器输出 + 玩家改动"的混合体。
     把整个 store 序列化 = 把**推导结果当成事实来源** ⇒ 玩家存档会在他改一条
     地形参数之后**直接失真**（旧存档里是被埋住的土，新生成器认为那里该是空气）。
  2. **体积**。一个区块 32³ = 32768 个体素，49 个区块 = **160 万**条记录；
     玩家真改过的可能只有几十个。差 5 个数量级。

  生成器是**确定性**的 ⇒ "重新生成 + 重放改动"在同参数下等价，
  而**在参数变化后只有它是对的**。

  **按世界坐标存，不按区块内局部坐标**：后者要两条记录（区块 + 偏移），
  而且会在 `CHUNK_SIZE` 变化后失效。

  **格式是 RON**（与内容文件一致，人可读可 diff）。带 `version` 字段 ——
  没有它，将来改格式时无法区分"旧存档"与"坏存档"，只能报含义模糊的解析错误。

  **版本比程序新时拒绝并给出行动指引**（"升级游戏再读，不要用旧版本覆盖它"）。
  若当坏文件处理，用户的反应会是"存档坏了，我重开一个" —— 然后**数据真的丢了**。

  **写盘先写临时文件再改名**：直接覆盖的话，写到一半崩了会留下半截存档，
  而旧存档已经被毁了。

  **4 条测试**（往返一致 / 改动真的生效 / 未来版本被拒 / 存档不含地形），
  **验证过往返那条会失败**（临时让 `load` 丢掉 `edits` ⇒ 报"改动该原样回来"）。

  **踩坑**：`TerrainParams::default()` 的 `surface`/`soil`/`deep`
  **全是 `VoxelId(0)`（空气）** —— 那是 `WorldPlugin` 装配前的占位值，
  生成出来是空世界。测试里假设"地下一定是实心"直接挂了 ⇒ 改用
  `terrain_with_blocks()` 并且**探测**出实心/空气点，不硬写高度。


## 每步收尾

- [ ] `pwsh ./scripts/arch-guard.ps1`（**10/10 通过、221 个测试**）

- [ ] 单文件 < 500 行（R26）、新事件在定义它的模块注册（R34）、新内容进 `.ron`（规则 13）

- [ ] 新增集成测试前先看 [combat-design.md §7.2](../docs/combat-design.md#72-三个踩过的测试陷阱写新测试前先看这里)

      （时间三件套 + `Commands` 延迟，三个坑都不会报错）

- [ ] 改完渲染用 BRP 截图**看一眼**：结构测试看不出黑屏 / 投影退化 / 位置错

## 渲染排查方法论（**别再从源码猜**）

这一轮（第 5 轮）在"地形上为什么有黑块"上耗了十几个来回，
最后是靠**故意涂错颜色**收束的。记下来：

1. **先量化，别看图**。人眼看图会说"一堆色块"，而直方图会说
   "两个可见面只差 12% 亮度"——后者才能定位问题。用 Python + Pillow 统计像素。
2. **"故意涂错颜色"是最有效的判别手段**：
   - **按面朝向上色**（侧=红、顶=绿、底=蓝）：一眼看出**哪类面在渲染**，
     三色之和 = **真实网格覆盖**。本轮直接用它证明了"顶面占 87.8%"。
   - **整张图集涂一色**：区分"完全没画"与"画了但颜色不对"。
   - **只涂某一格**：确认格号映射对不对。
3. **一次只改一个变量**：起伏 0 vs 2、半径 3 vs 5、环境光 60 vs 20000。
   本轮用"起伏设为 0"直接证明了"洞只在高度变化处"。
4. **小心测量方法本身是错的**。本轮两次踩坑：
   - 用"两次截图的差异包围盒"测精灵尺寸 —— 差异只捕捉**变化环**，不是精灵本身；
   - 只在**小尺寸区间**测线性关系 —— 比例会误导，得到"不生效"的错误结论。
5. **环境变量开关**比反复改源码快得多（`std::env::var("XXX").is_ok()`），
   但**收尾时一定要全删掉**，并 grep 确认：
   `Select-String -Pattern 'ALL_TILES_MAGENTA|HIDE_TERRAIN|SPRITE_TEST_SIZE'`。
6. **测试必须验证"它会失败"**。本轮栽了两次：
   - 回归测试被我**关在 `mod tests` 外面**（多了一个 `}`），于是它
     编译进 crate 但**根本不运行** —— 禁掉被测分支它照样"通过"。
     症状是编译器警告 `unnameable_test_items` / `function is never used`。
     **别忽略警告**：这条警告就是在说"你写的测试没生效"。
   - 我自己的判定函数写错，把**明明存在**的面判成缺失（538/1077 假阴性，写错两版）。
     最后改成"**逐三角形 + 重心坐标判断点是否在内**"才稳 ——
     **别为每种面写一套新判定**，统一成"点在不在三角形里"，只改点在哪个平面。
7. **`#[path]` 引入的测试文件要自己写 `mod tests { }` 包一层**。
   裸内容会被当成与挂载点同级的 item，`super` 就指错了。
   （`crates/voxelith-axiom/src/behaviors/content/loader/tests.rs` 是正确样例。）

## 素材使用情况（用户问过两次，2026-10-01 核实）

四个 Kenney 包在 `D:\AI\assets`（zip + 解压都在，**不用重新下载**）。

| 素材 | 形态 | 用了没 |
|---|---|---|
| `ui-pack-adventure` | 592×600 图集，128 元素（panel×30 / progress×18 / button×6） | ✅ **已用**（HUD 换皮） |
| `roguelike-characters` | 918×203，16×16 格 / 1px 间距，约 400+ **静态**小人 | ⬜ 未用（候选：怪物占位美术） |
| `platformer-kit` | **3D 模型包**（FBX/OBJ/GLB）+ 一张调色板 PNG | ❌ 不能用于体素 |
| `modular-cave-kit` | **3D 模型包**（41 个 GLB）+ 两张调色板 PNG | ⚠️ **只有一块纹理可用** |

### 那两个 3D 包**不能拼接我们的地图**（用户问过）

看起来像"可以拼接"是对的 —— `modular-cave-kit` 里有 `template-floor` /
`template-wall` / `template-wall-corner` / `template-floor-layer` 这些**模块化模板**，
它确实是**按网格拼装**设计的。但：

- 它拼的是 **3D 网格模型**（每个块是一个 GLB 几何体）；
- 而我们的地形是**体素 + 贪婪网格化**（高度图 → `ChunkMesh` → 一张图集贴图）；
- 两套渲染路径完全不同：要用它就得**放弃体素系统**，改走"预制网格拼接"。

**所以：不拼。** 体素是三个目标里的第 1 条，不能为了用素材把它换掉。

### 但 cave-kit 里有一样**真能用**的东西 ⭐

`Models\Textures\variation-a.png` 是 512×512 的**调色板图**（按色相的渐变色带），
但**左上 128×128 那块是真的可平铺纹理**：蓝灰底 + 六边形斑点噪点，63 种颜色。

⇒ **可以当石头 / 岩壁方块贴图。** 这修正了我之前"3D 包的纹理全是色带、
完全不能用"的结论 —— 那个结论对 512×512 的绝大部分成立，但漏了左上那块。

**待做**：把它转成 16×16 平铺贴图接进 `atlas.rs`
（现在石头是程序化的 `Speckle` 噪点）。

### UI 换皮已落地

- `tools/transcode-kenney-ui.py`：从 Kenney 图集**按名字裁** 11 个图块，烘焙成 Rust 源码。
  只裁需要的 ⇒ **58 KB**，而整张图集是 1.4 MB（省 96%）。
- `crates/voxelith-prime/src/ui_theme/`：`UiTheme` 资源 + 九宫格 helper；
  每个图块一个文件（64×64 摊成源码是几百行，合一个文件会撞 R26）。
- HUD：左上**棕色**九宫格面板、左下**灰色**九宫格面板、
  进度条换成 Kenney 的圆头条（按池序号循环三色 ⇒ 加资源不用改代码）。
- **踩坑**：Kenney 面板图块是**不透明**的，所以 `BackgroundColor` 被整个盖住、
  改它**完全无效**；而浅色文字压在亮面板上**看不见**。
  正确做法是给图块加暗色 **tint**（`ImageNode.color` 与像素相乘）。

