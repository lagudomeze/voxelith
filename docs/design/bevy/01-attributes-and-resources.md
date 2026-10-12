# 属性与资源（Bevy）

**这一篇只放真的取决于 Bevy 的取舍。** 模型见[属性与资源](../01-attributes-and-resources.md)，组件形状的约定见[组件约定](../00-component-conventions.md)。

**每条都实测过**：Bevy **0.19.1**，最小依赖（`bevy_app` / `bevy_asset` / `bevy_ecs` / `bevy_scene`），编过也跑过。

## 组件清单与派生

```rust
// 六项属性：newtype 组件，手写 Default
#[derive(Component, Clone, Debug)]
pub struct Strength(pub f32);
impl Default for Strength {
    fn default() -> Self { Self(0.0) }
}
// …另外五项同理

// 资源：只有当前值，具名字段（将来 .bsn 文件按字段名打补丁）
#[derive(Component, Clone, Debug, Default)]
pub struct Health { pub current: f32 }
#[derive(Component, Clone, Debug, Default)]
pub struct Stamina { pub current: f32 }
```

- **不派生 `Reflect`**（P0 不需要；BSN 只要 `Clone + Default`）。

## 修正：泛型加值组件 + 类型别名

```rust
#[derive(Component, Clone, Debug)]
pub struct Add<C> {
    pub amount: f32,
    pub _marker: PhantomData<C>,
}

impl<C> Add<C> {
    pub fn new(amount: f32) -> Self { Self { amount, _marker: PhantomData } }
}

// `#[derive(Default)]` 会给 C 加上 Default 约束，所以手写
impl<C> Default for Add<C> {
    fn default() -> Self { Self::new(0.0) }
}

// 按基础值的比例（0.5 = +50% 基础值）
#[derive(Component, Clone, Debug)]
pub struct Ratio<C> {
    pub value: f32,
    pub _marker: PhantomData<C>,
}
impl<C> Default for Ratio<C> {
    fn default() -> Self { Self { value: 0.0, _marker: PhantomData } }
}

pub type AddStrength = Add<Strength>;
pub type AddMaxHealth = Add<MaxHealth>;       // 派生量用标记类型
pub type RatioStrength = Ratio<Strength>;      // 比例同样用别名
```

**实测结果**：

| 写法 | 在 `bsn!` 里 | 结果 |
| --- | --- | --- |
| 泛型 turbofish | `Add::<Strength> { amount: 5.0 }` | ✅ 通过 |
| 类型别名 | `AddStrength { amount: 3.0 }` | ✅ 通过 |
| 具名字段补丁 | 未提到的 `_marker` 自动取默认 | ✅ 通过 |

两条硬性注意：

- **`PhantomData<C>` 是必需的**：`Add<C>(f32)` 会因为"泛型参数 `C` 没被用到"编译不过。
- **`Default` 必须手写**：派生版本会给 `C` 加 `Default` 约束，而标记类型不一定有。

**派生量要有标记类型**：血量上限、护甲这类量不存字段（`01` 里"一切皆修正"），但要能被修正，就得有一个**类型**当目标——所以会有一批 `MaxHealth` / `Armour` 这样的零大小标记类型。

## relationship：修正指回主人

```rust
#[derive(Component, FromTemplate)]
#[relationship(relationship_target = Modifiers)]
pub struct ModifiesOf(pub Entity);

#[derive(Component, Default)]
#[relationship_target(relationship = ModifiesOf, linked_spawn)]
pub struct Modifiers(Vec<Entity>);   // 字段私有
```

- **必须派生 `FromTemplate`**：`Entity` 没有 `Default`，走不了那条 blanket impl。
- **字段必须私有**：`RelationshipTarget` 的要求（防止绕过关系直接改）。
- **`linked_spawn` 让主人销毁时修正自动一起消失**（实测：主人 `despawn` 后两个修正实体都查不到了）。"卸下装备 = 销毁那个修正实体"因此不用手写清理。

## 在 `bsn!` 里写"角色 + 它的修正"

```rust
world.spawn_scene(bsn! {
    #E
    Strength(10.0)
    Modifiers [
        AddStrength { amount: 5.0 },
        AddStrength { amount: 7.0 },
    ]
})
```

**踩过的坑**：列表里**不要**手写 `ModifiesOf(#E)`。`RelationshipTarget` 有 `Relationship` 关联类型，宏据此知道对方是谁并**自动补上**；手写一遍就会得到：

```
Bundle <dynamic bundle> has duplicate components: ["bsn_spike::ModifiesOf"]
```

## 读最终值

```rust
/// 读出规则：基础值 + Σ平加 + 基础值 × Σ比例
/// 纯加法归约：登记顺序不影响结果，也可以并行各算一份再求和。
fn strength_total(world: &World, owner: Entity, base: f32) -> f32 {
    let mut add = 0.0;
    let mut ratio = 0.0;
    if let Some(mods) = world.get::<Modifiers>(owner) {
        for m in mods.iter() {
            if let Some(a) = world.get::<Add<Strength>>(m) {
                add += a.amount;
            }
            if let Some(r) = world.get::<Ratio<Strength>>(m) {
                ratio += r.value;
            }
        }
    }
    base + add + base * ratio
}
```

走主人自己的 `Modifiers` 组件 + 那几个修正实体，**不做全库扫描**——这是"每次读时算、不缓存"能站得住的前提。

**实测**：基础 10，加 +5 平加与 +50% 比例 → **20**；把这两条修正**反向登记**进 Modifiers 再读，仍是 **20**（顺序无关）。

## Bevy 现状备注

- **`.bsn` 文件加载在 0.19 还没有**。`bevy_scene` 文档的原话是 scene 由 `bsn!` 或 "**in the future** `.bsn` asset files" 创建；管道已备（`ScenePatch` 是 `Asset`，`SceneComponent` 是"加载完自动应用"的组件），但解析器未实现。将来文件是**补丁**语义，不是从零描述实体。
