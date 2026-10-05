# Bevy 查询与组件存储：本项目的写法

> 本文回答两个问题：**查询怎么写**（`QueryData` / `QueryFilter`）、
> **组件用什么存储**（`Table` / `SparseSet`）。
> 与 [bevy-events.md](bevy-events.md) 并列：那篇讲"系统之间怎么说话"，这篇讲"系统怎么看世界"。

## 0. 一句话

**元组超过三个字段、或者你需要给"查询项"起个名字 → 写 `QueryData`；
过滤器能表达一条**有名字的语义**（"引擎角色轴的玩家侧"）→ 写 `QueryFilter`。
两者都不为省字服务。**

存储则看这条：**这个组件会不会被反复插删？**
会 → `SparseSet`；不会（或者它主要用来当 `With` 过滤条件）→ 默认 `Table`。

## 1. `QueryData`：什么时候值得写

```rust
/// 一条**到时长、待结算**的行动。
#[derive(QueryData)]
pub struct ReadyAction {
    pub entity: Entity,
    pub cast: &'static CastsSkill,
    pub owner: &'static InitiatedBy,
    pub action: &'static Action,
}
```

判据（满足其一就写）：

| 情况 | 例子 |
|---|---|
| 元组字段 ≥ 4，且在**一个地方**被完整读一遍 | `ReadyAction`（`resolve_actions`） |
| 元组字段 ≥ 4，且想给"这一坨"起个名字 | `DecidingMonster`（`monster_decide`） |
| 需要在"查询项"上挂**方法** | `DecidingMonsterItem::is_busy()`、`CameraSnapshotItem::kind()` |
| 同一坨字段在**多个系统**里重复出现 | （本项目暂时没有，出现时优先写） |

**不值得写**的情况：两三个字段的元组。`Query<(Entity, &DecisionSlot)>` 本来就一眼看完，
包成结构体只是多一层跳转。

### 1.1 过滤器**不要**塞进 `QueryData`

```rust
// ✅ 过滤器留在过滤器位置
Query<ReadyAction, With<ReadyToResolve>>
// ❌ 放进查询项
#[derive(QueryData)] struct Bad { ready: Has<ReadyToResolve>, .. }
```

两者不一样：`With` 决定**扫哪些实体**（可走 archetype 级跳过）；
`Has<T>` 是"拿到了就顺便看一眼"。把过滤条件降级成数据字段，会把该跳过的实体也扫一遍。

### 1.2 mutable 要显式声明

```rust
#[derive(QueryData)]
#[query_data(mutable)]
pub struct DecidingMonster {
    pub energy: &'static mut ActionEnergy,
    ..
}
```

`#[query_data(mutable)]` 不是"允许 `&mut`"（那是字段类型的事），
它决定生成的 item 类型是否宣告可变访问 —— 少了它，`&mut Query<..>` 迭代编不过。

## 2. `QueryFilter`：给"筛哪条轴"起名字

本项目把角色拆成**三条正交轴**（引擎角色 / 阵营 / 特性，见
[combat-design.md](combat-design.md) §4.0）。三条轴在代码里长得几乎一样：

```rust
With<Player>          // 引擎角色轴：谁听输入
With<Faction>         // 阵营轴
With<ActorTags>       // 特性轴
```

所以引擎角色轴的两个方向各有名字：

```rust
#[derive(QueryFilter)]
pub struct InputDriven { player: With<Player> }      // 听输入的那侧

#[derive(QueryFilter)]
pub struct AiDriven { not_player: Without<Player> }  // 自己 tick 的那侧
```

用法上它和 `With<Player>` 等价，但它**把"我在按哪条轴筛"写进了类型** ——
一个由 AI 驱动的友方 NPC 是 `Faction::Player`，却不该被 `InputDriven` 选中。
裸的 `With<Player>` 读起来分不出这两件事。

**其余地方不写。** 单个 `With<X>` 包一层类型只是多一个跳转，没有信息增益。
只有当过滤器**由多个子句组成**、或者它承载了一条**项目自己的语义**时才值得命名。

## 3. 组件存储：`Table` vs `SparseSet`

```rust
#[derive(Component)]
#[component(storage = "SparseSet")]
pub struct DecisionSlot(Entity);
```

| | `Table`（默认） | `SparseSet` |
|---|---|---|
| 遍历 | 快（连续内存） | 慢（指针追逐） |
| 插 / 删 | **要搬整行**（换 archetype） | 只动稀疏集 |
| 当 `With` 过滤条件 | 快（archetype 级跳过） | **慢**（要逐个查稀疏集） |

### 3.1 一条容易搞错的细节：判据是 **table**，不是 archetype

"`SparseSet` 不参与 archetype 身份"这句话**不准确**。Bevy 的真实模型是：

- `Archetype` = 一组组件；
- `Table` = 真正存组件行的地方，**多个 archetype 可以共用一张 table**；
- 插一个 `SparseSet` 组件**仍然会换 archetype**，只是**不换 table**。

省掉的是"把那一整行 `Table` 组件拷到新地方"这一步 —— 那才是插删的代价。
所以想验证一条存储选择，断言要写在 `archetype().table_id()` 上。
（写 `archetype().id()` 会得出"没生效"的**错误结论** —— 本文第一版就是这么写的，
被 `the_decision_slot_does_not_move_the_monster_between_tables` 红出来的。）

### 3.2 本项目实际怎么选

| 组件 | 存储 | 为什么 |
|---|---|---|
| `DecisionSlot` | `SparseSet` | 每个决策周期插一次摘一次；怪物身上十来个组件；**它当数据读，不当过滤器** |
| `PendingChunkMesh` | `SparseSet` | 临时标记：派发时插、收结果时摘；区块实体带着 `Mesh3d` / `MeshMaterial3d` 一串 |
| `ReadyToResolve` / `ResolveNow` / `Threat` | `Table` | 它们是**过滤器**（`With<ReadyToResolve>`），`SparseSet` 会让过滤更慢；而且只插一次 |
| `ActiveActions` / `Statuses` | `Table` | 同上：`Update` 里被当过滤条件读 |
| `Faction` | `Table` | **生成时写一次，之后永不改** ⇒ `SparseSet` 一点好处都没有，反而拖慢 `With<Faction>` |

> `Faction` 这一行是本文存在的理由之一：**"枚举值就用 SparseSet 标记"是不成立的**。
> 那套做法（`bevy_enum_tag` 那种）解决的是"枚举值会变、而且要按值过滤"；
> 本项目的 `Faction` 生成后就固定，为它写同步系统 + 三个标记组件是净开销。
> 判据永远是"**会不会被反复插删**"，不是"它是不是枚举"。

### 3.3 怎么验证一条存储选择

两条测试一起写，缺一不可：

1. **存储类型**（`Components::get_info(id).storage_type()`）——
   它防的是"有人顺手改回 `Table`，什么都不报，只是变慢"。
2. **行为**（插上 / 摘下后 `table_id()` 不变）——
   它防的是"属性写了但没生效"（例如 `#[component(storage = ...)]` 被别的 derive 属性吃掉）。

例子见 `monster_decision.rs` 的 `the_decision_slot_is_registered_as_a_sparse_set`
与 `the_decision_slot_does_not_move_the_monster_between_tables`。

## 4. 检查清单

- [ ] 元组 ≥ 4 个字段，或者需要给查询项起名字 / 挂方法了吗？→ `QueryData`
- [ ] 过滤条件是**数据字段**还是**过滤器**？前者会把该跳过的实体也扫一遍
- [ ] `#[query_data(mutable)]` 加了吗（有 `&mut` 字段时）
- [ ] `QueryData` / `QueryFilter` 的 derive 要**显式 `use`**：
      `bevy_ecs::query::{QueryData, QueryFilter}`（prelude 只给 trait，不给 derive 宏）
- [ ] 新组件：会被**反复插删**吗？→ `SparseSet`；会被当 `With` 过滤吗？→ `Table`
- [ ] 加了 `SparseSet` 就补两条测试（存储类型 + `table_id` 不变）
