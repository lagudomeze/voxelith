# 属性与资源（Bevy）

**这一篇只放真的取决于 Bevy 的取舍。** 模型见[属性与资源](../01-attributes-and-resources.md)，组件形状的约定见[组件约定](../00-component-conventions.md)。

**每条都实测过**：Bevy **0.19.1**，工作区里 `cargo test` 跑通；探索性验证另有 `D:\code\rust\voxelith-bsn-spike`。

## 命名规则（先看这条）

使用方会**同时 glob 导入** `bevy_ecs::prelude::*` 与本 crate 的 prelude，所以**本 crate 的公开名字不能与 `bevy_ecs::prelude` 撞**。实测踩到三个：

| 撞的名字 | 对方是什么 | 症状 | 改成 |
| --- | --- | --- | --- |
| `Add` | `bevy_ecs::lifecycle::Add` | `E0659` 歧义，直接编译不过 | `Flat` |
| `Resource` | `bevy_ecs::resource::Resource`（trait） | **被静默遮蔽**——不报错，但名字的意思变了 | `Pool`（后来干脆不用泛型载体） |
| `Single` | `bevy_ecs::system::Single`（system param） | 同上，会遮蔽 | 不用这个名字 |

**自查办法**：对照 `bevy_ecs` 的 `pub mod prelude { ... }` 名单过一遍。

## 形状总览

```
Strength(pub f32)              ← 单值型：newtype，自己就是自己的标记（Mark = Self）
Health { current, base_max }   ← 资源池型
HealthMax                      ← 上限的标记（资源有两个值，上限得有名字）

Modifier<Mark>                 ← 挂在“修正实体”上的组件（枚举，两个变体）
├── Flat(Flat<Mark>)           ← 平加：+5 力量
└── Ratio(Ratio<Mark>)         ← 比例：+50% 基础力量

ModifiesOf(Entity)             ← 修正指回主人
Modifiers(Vec<Entity>)         ← 主人身上自动维护的列表（linked_spawn）

Resolvable                     ← 读出接口：基础值 + Σ每条修正的贡献
```

**没有泛型载体**：不需要 `Value<M>` 这种东西——单值型把 `Mark` 指成自己就够了，还能省掉一层标记命名。

## 属性：一条宏声明一行

```rust
macro_rules! single_value {
    ($($(#[$meta:meta])* $name:ident),+ $(,)?) => {
        $(
            $(#[$meta])*
            #[derive(Component, Clone, Copy, Debug, Default)]
            pub struct $name(pub f32);

            impl Resolvable for $name {
                type Mark = Self;              // 单值型：修正直接指向这个类型
                fn base(&self) -> f32 { self.0 }
            }
        )+
    };
}

single_value! {
    /// 力量：物理行为的强度。
    Strength,
    /// 敏捷：命中与闪避。
    Dexterity,
    // …
}
```

## 资源：当前值 + 基础上限

```rust
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Health {
    pub current: f32,    // 状态：由事件改，不走修正
    pub base_max: f32,   // 上限的基础值：修正加在它上面
}

/// 上限那个量——资源有两个值，所以它需要一个名字。
#[derive(Clone, Copy, Debug, Default)]
pub struct HealthMax;

impl Resolvable for Health {
    type Mark = HealthMax;
    fn base(&self) -> f32 { self.base_max }   // 读出的是上限
}

impl Health {
    /// 一步到位：读出最终上限。
    pub fn resolved_max(&self, world: &World, owner: Entity) -> f32 {
        self.resolved(world, owner)
    }
}
```

## 读出接口

```rust
pub trait Resolvable {
    /// 指向这个量的修正用哪个标记。
    type Mark;

    /// 基础值。
    fn base(&self) -> f32;

    /// 读出：基础值 + Σ每条修正的贡献。顺序无关，所以可交换、可并行。
    fn resolve<'a>(&'a self, modifiers: impl Iterator<Item = &'a Modifier<Self::Mark>>) -> f32
    where Self: Sized
    {
        let base = self.base();
        base + modifiers.map(|m| m.contribution(base)).sum::<f32>()
    }

    /// 一步到位：从世界里读出最终值（对资源池就是最终上限）。
    fn resolved(&self, world: &World, owner: Entity) -> f32
    where Self: Sized, Self::Mark: Send + Sync + 'static
    {
        self.resolve(modifiers::<Self::Mark>(world, owner))
    }
}
```

**遍历修正的迭代器**（走主人自己的 `Modifiers`，不做全库扫描）：

```rust
pub fn modifiers<'w, Mark: Send + Sync + 'static>(
    world: &'w World,
    owner: Entity,
) -> impl Iterator<Item = &'w Modifier<Mark>> {
    world.get::<Modifiers>(owner).into_iter().flat_map(move |mods| {
        mods.iter().filter_map(move |m| world.get::<Modifier<Mark>>(m))
    })
}
```

## 修正：一个枚举组件

```rust
#[derive(Component, Clone, Debug, VariantDefaults)]
pub enum Modifier<Mark> {
    Flat(Flat<Mark>),
    Ratio(Ratio<Mark>),
}

impl<Mark> Modifier<Mark> {
    /// 这条修正贡献多少（比例按**基础值**算）。
    pub fn contribution(&self, base: f32) -> f32 {
        match self {
            Modifier::Flat(flat) => flat.amount,        // 平加与基础值无关
            Modifier::Ratio(ratio) => base * ratio.value,
        }
    }
}

// `bsn!` 需要 `Clone + Default`（那两个 trait 才有自动 Template）。
// 泛型枚举的 `#[derive(Default)]` 只认无数据的变体，所以手写一个“空平加”——贡献 0。
impl<Mark> Default for Modifier<Mark> {
    fn default() -> Self { Self::Flat(Flat::default()) }
}
```

两条**必须**照做：

- **泛型参数不能叫 `M`**：`Component` 派生在泛型枚举上会自己引入一个 `M`，撞名报 `E0403`（实测）。所以这里叫 `Mark`。
- **载荷类型要手写 `Default`**：派生版本会给泛型参数加 `Default` 约束，而 `VariantDefaults` 需要在泛型上取默认值（否则报 `the trait bound Mark: Default is not satisfied`）。

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
- **字段必须私有**：`RelationshipTarget` 的要求（防止绕过关系直接改）。
- **`linked_spawn`**：主人销毁时修正实体**自动一起消失**（实测）。“卸下装备 = 销毁那个修正实体”因此不用手写清理。

## 别名：把类型绑到好写的名字上

```rust
pub type ModStrength = Modifier<Strength>;   // 挂在修正实体上的组件
pub type FlatStrength = Flat<Strength>;      // 它的两种载荷
pub type RatioStrength = Ratio<Strength>;
```

（单值属性的载体**不需要别名**——`Strength` 本身就是那个类型。）

## 在 `bsn!` 里写“角色 + 它的修正”

```rust
world.spawn_scene(bsn! {
    #Hero
    Strength(10.0)                                 // 单值型：tuple 写法
    Health { current: 30.0, base_max: 60.0 }       // 资源池：具名字段
    Modifiers [
        ModStrength::Flat(FlatStrength { amount: 5.0 }),
        ModStrength::Ratio(RatioStrength { value: 0.5 }),
        ModHealthMax::Flat(FlatHealthMax { amount: 20.0 }),
    ]
})
```

**带数据的枚举变体是能写的**，但要满足三条（都实测过）：

| 条件 | 不满足会怎样 |
| --- | --- |
| 枚举要有 `VariantDefaults` | 报 `no variant, associated function, or constant named default_flat` |
| 载荷类型手写 `Default` | 报 `the trait bound Mark: Default is not satisfied` |
| 泛型参数别叫 `M` | `Component` 派生报 `E0403`（名字重复） |

路径上要么写全 turbofish，要么用别名——**两种都行**：

```rust
Modifier::<Strength>::Flat(Flat::<Strength> { amount: 5.0 })   // 全写
ModStrength::Flat(FlatStrength { amount: 5.0 })                // 别名（配置里用这个）
```

**踩过的坑**：`Modifiers [...]` 列表里**不要**手写 `ModifiesOf(#Hero)`。`RelationshipTarget` 有 `Relationship` 关联类型，宏据此知道对方是谁并**自动补上**；手写一遍就会得到：

```
Bundle <dynamic bundle> has duplicate components: ["voxelith_core::modifier::ModifiesOf"]
```

## `.bsn` 文件加载还没来

`bevy_scene` 文档的原话：scene 由 `bsn!` 或 in the future `.bsn` asset files 创建。管道已备（`ScenePatch` 是 `Asset`、`SceneComponent` 是“加载完自动应用”的组件），**解析器尚未实现**。将来文件是**补丁**语义，不是从零描述实体。
