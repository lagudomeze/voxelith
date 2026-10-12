# 组件约定

**为什么有这一篇**：我们**不自顶向下设计**，而是先把小的组件定好，再用 BSN 把它们组合成更复杂的效果；等 BSN 支持文件加载之后，这些组合由数据文件写。三条约定是让这条路走得通的前提——每个组件都要满足。

## 1. 纯数据，且能默认构造

每个组件都是 **`Component + Default + Clone`**，字段是简单的纯数据。

- **`Default` 不是装饰**：BSN 里"只写要改的那个字段、其余取默认"（`CompA { name: val }`、`CompA(val)`）就建立在这上面；没有 `Default` 就得为它手写 Template。
- **不加 `Reflect`**：BSN 不需要它。依据是源码，不是印象——`bevy_ecs-0.19.1/src/template.rs:404` 对 `Clone + Default + Unpin` 有 blanket impl：

  ```rust
  impl<T: Clone + Default + Unpin> FromTemplate for T { type Template = T; }
  ```

  而 `bevy_scene` 全库只有 6 处反射引用，全在 `SceneComponent` 自己（它要能被补丁）与资产加载器的 `TypePath` 上。
- 将来若要"外部程序读运行中的世界状态"（`bevy_remote` / BRP 是靠 `AppTypeRegistry` + `ReflectComponent` 工作的）或要文件加载，**再按需派生**——那是局部改动，不阻碍现在。

## 2. 字段具名，语义单一

- 将来 `.bsn` 文件加载是**补丁**语义：在既有组件上改某个字段。所以字段名要能直接读懂，一个字段只表达一件事。
- **多字段的组件用具名字段**：补丁按名字写，位置覆盖不好读。
- **只有一个值的用 newtype**（`Strength(pub f32)`）：`bsn!` 对它有一条专门的写法 `Strength(10.0)`，比 `{ base: 10.0 }` 更短也更清楚。

## 3. 一件事一个组件，组合优先于枚举

- 不写"资源类型 = {血, 魔, 精, 悖论, 均衡, 仇恨}"这种枚举，而写"**一个数值 + 一串规则组件**"。
- 不写"这个角色是法师所以有法力"，而写"这个角色挂了法力组件"。
- 判据沿用需求里那条：**带引擎规则的固定在代码里，只用于比较相等性的名字走数据**（`CT-02`）。
