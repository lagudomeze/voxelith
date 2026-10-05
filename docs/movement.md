# 移动与空间（技术参考）

> **状态**：本文是**技术参考**，不是规则集 —— 里面的条目**还没有 `R<n>` 编号**。
> 按 [README.md](README.md) §状态说明的流程：先记在 [OPEN-QUESTIONS.md](OPEN-QUESTIONS.md)，
> 确认后才落到文档并分配编号。本文里每一节都标了**已确认 / 待确认**。
>
> **已确认**：空间模型（格子 = 体素块）、时间源、位置 = 时间的纯函数、分层归属。
> **待确认**：移动与动作的关系（Q28）、打断窗口（Q29）、施法请求与路径（Q30）、空间细节（Q31）。

## 1. 一句话

**时间是唯一权威，位置是时间的纯函数。算一次路径，执行很多步。**

```text
逻辑位置 = IVec2 格子        视觉位置 = f(now)（每帧重算，不是累加）
路径     = 决策时算一次       一次 Move 只走一格，多格靠队列串联
```

## 2. 空间模型：格子 = 体素块 `已确认`

`GridPosition` 不是在体素世界旁边新造的一套坐标 —— **格子就是体素块**。

这带来四样**免费**的东西：

| 得到什么 | 靠什么 |
|---|---|
| **可走性检查** | 直接查 `atoms::world` 的 `get_voxel`，**不需要第二张地图** |
| 地形减速 / 视线遮挡 / 地面高度 | 同一份数据 |
| `R107` 自动满足 | 坐标本来就源自 `VoxelPos`，渲染层**没有机会**自己发明坐标 |
| 分层干净 | 全是 L0 数据 → L1 的规则直接查 → **L2 完全不参与** |

尺度上也合适：`CHUNK_SIZE = 32`，战场约 26 格 —— **正好落在一个区块内**。

**未定细节见 [OPEN-QUESTIONS.md](OPEN-QUESTIONS.md) Q31**（地面高度、`IVec2` 的来源、多层地形）。

> ⚠️ **`IVec2` 现在在 `axiom` 里拿不到**（已核实）：`axiom` 的直接依赖恰好是 R5 白名单 5 个
> + 登记表 3 个，**依赖树里没有 `bevy_math`、没有 `glam`**，源码里也一个 `Vec2`/`IVec2` 都没有。
> 倾向**直接复用 `atoms::world::VoxelPos`**（去掉 Y 的视图），既不加依赖也不造平行类型。

## 3. 时间源：`Time<Virtual>`，且 `CombatPhase` 是冻结的唯一权威 `已确认`

用 Bevy 的 `Time<Virtual>`，**不自己造时钟**（和 `behaviors::time_scale` 是同一套）：

| 需求 | 做法 |
|---|---|
| 当前游戏时间 | `time.elapsed_secs_f64()` |
| 变速 | `time.set_relative_speed(..)` |
| 防卡顿 | `time.set_max_delta(..)` |

### 但**不要**自己调 `pause()` / `unpause()` / `set_relative_speed()`

冻结时间的唯一权威是 `CombatPhase`：`behaviors::time_scale` 的 `drive_virtual_time`
在 `PreUpdate` 按相位设倍率，测试 `delta_and_phase_never_disagree` 专门钉住
"delta 与相位绝不说两套话"。

移动这边若再调一次 `time.pause()`，就出现**两个冻结源**：`CombatPhase` 说 Resolving
而 `is_paused` 还是 `true`，谁先解冻？—— 这种"两个开关管一件事"正是本项目一路在防的。

> **移动只读 `Res<Time<Virtual>>`，不写它。**

## 4. 位置 = 时间的纯函数 `已确认`

```rust
/// 从 `Motion` 重算，**不是累加**。暂停、变速、回放天然正确。
pub fn position_at(motion: &Motion, now: f64) -> Vec3 {
    if motion.from == motion.to {
        return ground_of(motion.to);          // 高度从地形查，不是 0.0
    }
    let span = motion.ends_at - motion.started_at;
    let t = ((now - motion.started_at) / span).clamp(0.0, 1.0) as f32;
    let from = ground_of(motion.from);
    let to = ground_of(motion.to);
    from.lerp(to, t)
}
```

**为什么必须是纯函数**：反制窗口会把虚拟时间倍率设成 0。累加式推进（`pos += v * delta`）
在"冻结 → 恢复"的边界上会留一帧漂移；`f(now)` 不会。

### 它对现有代码的推论：**别造第二个时间轴**

这条批评**同样适用于现有实现**：`behaviors::action` 的 `tick_actions` 是

```rust
action.elapsed += delta;          // behaviors/action/mod.rs
```

也就是我们已经有了一条**累加式**时间轴。若移动用绝对时间戳、动作继续累加，两者在
`Time<Virtual>` 倍率变化时会按**不同的误差**漂移，出现"动作演完了但位移还差一点"。

**结论**：采纳移动设计时**一并把 `Action` 统一成绝对时间戳**
（存 `started_at` / `ends_at`，`elapsed` 退化成 getter）。

## 5. 分层：两个系统跨两层，纯函数必须回 L0 `已确认`

| 东西 | 碰什么 | 层 | 依据 |
|---|---|---|---|
| `GridPosition` / `Motion` / `Speed` | 只有 `Entity` / 格子 | **L0** | 零依赖 |
| `position_at` | 格子 → 世界坐标的**纯函数** | **L0** | `R107`：渲染层不许自己发明世界坐标；先例是 `VoxelPos::to_world` |
| `step_motion` | `Time<Virtual>` + 多组件 | **L1** | `R11`：需要多个组件同时在场 |
| `update_transform` | 写 `Transform` | **L2** | `R10`：`Transform` 禁止进 L0/L1 |

所以上游那份设计里"两个系统放一个 `chain()`"的写法要**拆开跨层**：
纯函数回 L0，搬运工留 L2。否则"格子 ↔ 世界"的换算会变成渲染层的私货。

## 6. 组件

```rust
// L0（零依赖）
pub struct GridPosition(pub IVec2);          // 逻辑位置，永远是合法格子
pub struct Speed(pub f32);                   // 格 / 秒，每实体独立
pub struct Motion {
    pub from: IVec2,        // 当前这一步
    pub to: IVec2,
    pub started_at: f64,    // 绝对时间戳（与 Time<Virtual> 同一个时钟）
    pub ends_at: f64,
    pub path: VecDeque<IVec2>,   // 剩余路径（相邻格序列）
}

// 状态机
Idle    : from == to && path.is_empty()
Moving  : from != to
衔接    : 到达后 path 非空 → 立刻 pop，继续 Moving（同帧，无停顿）
```

`from == to` 即空闲。

> **`Speed` 与 `Stats` 的关系还没定**（未记入 OPEN-QUESTIONS，先在这里留一笔）：
> 地形减速 / 减速状态如果各写一套，就会造出**第三条数值通道** ——
> 而本项目已经把伤害/减伤统一到 `Contest` + `Effect`，并对"状态 → 派生修饰符 → `Stats`"
> 立过规矩。建议 `Speed` 最终由 `Stats` 派生，别自成一体。

## 7. 驱动：一个系统，先结算到达，再 pop 下一步

```rust
fn step_motion(time: Res<Time<Virtual>>, mut q: Query<(&mut GridPosition, &Speed, &mut Motion)>) {
    let now = time.elapsed_secs_f64();
    for (mut pos, speed, mut motion) in q.iter_mut() {
        // 1. 先结算到达
        if motion.from != motion.to && now >= motion.ends_at {
            pos.0 = motion.to;
            motion.from = motion.to;
        }
        // 2. 再 pop 下一步（顺序反了会停顿一帧）
        if motion.from == motion.to {
            if let Some(next) = motion.path.pop_front() {
                motion.to = next;
                let duration = 1.0 / speed.0;
                motion.started_at = now;
                motion.ends_at = now + duration as f64;
            }
        }
    }
}
```

速度是每实体属性，各自算各自的 `ends_at`：玩家 `4.0`（0.25s/格）、哥布林 `6.0`（0.167s）、
僵尸 `1.5`（0.667s）。地形减速以后写进 `duration` 的分母。

## 8. 移动与动作的关系 `待确认（Q28）`

**倾向：前摇锁、后摇放**（FFXIV 的滑步施法）。

```text
前摇 [t0, t0+W)      移动 = 取消施法（"假动作 / feint"的空间）
释放点 t0+W          已结算，不可撤回
后摇 [t0+W, t0+W+R)  技能已生效 —— 后摇的语义是【冷却】，不是【定身】→ 应当可移动
```

支持它的硬理由：**威胁系统本身就要求前摇是一段独立、可被外人观测、还能被插进来的时间**
（"敌方进入前摇 → 开反制窗口"）。若移动与施法挤进同一个槽，"前摇"就没有独立身份，
反制窗口也就没有明确起止。

若采纳，`combat-design.md` §7 原则 2 的措辞要从"一次只执行一个动作"收窄为
"**一次只执行一个技能动作**；移动是独立通道，受动作阶段门控"。

L2 需要读"这个角色自己的动作阶段"，而现在只有全局的 `CombatPhase`，
`ActorState` 只是个 `statuses: Vec<Entity>` 的空壳（**名字与内容不符**）。
阶段状态得有个真正的家 —— 与三段时间轴是同一件事，建议一起做。

## 9. 路径规则

| 时机 | 做什么 |
|---|---|
| **决策时** | 算一次路径，塞进 `path`（现在还没有 A*，方向键走一格就够） |
| **每步开始** | `pop_front()` 取下一格，检查可走性 |
| **每帧** | 只推进时间、更新 `Transform`，**不碰路径** |

**输入只负责塞 path**，不关心"现在在不在走"。

### 两个必须区分的"被阻挡" `待确认`

- **临时挡**（别人正在过这一格）→ **原地等**，保留路径
- **永久挡**（地形变了）→ **重算路径**

不分的话，两个实体互相让路会变成"清空路径 → 重算 → 又被挡"的抖动。

### 施法请求 = 清空 path，但当前这一格走完 `待确认（Q30）`

逻辑位置是 `IVec2`，**不能停在格子中间**。走完当前格给了玩家约 `1/speed` 秒
（0.25s 量级）的反应窗，不会觉得按键被吞。

## 10. 已知坑（都是具体的，不是抽象的）

1. **`update_transform` 只改 `translation`，别重建 `Transform`。**
   现在 `actor_render` 的 `attach_visuals` 给精灵插了
   `Quat::from_rotation_y(-camera_azimuth_rad)`（等距下要转 45° 才正对相机）。
   每帧 `Transform::from_translation(..)` 会把旋转清掉 → **精灵侧对相机，变成一张纸的边**。
2. **`position_at` 的 `y` 必须从地形查。** 上游设计写的是 `.extend(0.0)` ——
   那会让角色**陷进地板**（或浮空）。高度只能在**一处**加，别在 L0 和 L2 各加一次。
3. **新实体必须出生就带 `Motion`。**
   `Query<(&mut GridPosition, &Speed, &mut Motion)>` 对没有 `Motion` 的实体**静默不匹配**：
   不移动、不报错。要么 spawn 时插 `Motion { from: pos, to: pos, .. }`，要么给
   `GridPosition` / `Speed` 配 `require`。
4. **绝对时间戳 + 存档 = 读档后全部瞬移到位。** `ends_at` 是"虚拟绝对秒"，
   读档后 `elapsed` 从 0 重来，所有 `ends_at` 都在过去。
   **别拿绝对秒当存档格式** —— 存"剩余时长"。（现在不用解决，但别把格式定死。）
5. **打断一旦能命中后摇，就等于能取消冷却**（见 [OPEN-QUESTIONS.md](OPEN-QUESTIONS.md) Q29）。
   今天没问题（行动实体在释放点就销毁了），统一时间轴之后就有了。

## 11. 与现有代码的接口（要动哪里）

| 现在 | 要变成 |
|---|---|
| `prime/actor_render` 的 `DemoPatrol`（`Vec2` + 正弦巡逻） | 删除。它是明确标注的临时替代："玩法层还没有空间维度……接入真实移动之后删掉它即可" |
| `prime/actor_render` 的 `Movement::still(pos)` | 换成 L0 的 `GridPosition` / `Motion` |
| `behaviors/action` 的 `tick_actions`（`elapsed += delta`） | 统一成绝对时间戳（见 §4） |
| `actor_render` 的 `sync_sprite_transform` | 变成从 `Motion` 重算（§7 的 L2 那一半） |

## 12. 建议的落地顺序

1. **`Action` 改绝对时间戳** —— 它是"别造第二个时间轴"的前提，也是三段时间轴的前提。
2. **L0 组件 + `position_at` + `step_motion`** —— 用方向键走一格就能验证（不需要 A*）。
3. **替掉 `DemoPatrol`** —— 演示巡逻一删，空间维度就真的进来了。
4. **三段时间轴（前摇 / 释放点 / 后摇）+ 动作阶段组件** —— 与 Q28 一起定。
5. **A* 寻路 / 可走性 / 冲突** —— 放到最后：`push_back(pos.0 + dir)` 能顶很久，
   而多阶段动作是**招牌机制**、也是反制窗口成立的前提。
