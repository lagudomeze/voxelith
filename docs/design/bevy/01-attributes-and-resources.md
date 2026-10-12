# 属性与资源（Bevy）

**这一篇只放真的取决于 Bevy 的取舍。** 模型见[属性与资源](../01-attributes-and-resources.md)，组件形状的约定见[组件约定](../00-component-conventions.md)。

**每条都实测过**：Bevy **0.19.1**，工作区里跑通（`cargo test`），另有验证工程 `D:\code\rust\voxelith-bsn-spike`。

## 命名规则（先看这条）

使用方会**同时 glob 导入** `bevy_ecs::prelude::*` 与本 crate 的 prelude，所以**本 crate 的公开名字不能与 `bevy_ecs::prelude` 撞**。实测踩到两个：

| 撞的名字 | 对方是什么 | 症状 | 改成 |
| --- | --- | --- | --- |
| `Add` | `bevy_ecs::lifecycle::Add` | `E0659` 歧义，直接用不了 | **`Flat`** |
| `Resource` | `bevy_ecs::resource::Resource`（trait） | **被静默遮蔽**——不报错，但那一层的意思已经变了 | **`Pool`** |

**自查办法**：对照 `bevy_ecs` 里的 `pub mod prelude { ... }` 名单过一遍。现在用到的名字（`Value` / `Pool` / `Flat` / `Ratio` / `Modifiers` / `ModifiesOf` / `Totals`）都不在名单里。

## 标记类型

每个“可修正的量”一个零大小的标记，**必须 `Clone + Copy + Default`**：

```rust
#[derive(Clone, Copy, Debug, Default)]
pub struct StrengthStat;
#[derive(Clone, Copy, Debug, Default)]
pub struct HealthMax;
```

**为什么要这三个**：泛型组件上的 `#[derive(Clone)]` 是 imperfect derive——它会生成 `impl<M: Clone> Clone for Flat<M>`。标记类型不满足，编译就停在 `the trait Clone is not implemented for StrengthStat`。给标记补上 `Clone + Copy + Default` 之后，泛型组件的 `#[derive(Component, Clone, Debug, Default)]` 全都能直接用。

## 两种载体

```rust
/// 单值型：只有一个基础值（属性这类）
#[derive(Component, Clone, Debug, Default)]
pub struct Value<M> {
    pub base: f32,
    pub _marker: PhantomData<M>,
}

/// 资源池型：一个当前值 + 一个基础上限。
/// 只有当前值是状态（由事件改）；上限走同一套修正。
#[derive(Component, Clone, Debug, Default)]
pub struct Pool<M> {
    pub current: f32,
    pub base_max: f32,
    pub _marker: PhantomData<M>,
}
```

## 修正：平加与比例

```rust
#[derive(Component, Clone, Debug, Default)]
pub struct Flat<M> { pub amount: f32, pub _marker: PhantomData<M> }

#[derive(Component, Clone, Debug, Default)]
pub struct Ratio<M> { pub value: f32, pub _marker: PhantomData<M> }
```

## 别名：把标记绑到名字上

```rust
pub type Strength = Value<StrengthStat>;
pub type Health   = Pool<HealthMax>;

pub type FlatStrength  = Flat<StrengthStat>;
pub type RatioStrength = Ratio<StrengthStat>;
pub type FlatHealthMax = Flat<HealthMax>;
```

**实测**：别名与泛型 turbofish **都能写在 `bsn!` 里**：

```rust
Strength { base: 10.0 }                 // 别名（具名字段）
Flat::<StrengthStat> { amount: 5.0 }    // 泛型 turbofish
FlatStrength { amount: 3.0 }            // 别名
Health { current: 8.0, base_max: 30.0 } // 资源池型
```

## 统一读取：收集 + 公式

```rust
/// 指向某个量的修正总和。
#[derive(Default, Clone, Copy, Debug)]
pub struct Totals { pub add: f32, pub ratio: f32 }

impl Totals {
    /// 公式：基础值 + Σ平加 + 基础值 × Σ比例。
    /// 纯函数——不带 World、不遍历，顺序无关，可并行算完再归约。
    pub fn resolve(&self, base: f32) -> f32 {
        base + self.add + base * self.ratio
    }
}

/// 遍历主人身上的修正，归约成 Totals（纯读）。走主人自己的 `Modifiers`
/// 组件 + 那几个修正实体，**不做全库扫描**。
pub fn collect<M: Send + Sync + 'static>(world: &World, owner: Entity) -> Totals {
    let mut totals = Totals::default();
    if let Some(mods) = world.get::<Modifiers>(owner) {
        for m in mods.iter() {
            if let Some(a) = world.get::<Flat<M>>(m)  { totals.add   += a.amount; }
            if let Some(r) = world.get::<Ratio<M>>(m) { totals.ratio += r.value;  }
        }
    }
    totals
}

/// 单值型：读出“基础值 + 修正”。
pub fn resolve_value<M: Send + Sync + 'static>(world: &World, owner: Entity) -> Option<f32> {
    let base = world.get::<Value<M>>(owner)?.base;
    Some(collect::<M>(world, owner).resolve(base))
}

/// 资源池型：读出“最终上限”。当前值不走这里——它是状态。
pub fn resolve_max<M: Send + Sync + 'static>(world: &World, owner: Entity) -> Option<f32> {
    let base = world.get::<Pool<M>>(owner)?.base_max;
    Some(collect::<M>(world, owner).resolve(base))
}
```

**实测**：同一套 `collect` + `resolve` 同时服务单值型与资源池型——力量 `10 +5 +50% → 20`，血量上限 `60 +20 +50% → 80`，而**当前值 30 不被修正改动**。

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

## 在 `bsn!` 里写“角色 + 它的修正”

```rust
world.spawn_scene(bsn! {
    #Hero
    Strength { base: 10.0 }
    Health { current: 30.0, base_max: 60.0 }
    Modifiers [
        FlatStrength { amount: 5.0 },
        RatioStrength { value: 0.5 },
        FlatHealthMax { amount: 20.0 },
    ]
})
```

**踩过的坑**：列表里**不要**手写 `ModifiesOf(#Hero)`。`RelationshipTarget` 有 `Relationship` 关联类型，宏据此知道对方是谁并**自动补上**；手写一遍就会得到：

```
Bundle <dynamic bundle> has duplicate components: ["voxelith_core::modifier::ModifiesOf"]
```

## `.bsn` 文件加载还没来

`bevy_scene` 文档的原话：scene 由 `bsn!` 或 in the future `.bsn` asset files 创建。管道已备（`ScenePatch` 是 `Asset`、`SceneComponent` 是“加载完自动应用”的组件），**解析器尚未实现**。将来文件是**补丁**语义，不是从零描述实体。
