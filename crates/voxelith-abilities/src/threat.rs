//! **威胁窗口**：未处理的、针对玩家的威胁集合 —— 反制的依据。
//!
//! 这是三件套**没有**提供、必须自己搭的那一块：
//!
//! | 需要的能力 | 谁提供 |
//! |---|---|
//! | 一次行动的前摇是一段可观测、可插入的时间 | gearbox 的 `#WindUp` 状态 + `Delay` |
//! | "现在有人在打我"这份**状态** | **本模块**（gearbox 无守卫、diesel 无威胁概念） |
//! | 前摇期间时间停住 | `Time<Virtual>` 倍率 0（gearbox 的延时读 `Res<Time>`，所以真停） |
//! | 反制 = 放弃自己当前行动再出手 | 行动槽改造（下一批） |
//! | 打断/霸体由组件裁决 | `Interrupts` / `SuperArmor`（下一批） |
//!
//! ## 三条不可动摇的规则（沿用旧设计，因为它们是踩出来的）
//!
//! 1. **窗口是状态，不是消息**：Bevy 的 `Message` 只活两帧，用"消息还在不在"表示窗口
//!    会让暂停**自己解开**。消息只负责通知。
//! 2. **窗口只有一个写入口**（[`rebuild_threat_window`]）。
//! 3. **每帧整批重建，不做增量加减**。这一条改过，理由值得记住：
//!    最初用 `Added<Active>` 登记 + `RemovedComponents` + 存活对账清点，结果**只在
//!    系统顺序恰好合适时才对**——`Added` 信号与 gearbox 的状态转移之间没有排序保证，
//!    谁在 `Update` 里多插一个系统（例如后来加的硬控派生），窗口就会晚一帧才开。
//!    整批重建让"漏信号"这条失败路径**根本不存在**（"永久冻结"也就无从发生），
//!    代价是每帧扫一遍带标记的生效态（个位数）。
//!
//!    教训：**能整批重建的派生数据，就不要用信号增量去维护。**
//!
//! ## 冻结的生效延迟**恰好一帧**
//!
//! `Time<Virtual>` 的倍率在 `Update` 里改，而虚拟时间在下一帧 `First` 才被 `TimePlugin`
//! 读进去。所以"窗口一开就立刻停"是做不到的；能保证的是**一帧**。
//! 写测试时要按这个口径断言（"任意时刻 delta != 0 ⟺ 窗口是空的"）。
//!
//! ## 与旧设计的差异
//!
//! 旧引擎里威胁来自"决策槽 + 挂起威胁"那套；新栈里威胁就是**"某个前摇状态正在 `Active`"**，
//! 由内容标记（`threatening: true`）决定谁算威胁。于是不需要"决策实体""登记表"这些东西。

use bevy::prelude::*;
use bevy_diesel::invoker::{InvokedBy, resolve_invoker};
use bevy_diesel::target::InvokerTarget;
use bevy_gearbox::Active;

/// **标记**：这个状态（通常是前摇 `#WindUp`）正在威胁某个目标。
///
/// 带一个 `bool` 而不做成纯标记，是因为"要不要挂它"由**内容**决定，
/// 而 BSN 里"有条件地插入组件"会让场景类型分叉（`impl Scene` 只能是一个具体类型）。
/// 所以永远挂上，读的时候看字段。
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ThreatensPlayer(pub bool);

impl ThreatensPlayer {
    /// 是威胁吗。
    pub const fn yes(self) -> bool {
        self.0
    }
}

/// 一条威胁：哪条行动的前摇正在进行、谁发起的、威胁到谁。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Threat {
    /// 正在前摇的那个**状态实体**（威胁的凭据）。
    pub action: Entity,
    /// 发起者（怪物）。
    pub source: Entity,
    /// 被威胁的目标（玩家）。
    pub target: Entity,
}

/// 挂起的威胁集合（**Resource**）：全局唯一，"威胁哪个窗口"永远是"那唯一一个"。
///
/// 条目自带 `target`，所以将来多个 PC / 多场战斗时，"分键"发生在条目上而不是窗口上。
#[derive(Resource, Debug, Clone, Default)]
pub struct ThreatWindow {
    pending: Vec<Threat>,
}

impl ThreatWindow {
    /// 当前未处理的威胁。
    pub fn pending(&self) -> &[Threat] {
        &self.pending
    }

    /// 第一条（UI 与"谁在打我"用）。
    pub fn first(&self) -> Option<&Threat> {
        self.pending.first()
    }

    /// 有没有未处理的威胁（**相位判据**）。
    pub fn is_open(&self) -> bool {
        !self.pending.is_empty()
    }

    /// 有几条。
    pub fn len(&self) -> usize {
        self.pending.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// 清空（玩家做出决策后：解冻 + 关闭本次决策机会；**不取消**未被反制的威胁）。
    pub fn clear(&mut self) {
        self.pending.clear();
    }

    /// 登记一条（已存在则忽略）。
    ///
    /// **只给单测用**：生产路径现在整批重建（[`rebuild_threat_window`] 直接换掉整个 `pending`），
    /// 不再有增量登记这一步。
    #[cfg(test)]
    fn push(&mut self, threat: Threat) {
        if !self.pending.contains(&threat) {
            self.pending.push(threat);
        }
    }

    /// 摘掉某条行动的全部威胁。**只给单测用**（同上）。
    #[cfg(test)]
    fn remove_action(&mut self, action: Entity) {
        self.pending.retain(|threat| threat.action != action);
    }
}

/// 窗口开了 / 空了（**Message**）：给 UI 提示音、高亮、"反制可用"按钮用。
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThreatWindowChanged {
    /// `true` = 窗口刚开；`false` = 刚空。
    pub open: bool,
}

/// 重建威胁窗口：**从现在活着的"威胁前摇"重新填一遍**。
///
/// 每帧整批重建（而不是增量登记 + 对账清理），理由见模块文档第 3 条：
/// 增量维护要求"信号一条不丢 + 顺序恰好"，而那条依赖**没法保证**；
/// 重建则天然正确——状态不再生效，威胁自然就不在窗口里了。
///
/// 目标取自**发起者当前瞄的人**：瞄不到（没有 `InvokerTarget`）就不算威胁，
/// "打空气"不该冻结全场。
pub fn rebuild_threat_window(
    threatening: Query<(Entity, &ThreatensPlayer), With<Active>>,
    invokers: Query<&InvokedBy>,
    invoker_targets: Query<&InvokerTarget<IVec2>>,
    mut window: ResMut<ThreatWindow>,
) {
    let mut fresh: Vec<Threat> = Vec::new();
    for (action, threatens) in &threatening {
        if !threatens.yes() {
            continue;
        }
        let source = resolve_invoker(&invokers, action);
        let Some(target) = invoker_targets.get(source).ok().and_then(|aim| aim.entity) else {
            continue;
        };
        let threat = Threat {
            action,
            source,
            target,
        };
        if !fresh.contains(&threat) {
            fresh.push(threat);
        }
    }

    // 顺序稳定（按实体序）：UI 里的"第一条威胁"不该每帧乱跳。
    fresh.sort_by_key(|threat| threat.action);
    window.pending = fresh;
}

/// 冻结：窗口非空 → 虚拟时间倍率 0；空 → 1。
///
/// **只在变化时写**（`set_relative_speed` 会标脏 `Time<Virtual>`，每帧写会让所有
/// 依赖时间变更检测的系统白跑）。生效延迟一帧，见模块文档。
pub fn freeze_while_threatened(
    mut window_was_open: Local<Option<bool>>,
    window: Res<ThreatWindow>,
    mut time: ResMut<Time<Virtual>>,
) {
    let open = window.is_open();
    if *window_was_open == Some(open) {
        return;
    }
    *window_was_open = Some(open);
    time.set_relative_speed(if open { 0.0 } else { 1.0 });
}

/// 通知：窗口开/关翻转时发一条消息。
pub fn announce_window_changes(
    mut was_open: Local<bool>,
    window: Res<ThreatWindow>,
    mut changed: MessageWriter<ThreatWindowChanged>,
) {
    let open = window.is_open();
    if open != *was_open {
        *was_open = open;
        changed.write(ThreatWindowChanged { open });
    }
}

/// 注册威胁窗口：资源 + 消息 + 三个系统（顺序是契约：重建 → 冻结 → 通知）。
pub struct ThreatPlugin;

impl Plugin for ThreatPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ThreatWindow>()
            .add_message::<ThreatWindowChanged>()
            // ⚠️ 放在 `PostUpdate`，**不是风格问题**：威胁是"这一帧结束时谁还在前摇"。
            // gearbox 的状态转移跑在 `Update` 里，而 `Update` 内部的先后**没有保证**——
            // 放在 `Update` 曾让窗口比状态晚一帧才开（诊断输出：`#WindUp` 在 frame 1 拿到
            // `Active`，窗口直到 frame 2 才非空）。放在帧末就与 `Update` 内部顺序无关了。
            .add_systems(
                PostUpdate,
                (
                    rebuild_threat_window,
                    freeze_while_threatened,
                    announce_window_changes,
                )
                    .chain(),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_window_means_no_threat() {
        let window = ThreatWindow::default();
        assert!(!window.is_open());
        assert!(window.is_empty());
    }

    #[test]
    fn the_same_threat_is_not_registered_twice() {
        let mut window = ThreatWindow::default();
        let threat = Threat {
            action: Entity::PLACEHOLDER,
            source: Entity::PLACEHOLDER,
            target: Entity::PLACEHOLDER,
        };
        window.push(threat);
        window.push(threat);
        assert_eq!(window.len(), 1, "重复登记不该让窗口膨胀");
    }

    #[test]
    fn clearing_closes_the_window_without_cancelling_anything() {
        let mut window = ThreatWindow::default();
        window.push(Threat {
            action: Entity::PLACEHOLDER,
            source: Entity::PLACEHOLDER,
            target: Entity::PLACEHOLDER,
        });
        window.clear();
        assert!(
            !window.is_open(),
            "玩家做出决策后窗口关闭（威胁本身照旧推进）"
        );
    }

    #[test]
    fn removing_one_action_takes_all_its_threats() {
        let action = Entity::PLACEHOLDER;
        let other = Entity::from_raw_u32(7).unwrap();
        let mut window = ThreatWindow::default();
        window.push(Threat {
            action,
            source: action,
            target: action,
        });
        window.push(Threat {
            action: other,
            source: other,
            target: other,
        });
        window.remove_action(action);
        assert_eq!(window.len(), 1);
        assert_eq!(window.first().unwrap().action, other);
    }
}
