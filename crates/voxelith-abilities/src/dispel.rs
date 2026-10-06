//! **净化**：把目标身上的状态提前移除（旧 `Effect::RemoveStatus` / `DispelAction`）。
//!
//! 机制**完全复用叠层那一套**：给被净化的状态实例发
//! [`SupersedeStatus`](crate::stacking::SupersedeStatus)，让它走 `#Removed` 那条边。
//! 于是"被净化"与"被顶掉"是同一类离场，`on_remove` 照常触发（Round 20 的进出效果），
//! 修饰符也照常被精确卸下（Round 19 的两步收场）。
//!
//! ```text
//! 命中（`#Fire` 上的 GoOff）→ 该状态上有 Dispels → 找出**目标的**`max` 个状态
//!   └─ SupersedeStatus → #Removed → on_remove → （下一帧）销毁
//! ```
//!
//! ## 挑哪几个
//!
//! - `debuffs_only: true` ⇒ 只看"以减益方式挂上去的"（`who: Target`）；
//! - `max` ⇒ 一次最多几个；
//! - 取**最旧的**（按实体序），这样"净化最老的中毒"是可预期的。
//!
//! ## 为什么判定放在 `GoOff` 上而不是 `#Fire` 的进入上
//!
//! 与命中规则、状态生成走同一条路（[`crate::contest`] 也是读 `GoOff`）：
//! 目标、威力、是否真的打中，都由 diesel 的传播管线算过一次了，这里只消费结果。

use bevy::prelude::*;
use bevy_diesel::effect::GoOff;
use bevy_diesel::invoker::InvokedBy;
use bevy_diesel::target::InvokerTarget;

use crate::stacking::{StatusHostRule, SupersedeStatus, resolve_status_host};

/// **净化**（组件，挂在技能的命中态上，由内容构造）。
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dispels {
    /// 一次最多净化几个。
    pub max: u32,
    /// 只净化减益（以 `who: Target` 挂上去的状态）。
    pub debuffs_only: bool,
}

/// 执行净化：目标身上的旧状态走"被移除"那条边。
pub fn resolve_dispels(
    mut go_offs: MessageReader<GoOff<IVec2>>,
    dispensers: Query<&Dispels>,
    statuses: Query<(Entity, &StatusHostRule, &InvokedBy)>,
    invokers: Query<&InvokedBy>,
    invoker_targets: Query<&InvokerTarget<IVec2>>,
    mut supersede: MessageWriter<SupersedeStatus>,
) {
    for go_off in go_offs.read() {
        let Ok(dispels) = dispensers.get(go_off.entity) else {
            continue;
        };
        let Some(target) = go_off.target.entity else {
            continue;
        };

        // 找"挂在这个目标身上"的状态实例（宿主解析与叠层共用一处实现）。
        let mut candidates: Vec<Entity> = statuses
            .iter()
            .filter(|(_, host_rule, invoked)| {
                if dispels.debuffs_only && !matches!(host_rule, StatusHostRule::Target) {
                    return false;
                }
                resolve_status_host(host_rule, invoked, &invokers, &invoker_targets) == Some(target)
            })
            .map(|(entity, _, _)| entity)
            .collect();

        // 最旧的先走（实体序即生成序）。
        candidates.sort_by_key(|entity| entity.index());
        for entity in candidates.into_iter().take(dispels.max as usize) {
            supersede.write(SupersedeStatus { target: entity });
        }
    }
}

/// 注册净化。
pub struct DispelPlugin;

impl Plugin for DispelPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, resolve_dispels);
    }
}
