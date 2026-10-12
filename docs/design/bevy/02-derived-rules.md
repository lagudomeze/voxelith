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
pub struct Derived<From, To> where From: Send + Sync + 'static, To: Send + Sync + 'static {
    pub config: Handle<RuleConfig>,
    _marker: PhantomData<fn() -> (From, To)>,
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
| `PhantomData<(A, B)>` 会挡住手写 derive | auto trait 会漏到 `A`/`B` 上（`Unpin` 依赖它们），而手写 derive 要求这个字段自己也有 Template → 换成 **`PhantomData<fn() -> (A, B)>`** 切断泄漏 |
| `#[require(X)]` 要求 `X: Default`，**且是在泛型上** | 所以 `Modifier<Mark>` 的 `Default` 要**手写**（派生版本会给 `Mark` 加 `Default` 约束，`#[require(Modifier<To>)]` 就过不了） |
| 目标类型要有 `Send + Sync + 'static` | `Modifier<To>: Component` 要求它；所以 `Derived` 上写了这两个约束 |

## ⚠️ 变更检测的坑：`&mut Mut<T>` 一取就脏

把 `Mut<T>` 转成 `&mut T`（例如 `apply(…, &mut modifier)`）会**立刻调 `DerefMut`**，Bevy 就此标记“变了”——哪怕一个字都没写。于是每帧都标记，下游的变更检测被连锁触发。

**观察到的现象**：`apply` 的写入只发生一次（打印为证），但 `Changed<Modifier<…>>` 每帧都是 1。

**正确写法**：比较与赋值都留在 system 里，且只在真的不等时才碰它：

```rust
let value = derived_value(constitution, config);
if modifier.flat != value {
    modifier.flat = value;   // 只有这一行会标记
}
```

顺带一个好处：**配置热重载自动生效**（下一帧值一变就写），不需要监听 `AssetEvent`。

## 出错策略：缺了要吵，没到货就等

```rust
if let Some(config) = configs.get(handle) { return Some(config); }
if handle.id() == AssetId::<RuleConfig>::default() {
    panic!("这条派生规则没有写 config（bsn 里漏了？）");   // 编程/内容错误
}
match asset_server.load_state(handle) {
    LoadState::Failed(err) => panic!("配置加载失败：{err}"),   // 文件不在、格式坏
    _ => None,                                                 // 还在加载：这一帧不算
}
```

- **“没写 config”检测得出来**：`bsn!` 没写那个字段时是 `Handle::default()` = `AssetId::DEFAULT_UUID`（**不是** `INVALID_UUID`），所以能精确判等。
- **`LoadState` 分四种**：`NotLoaded` / `Loading` / `Loaded` / `Failed(Arc<AssetLoadError>)`。异步加载意味着“第一帧必然没到货”，所以**只有 `Failed` 才算错误**。
- 未到货那些帧，那条修正保持原值（默认 0）——**不是静默默认值，而是“还没算”**；到货那一帧自动补上。

## 注册资产与加载器

```rust
app.init_asset::<RuleConfig>()
   .register_asset_loader(RuleConfigLoader)
   .add_systems(Update, (sync_health_max_from_constitution, sync_stamina_max_from_willpower));
```

- `AssetLoader` 在 0.19 里的签名要点：`type Settings: Settings + Default + Serialize + Deserialize`（用 `()` 即可）、`type Error: Into<BevyError>`、`load` 是 **async**（`async fn` 写就行）、`extensions()` 返回 `&["ron"]`。
- **加载器自己也要 `TypePath`**（`AssetLoader: TypePath`）→ `#[derive(Default, TypePath)]`。
- `bevy_asset` **不自带**任何文本格式的 loader，得自己写一个（20 行）。

## 每条 (属性 → 量) 一条 system

查询是静态类型，**没法“查出所有 `Derived<_, _>`”**，所以每个 (属性, 目标) 对还是要有自己的一条 system。乘法逻辑共用（`derived_value`），system 里只是把类型接起来。加一条新规则 = 复制一段、换类型、在插件里注册一行。

这条边界是干净的：**存在哪些可能的组合，是代码的事**（六维与资源本来就在 `voxelith_core` 里定义）；**这个角色有没有这条、系数多少，是内容的事**。
