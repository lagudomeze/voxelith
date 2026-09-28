# 战斗系统设计（R47–R63）

## 1. 分工总览

| 模块 | 层 | 只做这些事 | 绝不做的 |
|---|---|---|---|
| `health` | L0 atoms | 存 `Health`；消费 `ModifyHealthMessage` 做纯数值结算 | 不知道护甲/闪避/暴击/伤害类型 |
| `combat` | L1 behaviors | 存战斗数据、算战斗公式、发战斗事件（R57） | 任何视觉表现（R58、R105） |
| `formula` | L1（`combat` 内） | 暴击、受疗加成、减伤等中间计算；拦截事件改 `amount` 再转发（R54） | 直接写 `Health` |
| `targeting` | L1 | 只做"寻找与判定"（R59） | 定义技能数值 |
| `skills` | L1 | 只做"技能数值与释放"（R60） | 碰撞检测 |
| `lifecycle` | L0/L1 | 通用实体销毁计时 | 锁死在 combat（R61） |
| presentation | L2 | 监听 `DamageRequest` 播特效（R55） | 算伤害、改血量 |

## 2. 事件流水线（R50）

```
Skill（L1 skills：技能数值与释放）
   │  发出 DamageRequest { source, target, damage_type, base_amount, context }
   ▼
L1 formula（命中 / 闪避 / 减伤 / 暴击）        ← targeting 提供"打到了谁"（R59）
   │  拦截并修改 amount，再转发
   ▼
ModifyHealthMessage { entity, amount }           ← 纯数值，负数=伤害，正数=治疗（R49、R51）
   ▼
L0 health 执行（唯一写 Health.0 的地方）（R56）
   │  结算后若 current <= 0
   ▼
DeathEvent（EntityEvent，target = 阵亡实体）
   ▼
L2 表现：死亡动画、掉落、音效
```

**治疗与伤害共用 `ModifyHealthMessage`，不区分事件类型。**（R51）
**自然恢复（Regeneration）作为 Buff，由独立系统定期发出正数 `ModifyHealthMessage`。**（R52）
**治疗技能的治疗加成计算在 L1，`health` 模块不感知。**（R53）

## 3. 关键边界

### 3.1 `health` 不认识 `DamageType`（R47、R98）

- `DamageType` 定义在 L1（`behaviors` 中），**不放 `health`**。（R48）
- `health` 只处理 `ModifyHealthMessage`：纯数值，负数=伤害，正数=治疗。（R49）
- 因此 `health` 里出现 `Armor`、`Dodge`、`Crit`、`DamageType` 的任何符号都是违规。

```rust
// ✅ health.rs（L0）——只有数值
#[derive(Message)]
pub struct ModifyHealthMessage { pub entity: Entity, pub amount: i32 }
```

```rust
// ✅ behaviors/combat/components.rs（L1）——带类型与上下文
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DamageType { Physical, Fire, Ice, True }

#[derive(Message)]
pub struct DamageRequest {
    pub source: Entity,
    pub target: Entity,
    pub damage_type: DamageType,
    pub base_amount: i32,
}
```

### 3.2 公式拦截式转发（R54）

```rust
// ✅ behaviors/combat/systems.rs（L1）
/// 命中/闪避/减伤/暴击都在这里算完，只把纯数值交给 L0。
pub fn apply_damage(
    mut requests: MessageReader<DamageRequest>,
    armors: Query<&Armor>,
    mut out: MessageWriter<ModifyHealthMessage>,
) {
    for req in requests.read() {
        if !roll_hit(req) { continue; }                 // 命中/闪避
        let mitigated = mitigate(req.base_amount, armors.get(req.target)); // 减伤
        let final_amount = roll_crit(mitigated);        // 暴击
        out.write(ModifyHealthMessage { entity: req.target, amount: -final_amount });
    }
}
```

要点：

- L1 **不改** `Health`，只发事件（R13）。
- 结算规则集中在一处，方便调参与测试。
- 表现层不需要知道公式，它只监听 `DamageRequest`（R55）。
- 如果是"就地改写再放行"的模式（暴击倍率、易伤层数），用 `MessageMutator<DamageRequest>`；同一系统里 `MessageReader<DamageRequest>` + `MessageWriter<DamageRequest>` 会资源冲突。详见 [bevy-events.md](bevy-events.md#52-messagemutator拦截式改写对应-r54)。

### 3.3 表现层监听 `DamageRequest`，不监听 `ModifyHealthMessage`（R55）

原因：`DamageRequest` 含 `DamageType` 与上下文，能决定"播什么特效"；`ModifyHealthMessage` 只是数字，且治疗/伤害混在一起，无法区分表现。

> **分层提醒**（见 [bevy-events.md](bevy-events.md) §2.4）：这里说的"监听"是**通知层**——表现系统读到事件后**启动**特效（起实体 / 插组件 / 写初值）。
> 特效播几帧、怎么渐隐属于**跨帧状态**，由组件 + 每帧系统推进，与"监听哪个事件"无关。
> 一句话：**事件决定"播什么"，组件决定"播多久"。**

```rust
// ✅ L2 presentation：只读事件、只生成表现
fn spawn_damage_vfx(mut requests: MessageReader<DamageRequest>, /* ... */) {
    for req in requests.read() {
        match req.damage_type { DamageType::Fire => { /* 火焰特效 */ }, _ => {} }
    }
}
```

### 3.4 `health` 是纯数值执行器（R56）

`Health.current` 的**唯一**写入口是 `apply_health_change`。任何其它地方出现 `health.current = ...` 都是违规，包括 L2、L1、测试外的所有模块。

## 4. 模块职责细则（R57–R63）

### `combat`（R57、R58、R62）

只做三件事：**存战斗数据、算战斗公式、发战斗事件**。

- 禁止任何视觉表现代码（R58）。
- 禁止 `SpriteBundle` / `Mesh` / `Text`（R105）。
- `combat/plugin.rs` 只注册逻辑系统，不注册 UI 更新、动画播放、粒子特效（R62）。

### `targeting`（R59）

只做"寻找与判定"：范围筛选、视线判定、优先级排序、命中目标列表。
不定义技能数值——数值属于 `skills`。

### `skills`（R60）

只做"技能数值与释放"：冷却、消耗、伤害基数、施法朝向。
不做碰撞检测——判定属于 `targeting`。

### `lifecycle` 与 `Lifetime`（R61、R63）

- `Lifetime` 是**通用实体销毁计时器**，所有模块都可能用到（R63）。
- 因此它属于通用实体生命周期领域（`core` 或等价领域模块），**不锁死在 `combat`**（R61）。
- 在 `axiom` 中的落地位置见 [OPEN-QUESTIONS.md](OPEN-QUESTIONS.md)（Q1：领域名 `core` 与 R21 的禁名清单存在张力）。

## 5. 战斗模块依赖图

```
skills ──► formula ◄── targeting
              │
              ▼
      ModifyHealthMessage (定义于 atoms::health)
              │
              ▼
        atoms::health（执行）
              │
              ▼
         DeathEvent ──► L2 presentation
```

允许的 import 方向：`skills → health`、`formula → health`、`presentation → combat`（事件类型）。
禁止：`health → combat`（R47）、`presentation → health` 的**写**访问（R16）。

## 6. 战斗系统自检清单

- [ ] 新伤害来源是否走 `DamageRequest`，而不是直接改血量？
- [ ] 公式是否全部在 L1，没漏到 L2？
- [ ] `health` 是否仍然只认识 `ModifyHealthMessage`？
- [ ] 表现是否监听 `DamageRequest` 而非 `ModifyHealthMessage`？
- [ ] `combat` 里是否出现任何渲染类型？
- [ ] 是否新增了模式匹配 `DamageType` 的表现代码（应在 L2）？
