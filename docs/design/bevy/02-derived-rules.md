# 派生（Bevy）

**这一篇只放真的取决于 Bevy 的取舍。** 模型（为什么这么做）见[属性与资源](../01-attributes-and-resources.md)的「派生」一节；代码在 `crates/voxelith_rules`。

**每条都实测过**：Bevy **0.19.1**，`cargo test -p voxelith_rules` 跑通（含“真从 `.ron` 文件加载”那条）。

## 形状

```rust
// 参数：一份文件 = 一条规则
#[derive(Asset, TypePath, Deserialize, Clone, Debug)]
pub struct RuleConfig { pub per_point: f32 }

// 一条派生规则：To 由 From 算出来，参数在哪份文件里
#[derive(Component, Clone, Debug, FromTemplate)]   // ← 手写（字段里有 Handle）
#[require(Modifier<To>)]                            // ← 结果那条修正自动带上
pub struct Derived<From, To>
where
    From: Send + Sync + 'static + Unpin,   // Unpin 是给 phantom 那条 const 约束的，见下
    To: Send + Sync + 'static + Unpin,
{
    pub config: Handle<RuleConfig>,
    _marker: PhantomData<(From, To)>,
}

pub type DerHealthMaxFromConstitution = Derived<Constitution, Max<Health>>;
```

`bsn!` 里直接写路径，场景初始化时会 `AssetServer::load` 它：

```rust
world.spawn_scene(bsn! {
    #Hero
    Constitution(12.0)
    Health { current: 30.0, base_max: 60.0 }
    DerivedModifiers [
        DerHealthMaxFromConstitution { config: "rules/health_from_constitution.ron" },
    ]
})
```

## 实测记下来的六条

| 事实 | 怎么做 / 不这么做会怎样 |
| --- | --- |
| `Handle<T>` 有专门的 Template | `impl FromTemplate for Handle<T> { type Template = HandleTemplate<T>; }`；**字符串字面量**在 `bsn!` 里自动变成 `HandleTemplate::Path`，场景初始化时 `AssetServer::load(路径)`。另有 `HandleTemplate::value(…)`（内联资产）与 `Handle::from(h)`（克隆已有 handle） |
| 带 `Handle` 字段的组件要**手写 `FromTemplate`** | 走 blanket 那条路（`Clone + Default + Unpin`）字段类型必须是 `Handle<T>` 本身，路径字符串就进不来 |
| **别顺手 `#[derive(Default)]`** | `Handle` 自己有 `Default`（Bevy 用 auto-trait specialization 让它能与 blanket 共存），带 handle 的 struct 若同时 `Clone + Default + Unpin` 就会与手写 derive 抢 |
| `PhantomData<(A, B)>` 会挡住手写 derive | auto trait 会漏到 `A`/`B` 上（`Unpin` 依赖它们），而手写 derive 要求这个字段自己也有 Template → 两条解法：给 `A`/`B` 加 `Unpin`（现用），或换成 `PhantomData<fn() -> (A, B)>` 切断泄漏（见文末附表） |
| `#[require(X)]` 要求 `X: Default`，**且是在泛型上** | 所以 `Modifier<Mark>` 的 `Default` 要**手写**（派生版本会给 `Mark` 加 `Default` 约束，`#[require(Modifier<To>)]` 就过不了） |
| 目标类型要有 `Send + Sync + 'static` | `Modifier<To>: Component` 要求它；所以 `Derived` 上写了这两个约束 |

## ⚠️ 变更检测的坑：`&mut Mut<T>` 一取就脏

把 `Mut<T>` 转成 `&mut T`（例如 `apply(…, &mut modifier)`）会**立刻调 `DerefMut`**，Bevy 就此标记“变了”——哪怕一个字都没写。于是每帧都标记，下游的变更检测被连锁触发。

**观察到的现象**：`apply` 的写入只发生一次（打印为证），但 `Changed<Modifier<…>>` 每帧都是 1。

**正确写法**：比较与赋值都留在 system 里，且只在真的不等时才碰它：

```rust
let value = from.base() * config.per_point;
if modifier.flat != value {
    modifier.flat = value;   // 只有这一行会标记
}
```

顺带一个好处：**配置热重载自动生效**（下一帧值一变就写），不需要监听 `AssetEvent`。

## 出错策略：没到货就是错误，加载由装配层等齐

```rust
// 就在 system 里，没有单独一层 helper
let config = configs.get(&rule.config).unwrap_or_else(|| {
    if rule.config.id() == AssetId::<RuleConfig>::default() {
        panic!("派生规则没有写 config（bsn 里漏了？）");
    }
    panic!("派生规则的配置还没加载好（装配层应该在进游戏前等齐）");
});
```

- **system 里没有“还在加载”这条分支**：要么配置在，要么就是错误。“等加载”是**装配层**的事——进游戏前把用到的资产等齐（真实项目里这一等就是加载界面）。
- **“没写 config”检测得出来**：`bsn!` 没写那个字段时是 `Handle::default()` = `AssetId::DEFAULT_UUID`（**不是** `INVALID_UUID`），所以能精确判等。
- `AssetServer::load_state` 仍然有四种（`NotLoaded` / `Loading` / `Loaded` / `Failed`），装配层想做得更细（显示进度之类）可以用它；**system 里不用**。
- 测试 `config_loads_from_a_ron_file` 就照这个模式写：**先等文件就绪，再摆用到它的实体**。

## 注册资产与加载器

```rust
app.init_asset::<RuleConfig>()
   .register_asset_loader(RuleConfigLoader)
   .add_systems(Update, (sync_health_max_from_constitution, sync_stamina_max_from_willpower));
```

- `AssetLoader` 在 0.19 里的签名要点：`type Settings: Settings + Default + Serialize + Deserialize`（用 `()` 即可）、`type Error: Into<BevyError>`、`load` 是 **async**（`async fn` 写就行）、`extensions()` 返回 `&["ron"]`。
- **加载器自己也要 `TypePath`**（`AssetLoader: TypePath`）→ `#[derive(Default, TypePath)]`。
- `bevy_asset` **不自带**任何文本格式的 loader，得自己写一个（20 行）。

## 一个泛型 system，插件里注册每种组合

```rust
impl<From, To> Derived<From, To>
where From: Send + Sync + 'static + Unpin, To: Send + Sync + 'static + Unpin
{
    pub fn new(config: Handle<RuleConfig>) -> Self { … }

    /// 把这条规则算出来，写进结果那条修正。
    pub fn sync(
        configs: Res<Assets<RuleConfig>>,
        owners: Query<&From>,
        mut rules: Query<(&Derived<From, To>, &DerivedFor, &mut Modifier<To>)>,
    ) where From: Value { … }
}

// 插件里：用别名注册，一眼看出是哪条规则
.add_systems(Update, (
    DerHealthMaxFromConstitution::sync,
    DerStaminaMaxFromWillpower::sync,
    DerStaminaRegenFromWillpower::sync,
))
```

关联函数也是普通函数，`add_systems` 照样收（**实测过**：`DerHealthMaxFromConstitution::sync` 能当 system 注册）。好处是 `Send + Sync + 'static + Unpin` 那堆约束待在 impl 块上，不用在函数上再抄一遍；`new` 与 `sync` 也在一起——一条规则的两半。

查询是静态类型，**没法“查出所有 `Derived<_, _>`”**——但**一个泛型函数就够**：每个 (属性, 目标) 对只是把它实例化一次。所以：

- **加一条新规则 = 在插件里加一行**（`别名::sync`）；
- 不存在“每个对复制一份 system”这回事（我第一版那么写了，是多余的）。

### 附：`PhantomData` 怎么选（这条坑的根）

| 写法 | auto trait（`Send`/`Sync`/`Unpin`） | 用在哪 |
| --- | --- | --- |
| `PhantomData<T>` | **跟着 T**（`Send` 当且仅当 `T: Send`） | T 本身已经是 `Send + Sync + 'static` 时（例如 T 是 `Component`）——直白好读 |
| `PhantomData<fn() -> T>` | **永远 `Send + Sync + Unpin`** | **只在需要“提到 T 但不继承它的约束”时**：这里 `Derived` 就是（手写 `FromTemplate` 要求这个字段自己有 `Template`，而泛型 `(From, To)` 证不出 `Unpin`） |
| `PhantomData<*const T>` | 永远**不** `Send`/`Sync` | 要让类型不 Send 时（我们没用到） |

即：**先问 T 是不是已经满足了你要的约束**——是就用 `PhantomData<T>`，不是才上 `fn() -> T`。`Adjust<R>`（core 的改值消息）里我一开始照抄了 `fn() -> R`，其实多余（`R: Resourceable` 已经蕴含 `Send + Sync + 'static`），已经改回 `PhantomData<R>`。

`Derived` 这里三条路都试过（实测）：

| 写法 | 结果 |
| --- | --- |
| `PhantomData<(From, To)>` | ✗ 报 `` `PhantomData<(From, To)>` cannot be unpinned … required by `Template` `` |
| 拆成两个 marker（`PhantomData<From>` + `PhantomData<To>`） | ✗ **同样不过**，报 `` `To` cannot be unpinned … within `PhantomData<To>` `` |
| **`PhantomData<(From, To)>` + 给两个参数加 `Unpin` 约束**（采用） | ✓ 字段直白；代价是 `Unpin` 进 `Derived` 的公开约束（struct、impl、用它的泛型 system 都要写）。标记类型本来就都是 `Unpin`，实践中没有代价 |
| `PhantomData<fn() -> (From, To)>` | ✓ 也能过，且不给 `Derived` 添任何约束；但字段类型绕，读的人得先懂 auto trait 这件事 |

前提是 `Derived` 要**手写 `FromTemplate`**（字段里有 `Handle`）——那套派生要求每个字段自己实现 `Template`，才会去要 `Unpin`。不需要手写派生的组件不会撞上这条。
