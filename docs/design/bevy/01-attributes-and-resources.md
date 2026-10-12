# 属性与资源（Bevy）

**这一篇只放真的取决于 Bevy 的取舍。** 模型见[属性与资源](../01-attributes-and-resources.md)，组件形状的约定见[组件约定](../00-component-conventions.md)。

**每条都实测过**：Bevy **0.19.1**，工作区里 `cargo test` 跑通（含 `tests/system_read.rs`）；探索性验证另有 `D:\code\rust\voxelith-bsn-spike`。

## 命名规则（先看这条）

使用方会**同时 glob 导入** `bevy_ecs::prelude::*` 与本 crate 的 prelude，所以**本 crate 的公开名字不能与 `bevy_ecs::prelude` 撞**。实测踩到三个：

| 撞的名字 | 对方是什么 | 症状 | 改成 |
| --- | --- | --- | --- |
| `Add` | `bevy_ecs::lifecycle::Add` | `E0659` 歧义，直接编译不过 | `Flat` |
| `Resource` | `bevy_ecs::resource::Resource`（trait） | **被静默遮蔽**——不报错，但名字的意思变了 | 资源相关用 `Resourceable` |
| `Single` | `bevy_ecs::system::Single`（system param） | 同上，会遮蔽 | 不用这个名字 |

**自查办法**：对照 `bevy_ecs` 的 `pub mod prelude { ... }` 名单过一遍。

## 形状总览

```
Strength(pub f32)                 ← 单值型：newtype，标记＝它自己
Health { current, base_max }      ← 资源：当前值 + 上限
Max<Health>                       ← 上限那个量的标记（资源有两个值，上限要有名字）

Modifier<Mark>                    ← 挂在“修正实体”上的组件
├── Flat(f32, PhantomData<Mark>)  ← 平加（负 = 减）
└── Ratio(f32, PhantomData<Mark>) ← 比例（负 = 按比例减）

ModifiesOf(Entity) / Modifiers(Vec<Entity>)   ← 关系：修正指回主人
Resolvable / Resourceable                     ← 读出：单值型 / 资源
resolve_with(base, mods)                      ← 全项目唯一的公式
```

两条**故意不提供**的东西：

- **没有泛型载体**（`Value<M>` 那种）：单值型的标记就是它自己，资源只需要一个 `Max<T>`。
- **没有 `resolved(world, owner)`**：写 system 时手里已经有组件和关联列表，不该回头去 `World` 里捞。见下面“system 里怎么用”。

## 属性：一条宏声明一行

```rust
macro_rules! single_value {
    ($($(#[$meta:meta])* $name:ident),+ $(,)?) => {$(
        $(#[$meta])*
        #[derive(Component, Clone, Copy, Debug, Default)]
        pub struct $name(pub f32);

        impl Resolvable for $name {
            fn base(&self) -> f32 { self.0 }
        }
    )+};
}
```

`Resolvable` 的标记就是实现者自己，所以不需要 `type Mark = Self` 这种重复写法。

## 资源：当前值 + 上限

```rust
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Health {
    pub current: f32,    // 状态：由事件（伤害/治疗/回复）改，不走修正
    pub base_max: f32,   // 上限的基础值：修正加在它上面
}

impl Resourceable for Health {
    fn current(&self) -> f32 { self.current }
    fn base_max(&self) -> f32 { self.base_max }
}

pub type ModHealthMax = Modifier<Max<Health>>;
```

**上限的标记是 `Max<T>`**——不用每个资源手写一个 `HealthMax` 那样的标记类型。

## 读出：一条公式

```rust
/// 全项目唯一的公式：基础值 + Σ每条修正的贡献（比例按基础值算）。
pub fn resolve_with<'a, Mark: 'a>(
    base: f32,
    modifiers: impl Iterator<Item = &'a Modifier<Mark>>,
) -> f32 {
    base + modifiers.map(|m| m.contribution(base)).sum::<f32>()
}

pub trait Resolvable: Component + Sized {
    fn base(&self) -> f32;
    fn resolve<'a>(&'a self, modifiers: impl Iterator<Item = &'a Modifier<Self>>) -> f32 {
        resolve_with(self.base(), modifiers)
    }
}

pub trait Resourceable: Component + Sized {
    fn current(&self) -> f32;
    fn base_max(&self) -> f32;
    /// 读出**最终上限**。当前值不走这里。
    fn resolve_max<'a>(&'a self, modifiers: impl Iterator<Item = &'a Modifier<Max<Self>>>) -> f32 {
        resolve_with(self.base_max(), modifiers)
    }
}
```

### system 里怎么用（主要用法）

关联列表来自 `&Modifiers`，修正来自各自的查询——**不需要 `World`**：

```rust
fn report(
    heroes: Query<(&Strength, &Health, &Modifiers)>,
    strength_mods: Query<&Modifier<Strength>>,
    health_mods: Query<&Modifier<Max<Health>>>,
) {
    for (strength, health, related) in &heroes {
        let s = strength.resolve(related.iter().filter_map(|m| strength_mods.get(m).ok()));
        let max_hp = health.resolve_max(related.iter().filter_map(|m| health_mods.get(m).ok()));
    }
}
```

`&Modifiers` 是关系集合，直接 `.iter()` 就行（`RelationshipTarget` 给了 `collection()`，且能 `Deref` 到底层 `Vec<Entity>`）。完整可跑的例子在 `crates/voxelith_core/tests/system_read.rs`。

### 只有测试/装配才需要从世界里捞

```rust
pub fn modifiers<'w, Mark: Send + Sync + 'static>(
    world: &'w World,
    owner: Entity,
) -> impl Iterator<Item = &'w Modifier<Mark>>
```

## 修正：一个枚举组件

```rust
#[derive(Component, Clone, Debug, VariantDefaults)]
pub enum Modifier<Mark> {
    Flat(f32, PhantomData<Mark>),
    Ratio(f32, PhantomData<Mark>),
}

impl<Mark> Modifier<Mark> {
    pub fn flat(amount: f32) -> Self { Self::Flat(amount, PhantomData) }
    pub fn ratio(ratio: f32) -> Self { Self::Ratio(ratio, PhantomData) }
    pub fn contribution(&self, base: f32) -> f32 {
        match self {
            Modifier::Flat(amount, _) => *amount,       // 与基础值无关
            Modifier::Ratio(ratio, _) => base * ratio,  // 按基础值算
        }
    }
}

impl<Mark> Default for Modifier<Mark> {
    fn default() -> Self { Self::flat(0.0) }
}
```

- **`PhantomData` 必须留**：合掉 `Flat<M>` / `Ratio<M>` 两个载荷类型之后，`Mark` 没被任何变体用到就报 `E0392`（泛型参数没被使用）。**但配置里不用写它**（见下）。
- **值是有符号的**：负的平加就是减，负的比例就是按比例减；同一个量可以同时挂加和减，一起归约。
- **两条必须照做**：泛型参数别叫 `M`（`Component` 派生在泛型枚举上会自己引入 `M`，撞名 `E0403`）；枚举要 `VariantDefaults`。

## relationship：修正指回主人

```rust
#[derive(Component, FromTemplate)]   // Entity 没有 Default，必须派生
#[relationship(relationship_target = Modifiers)]
pub struct ModifiesOf(pub Entity);

#[derive(Component, Default)]
#[relationship_target(relationship = ModifiesOf, linked_spawn)]
pub struct Modifiers(Vec<Entity>);   // 字段私有
```

- **必须派生 `FromTemplate`**：`Entity` 没有 `Default`，走不了那条 blanket impl。
- **字段必须私有**：`RelationshipTarget` 的要求。
- **`linked_spawn`**：主人销毁时修正实体**自动一起消失**（实测）。

## 别名

```rust
pub type ModStrength  = Modifier<Strength>;         // 单值型：标记＝类型自己
pub type ModHealthMax = Modifier<Max<Health>>;      // 资源：标记＝上限
```

（单值型的载体不需要别名——`Strength` 本身就是那个类型。修正也只留 `Mod*` 一个别名：`Flat`/`Ratio` 已经不是类型了。）

## 在 `bsn!` 里写“角色 + 它的修正”

```rust
world.spawn_scene(bsn! {
    #Hero
    Strength(10.0)                                 // 单值型：tuple 写法
    Health { current: 30.0, base_max: 60.0 }       // 资源：具名字段
    Modifiers [
        ModStrength::Flat(5.0),
        ModStrength::Flat(-2.0),                   // 减
        ModStrength::Ratio(0.5),
        ModHealthMax::Flat(20.0),
    ]
})
```

**带数据的枚举变体是能写的**，要满足三条（都实测过）：

| 条件 | 不满足会怎样 |
| --- | --- |
| 枚举要有 `VariantDefaults` | 报 `no variant, associated function, or constant named default_flat` |
| 载荷类型要能默认 | 报 `the trait bound ... : Default is not satisfied` |
| 泛型参数别叫 `M` | `Component` 派生报 `E0403`（名字重复） |

**tuple 变体可以只写第一个字段**：`Flat(f32, PhantomData<Mark>)` 在配置里就写 `ModStrength::Flat(5.0)`——宏会把没写的字段补成默认值。路径上写全 turbofish 或用别名，两种都行。

**踩过的坑**：`Modifiers [...]` 列表里**不要**手写 `ModifiesOf(#Hero)`。`RelationshipTarget` 有 `Relationship` 关联类型，宏据此知道对方是谁并**自动补上**；手写一遍就会得到：

```
Bundle <dynamic bundle> has duplicate components: ["voxelith_core::modifier::ModifiesOf"]
```

## `.bsn` 文件加载还没来

`bevy_scene` 文档的原话：scene 由 `bsn!` 或 in the future `.bsn` asset files 创建。管道已备（`ScenePatch` 是 `Asset`、`SceneComponent` 是“加载完自动应用”的组件），**解析器尚未实现**。将来文件是**补丁**语义，不是从零描述实体。
