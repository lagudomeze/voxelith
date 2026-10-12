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

Modifier<Mark> { flat, ratio }    ← 挂在“修正实体”上的组件：平加 + 比例（可以只给一个）

Value / Resourceable                          ← 读出：单值型 / 资源
（公式就写在下面两个读出方法里，各一行）
```

**修正怎么挂到主人身上不在这里**——core 只给修正组件与公式；挂法是应用侧的事（见下面“修正怎么挂”一节）。

两条**故意不提供**的东西：

- **没有泛型载体**（`Value<M>` 那种）：单值型的标记就是它自己，资源只需要一个 `Max<T>`。
- **没有 `resolved(world, owner)`**：写 system 时手里已经有组件和关联列表，不该回头去 `World` 里捞。见下面“system 里怎么用”。

## 属性：每项一个 newtype，全写开

一项七行，**不上宏**——读和改都更直接：

```rust
/// 力量：物理行为的强度。
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Strength(pub f32);

impl Value for Strength {
    fn base(&self) -> f32 { self.0 }
}

// 另外五项：Dexterity / Constitution / Perception / Magic / Willpower，形状完全一样
```

`Value` 的标记就是实现者自己，所以不需要 `type Mark = Self` 这种重复写法。

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
pub trait Value: Component + Sized {
    fn base(&self) -> f32;

    /// 读出：基础值 + Σ每条修正的贡献（比例按基础值算）。
    fn resolve<'a>(&'a self, modifiers: impl Iterator<Item = &'a Modifier<Self>>) -> f32 {
        let base = self.base();
        base + modifiers.map(|m| m.contribution(base)).sum::<f32>()
    }
}

pub trait Resourceable: Component + Sized {
    fn current(&self) -> f32;
    fn base_max(&self) -> f32;

    /// 读出**最终上限**。当前值不走这里。
    fn resolve_max<'a>(&'a self, modifiers: impl Iterator<Item = &'a Modifier<Max<Self>>>) -> f32 {
        let base = self.base_max();
        base + modifiers.map(|m| m.contribution(base)).sum::<f32>()
    }
}
```

那条公式**就写在这两个方法里，各一行**——不足以再抽一个名字出来（改的时候记得两处）。

### system 里怎么用（主要用法）

关联列表来自**你自己定义的挂法**（装备槽、增益列表……），修正来自各自的查询——**不需要 `World`**：

```rust
fn report(
    heroes: Query<(&Strength, &Health, &Gear, &Buffs)>,
    strength_mods: Query<&Modifier<Strength>>,
    health_mods: Query<&Modifier<Max<Health>>>,
) {
    for (strength, health, gear, buffs) in &heroes {
        // 面板：只看装备
        let equipped = strength.resolve(gear.0.iter().filter_map(|m| strength_mods.get(*m).ok()));

        // 结算：装备 + 增益
        let effective = strength.resolve(
            gear.0.iter().chain(buffs.0.iter()).filter_map(|m| strength_mods.get(*m).ok()),
        );

        // 资源上限：这里只算装备
        let max_hp = health.resolve_max(gear.0.iter().filter_map(|m| health_mods.get(*m).ok()));
    }
}
```

`Gear` / `Buffs` 是关系集合，直接 `.iter()` 就行（`RelationshipTarget` 给了 `collection()`，且能 `Deref` 到底层 `Vec<Entity>`）。完整可跑的例子在 `crates/voxelith_core/tests/system_read.rs`——**同一个公式，两个系统喂的集合不同，就有两个结果**。

## 修正：一个 struct 组件

```rust
#[derive(Component, Clone, Debug, Default)]
pub struct Modifier<Mark> {
    pub flat: f32,                    // 平加（负 = 减）
    pub ratio: f32,                   // 比例（按基础值算；负 = 按比例减）
    pub _marker: PhantomData<Mark>,   // 修正指向哪个量，靠类型表达
}

impl<Mark> Modifier<Mark> {
    pub fn new(flat: f32, ratio: f32) -> Self { … }
    pub fn flat(flat: f32) -> Self { Self::new(flat, 0.0) }
    pub fn ratio(ratio: f32) -> Self { Self::new(0.0, ratio) }

    /// 平加与比例**分开算，再相加**。
    pub fn contribution(&self, base: f32) -> f32 {
        self.flat + base * self.ratio
    }
}
```

- **一条修正可以同时带平加和比例**（`+5 力量 且 +50% 基础力量`）——两者分开算、再相加，所以叠加依然可交换、可并行。
- **值是有符号的**：`flat: -3.0` 就是减，`ratio: -0.2` 就是按比例减；同一个量上加减并存。
- `_marker` 可以是**私有**的（实测过：改成私有后 BSN 测试照样过）：`bsn!` 走的是那条 blanket `FromTemplate`，只要 `Clone + Default`，不要求字段对外可见。配置里也不用写它（见下）。
- **struct 没有枚举那两个坑**：不需要 `VariantDefaults`，泛型参数也不怕叫 `M`（那个 `E0403` 是 `Component` 派生在泛型**枚举**上的毛病）。

## 修正怎么挂？——core 不管，这是应用侧的事

`voxelith_core` 只给 [`Modifier`] 与公式，**不规定修正怎么挂到主人身上**：装备、增益、只在某个条件下生效的东西，生命周期与生效规则各不相同，定死一种挂法反而挡路。读出接口只吃一个迭代器——**谁调用，谁决定喂什么**。

一份实测过的做法（住在应用侧；`crates/voxelith_app/src/main.rs` 与 `tests/bsn.rs` 里都有）：

```rust
#[derive(Component, FromTemplate)]   // Entity 没有 Default，必须派生
#[relationship(relationship_target = Equipment)]
struct EquippedOn(Entity);

#[derive(Component, Default)]
#[relationship_target(relationship = EquippedOn, linked_spawn)]
struct Equipment(Vec<Entity>);       // 字段私有
```

- **必须派生 `FromTemplate`**：`Entity` 没有 `Default`，走不了那条 blanket impl。
- **字段必须私有**：`RelationshipTarget` 的要求（防止绕过关系直接改）。
- **`linked_spawn`**：主人销毁时修正实体**自动一起消失**（实测）；“卸下装备 = 销毁那个实体”因此不用手写清理。
- **它是 `RelationshipTarget`，所以 `bsn!` 里能直接写成列表**（见下）。
- 需要“装备算、增益不算”这类区分时，**再定义一套**：`tests/system_read.rs` 里就定义了两套（装备槽 + 增益列表），面板系统只喂装备、结算系统两套都喂——同一个公式，两个结果。

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
    Equipment [
        ModStrength { flat: 5.0 },
        ModStrength { flat: -2.0 },                   // 减
        ModStrength { ratio: 0.5 },
        ModHealthMax { flat: 20.0 },
    ]
})
```

**没写的字段会自动补默认值**，所以配置里只写关心的那几个：`ModStrength { flat: 5.0 }`、`ModStrength { ratio: 0.5 }`、`ModStrength { flat: 5.0, ratio: 0.5 }` 都行。路径上写全 turbofish 或用别名，两种都行。

> **留档**：修正组件曾经用泛型枚举写过，实测过三条限制——枚举要 `VariantDefaults`（否则报 `no variant ... named default_flat`）、载荷类型要能默认、泛型参数不能叫 `M`（`Component` 派生在泛型枚举上会自己引入 `M`，撞名 `E0403`）。**改成 struct 后这些都不需要了**；将来若再用枚举组件（比如行动的状态机），记得这三条。

**踩过的坑**：列表（`Equipment [...]`）里**不要**手写 `EquippedOn(#Hero)`。`RelationshipTarget` 有 `Relationship` 关联类型，宏据此知道对方是谁并**自动补上**；手写一遍就会得到：

```
Bundle <dynamic bundle> has duplicate components: ["bsn::EquippedOn"]
```

## `.bsn` 文件加载还没来

`bevy_scene` 文档的原话：scene 由 `bsn!` 或 in the future `.bsn` asset files 创建。管道已备（`ScenePatch` 是 `Asset`、`SceneComponent` 是“加载完自动应用”的组件），**解析器尚未实现**。将来文件是**补丁**语义，不是从零描述实体。
