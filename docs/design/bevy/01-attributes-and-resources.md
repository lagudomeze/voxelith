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
Health(pub f32)                   ← 资源的当前值（状态，不走修正）
Max<Health>(pub f32, PhantomData) ← 资源的上限，**本身也是个单值型**（可修正）
HealthMax = Max<Health>           ← 别名：bsn! 里写 `HealthMax(60.0)`

Modifier<Mark> { flat, ratio }    ← 挂在“修正实体”上的组件：平加 + 比例（可以只给一个）

Value { raw, raw_mut }            ← 唯一的量接口：属性/上限/回复速率/当前值组件都实现它
（求和不在 core —— 它在 rules 的重算系统里，见 02-derived-rules.md）
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
    fn raw(&self) -> f32 { self.0 }
    fn raw_mut(&mut self) -> &mut f32 { &mut self.0 }
}

// 另外五项：Dexterity / Constitution / Perception / Magic / Willpower，形状完全一样
```

`Value` 的标记就是实现者自己，所以不需要 `type Mark = Self` 这种重复写法。

## 资源：拆成两个组件

```rust
#[derive(Component, Clone, Copy, Debug, Default)]
#[require(Max<Health>)]      // 有当前值就有上限（传递依赖：上限又 require 了它的 Total）
pub struct Health(pub f32);  // 当前值：状态，由事件改，不挂 Total

impl Value for Health {
    fn raw(&self) -> f32 { self.0 }
    fn raw_mut(&mut self) -> &mut f32 { &mut self.0 }
}

// 上限本体在 core::value，这里只起名（bsn! 里写不出 `Max::<Health>` 那种路径）
pub type HealthMax = Max<Health>;
pub type ModHealthMax = Modifier<Max<Health>>;
```

**上限不是"资源的一部分"，它自己就是一个可修正的量**——所以重算只有一条路径：

```rust
recompute::<Strength, L>      // 属性
recompute::<Max<Health>, L>   // 血量上限：同一个泛型系统
```

## 量：一个 trait

```rust
pub trait Value: Component<Mutability = Mutable> + Sized {
    fn raw(&self) -> f32;                 // 取数（属性的基础值 / 资源的当前值 / 上限的基础值）
    fn raw_mut(&mut self) -> &mut f32;    // 只给“改值的唯一通路”（Adjust）用
}
```

求和不在 core：它要按项目自己的挂法去找修正，所以住在 `voxelith_rules::recompute`。

**求和不在 traits 里**——它要按项目自己的挂法去找修正，所以住在 `voxelith_rules::recompute`（见 [02-derived-rules.md](02-derived-rules.md)）。core 只给"一条修正贡献多少"（`Modifier::contribution`）与"基础值是多少"。

### 读的人：只看 `Total`

```rust
// 重算的人（知道挂法）：算完写进缓存
fn recompute_strength(
    mut owners: Query<(&Strength, Option<&Gear>, Option<&Modifier<Strength>>, &mut Total<Strength>)>,
    modifiers: Query<&Modifier<Strength>>,
) { … }

// 读的人：一个挂法都不认识
fn report(owners: Query<(&Total<Strength>, &Total<Max<Health>>, &Health)>) {
    for (strength, max_hp, health) in &owners {
        let _ = (strength.value, max_hp.value, health.current());
    }
}
```

`Gear` / `Buffs` 是关系集合，`link.collection().iter()` 就能遍历（要用到 `RelationshipSourceCollection` 这个 trait）。完整可跑的例子：重算半边见 `crates/voxelith_core/tests/system_read.rs`，真实实现见 `crates/voxelith_rules/src/recalc.rs`。
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
    Health(30.0)                                   // 当前值：tuple 写法
    HealthMax(60.0)                                // 上限：别名 + tuple，phantom 自动默认
                                                   //（实测：别名在 bsn! 里能当类型用；
                                                   //  在 Rust 代码里不能当构造函数，用 Max::new；
                                                   //  `#[derive(Component)]` 也放不到别名上——E0774
                                                   //  只能放在 struct/enum/union 上。别名本身就是同一个
                                                   //  类型，Component 由 Max<Q> 提供，不需要再 derive）
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
## 变更检测的两条坑（都实测过）

1. **`Changed<T>` 连“刚添加”也算**。刚 spawn 的实体，下一帧查 `Changed<T>` 会全部命中——测试里要断言“只有被改的那条算变”，得先跑一帧预热。
2. **别把 `Mut<T>` 提前转成 `&mut T`**。`&mut modifier` 这一步会调 `DerefMut`，Bevy 就此标记“变了”，哪怕一个字都没写——于是每帧都标记，下游被连锁触发。想“只在变了才写”，比较与赋值都要留在 system 里（详见 [02-derived-rules.md](02-derived-rules.md) 里那个坑的复现过程）。

配套的写入纪律：**用 `get_mut(实体)` 逐个取，不要遍历所有同类组件**——没事发生的实体就不会被碰。
## 组件字段不是 `f32` 时，`bsn!` 里的 `S(1.0)` 写不出来（实测）

`bsn!` 的元组写法 `S(1.0)` 是**直接把值当字段**用的，不做 `.into()`：

```
#[derive(Component, Clone, Copy, Debug, Default)]
struct S(Data);            // Data 是自定义结构

bsn! { S(1.0) }            // ✗ error[E0308]: expected `Data`, found floating-point number
bsn! { S(Data { … }) }     // ✓
```

所以**把「一个 f32」换成「一个结构体」会让所有场景里的 `Strength(10.0)` 破掉**。`#[template(某类型)]` 能给字段换 Template 类型，但那个类型得在 `bsn!` 里被写出来（`Strength(某模板(10.0))`），并没有更短。

结论：**要保住 `Strength(10.0)` 这种写法，量的基础值就得是 `f32` 字段本身**；缓存要放在别处（伴生组件）或者接受 `template_value(Strength::new(10.0))` 这种啰嗦写法。
## 有效值靠 `Total<T>` 缓存（`#[require]` 自动带上）

每个量（属性、回复速率、资源上限）都有一个伴生缓存组件，由 `#[require]` 自动挂上，**`bsn!` 里不用写**：

```rust
#[derive(Component, Clone, Copy, Debug, Default)]
#[require(Total<Strength>)]
pub struct Strength(pub f32);

#[derive(Component, Clone, Copy, Debug, Default)]
#[require(Total<Max<Health>>)]
pub struct Max<Q: Value>(pub f32, PhantomData<Q>)
```

读的人只读 `Total`，重算的人（知道修正怎么挂）写它。这带来一个副产品：**基础值可以保持 `pub f32`**（`bsn!` 要能构造它），而"最终值"永远从 `Total` 读——两者不再混在一起。

实测：`#[require(Total<某量>)]` 这种**带泛型实例**的 required component 可用（`#[require(Modifier<To>)]` 同理）。
## 第三条路：自己定义 Template 类型（现在没用，先记着）

`bsn!` 里怎么写一个组件，除了 blanket（`Clone + Default + Unpin`）和 `#[derive(FromTemplate)]`，还有第三条路：**手写 `FromTemplate`，并把 `Template` 指成另一个类型**——Bevy 自己就是这么给资产留出路的：

```rust
impl<T: Asset> FromTemplate for Handle<T> {
    type Template = HandleTemplate<T>;          // ← 不是 Handle<T> 本身
}

impl<T: Asset> Template for HandleTemplate<T> {
    fn build_template(&self, context: &mut TemplateContext) -> Result<Handle<T>> { … }
}
```

这样"字段是 `Handle<T>`"与"bsn 里能写一段路径字符串"两件事就分开了。

**什么时候值得用**：字段在 Rust 里是结构体 / 引用 / 枚举，但在 `bsn!` 里希望写得更简单（路径、id、简短形式）。**代价**是每个这样的组件要自己写一对类型 + 两个 impl（`FromTemplate` 与 `Template`）。

**我们暂时没用**：`Strength(10.0)` 是靠"字段本身就是 `f32`" + 有效值缓存放伴生组件（[`Total`]）解决的；实测过字段换成结构体后 `bsn! { S(1.0) }` 会 `E0308` ✗（见上一节）。真需要"结构体字段 + 简便写法"时，这条路是现成的。

**一句提醒**：`bsn!` 里能写什么，取决于**那一段文本怎么变成模板值**——`Handle` 能收路径字符串是 Bevy 给它铺好的，**数字不会自动 `into()`**（这正是我们 `E0308` 那条的原因）。要用这条路，先写个最小探针验一遍再推广。
