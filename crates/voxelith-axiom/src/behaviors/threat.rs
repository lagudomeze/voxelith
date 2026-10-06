//! **威胁窗口**：未处理的、针对 PC 的威胁集合（L1）。
//!
//! ```text
//! 行动生成（威胁到 PC）→ 发 ThreatensPlayer 消息
//!    → 本模块的系统读消息 → ThreatWindow.pending.push（窗口的唯一写入口）
//!    → update_phase：窗口非空 → 写 NextState
//!    → 次帧 PreUpdate 落地 States → First 把倍率设为 0（冻结）
//! ```
//!
//! ## 三条不可动摇的规则
//!
//! 1. **窗口是状态，不是消息。** Bevy 的 `Message` 是双缓冲、**只活两帧**
//!    （`message/messages.rs`：每次 `update` 交换缓冲并清掉最老的那个），
//!    所以"用消息还在不在"表示窗口会让暂停**自己解开**。消息只负责通知。
//! 2. **窗口只有一个写入口**（本模块的 [`collect_threats`]）。
//! 3. **"未处理"必须配存活对账。** 只靠信号（`RemovedComponents<Threat>`）的话，
//!    任何一次漏发都会让窗口**永久卡住**（暂停再也解不开）——这是本项目最怕的沉默失败。
//!    所以 [`reap_threats`] 每帧再做一次 `retain(行动还存在)`。
//!
//! ## 为什么是集合而不是"一格"
//!
//! 窗口是"多条威胁的聚合"，不是"谁先抢到的那一格"：多条威胁并存是合法的，
//! 玩家在 UI 里**自己选**要反制哪一条。于是窗口不参与任何竞争，
//! 也就不需要"先到先得"那套调度契约。

use bevy_app::prelude::*;
use bevy_ecs::lifecycle::RemovedComponents;
use bevy_ecs::prelude::*;

use crate::atoms::action::{Action, Threat as ActionThreat};

/// 一条威胁：哪条行动、谁发起的、威胁到谁。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Threat {
    /// 即将生效的行动。
    pub action: Entity,
    /// 发起者（`Targeting::ThreatSource` 指向它）。
    pub source: Entity,
    /// 它威胁的目标（PC）。
    pub target: Entity,
}

/// 挂起的威胁集合（**Resource**）：全局唯一，"威胁哪个窗口"永远是"那唯一一个"。
///
/// 条目自带 `target`，所以将来多个 PC / 多场战斗时，"分键"发生在条目上，
/// 而不是窗口上（暂停是全局的，窗口也应当是全局的）。
#[derive(Resource, Debug, Clone, Default, PartialEq, Eq)]
pub struct ThreatWindow {
    pending: Vec<Threat>,
}

impl ThreatWindow {
    /// 当前未处理的威胁。
    pub fn pending(&self) -> &[Threat] {
        &self.pending
    }

    /// 第一条威胁（`Targeting::ThreatSource` 与 `Requirement::HasThreat` 用它）。
    pub fn first(&self) -> Option<&Threat> {
        self.pending.first()
    }

    /// 威胁来源（第一条威胁的发起者）。
    pub fn source(&self) -> Option<Entity> {
        self.pending.first().map(|threat| threat.source)
    }

    /// 有没有未处理的威胁（**相位判据**：这就是 `has_threat`）。
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

    /// 登记一条威胁。
    ///
    /// **生产代码只应通过 [`ThreatensPlayer`] 消息写入**（[`collect_threats`] 是唯一写入口）；
    /// 这个方法是公开的，好让集成测试能直接摆出一个窗口。
    pub fn push(&mut self, threat: Threat) {
        if !self.pending.contains(&threat) {
            self.pending.push(threat);
        }
    }

    /// 清空（玩家做出决策后）。
    ///
    /// **语义**：清空窗口 = 解冻 + 关闭本次决策机会；**不等于取消威胁**。
    /// 未被反制的行动照旧推进并结算。
    pub fn clear(&mut self) {
        self.pending.clear();
    }

    /// 摘掉某条（它已经处理完了）。
    fn remove(&mut self, action: Entity) {
        self.pending.retain(|threat| threat.action != action);
    }
}

/// **通知型消息**：某条行动威胁到了 PC。
///
/// 生产者是"提交行动"那一步（[`commit_actions`](crate::behaviors::action)）；
/// 消费者是本模块；L2 也可以旁路监听它来播提示音 / 高亮 UI。
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThreatensPlayer {
    /// 威胁行动。
    pub action: Entity,
    /// 发起者（怪物）。
    pub source: Entity,
    /// 被威胁的目标。
    pub target: Entity,
}

/// 注册威胁窗口：资源 + 消息。
///
/// **系统不在这里注册**：`collect_threats` / `reap_threats` 必须夹在提交与相位之间，
/// 跨域顺序由装配层 [`CombatPlugin`](crate::behaviors::combat::CombatPlugin) 独占（**Q14**）。
pub struct ThreatPlugin;

impl Plugin for ThreatPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ThreatWindow>()
            .add_message::<ThreatensPlayer>();
    }
}

/// 窗口的**唯一写入口**：读消息 → 登记。
///
/// 只登记"针对 PC 的威胁"由生产者负责（它知道谁是 PC）。
pub fn collect_threats(
    mut messages: MessageReader<ThreatensPlayer>,
    mut window: ResMut<ThreatWindow>,
) {
    for message in messages.read() {
        window.push(Threat {
            action: message.action,
            source: message.source,
            target: message.target,
        });
    }
}

/// 对账：已处理的威胁要离场。
///
/// 两个信号源一起用：
/// - `RemovedComponents<Threat>`：覆盖"`Resolve` 时摘掉标记"与"实体被 despawn"两种情况；
/// - 存活对账：`retain(行动实体还存在)`——漏一次信号也不会把暂停永久卡住。
pub fn reap_threats(
    mut removed: RemovedComponents<ActionThreat>,
    actions: Query<(), (With<Action>, With<ActionThreat>)>,
    mut window: ResMut<ThreatWindow>,
) {
    for action in removed.read() {
        window.remove(action);
    }
    if window.is_open() {
        window
            .pending
            .retain(|threat| actions.contains(threat.action));
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
    fn a_registered_threat_opens_the_window() {
        let mut window = ThreatWindow::default();
        window.push(Threat {
            action: Entity::PLACEHOLDER,
            source: Entity::PLACEHOLDER,
            target: Entity::PLACEHOLDER,
        });
        assert!(window.is_open());
        assert_eq!(window.len(), 1);
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
    fn clearing_the_window_closes_it() {
        let mut window = ThreatWindow::default();
        window.push(Threat {
            action: Entity::PLACEHOLDER,
            source: Entity::PLACEHOLDER,
            target: Entity::PLACEHOLDER,
        });
        window.clear();
        assert!(!window.is_open(), "玩家做出决策后窗口关闭（威胁本身照旧）");
    }
}
