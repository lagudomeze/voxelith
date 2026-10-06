//! **反制**：让出自己正在做的事 + 由组件裁决能不能打断对方 + 出手。
//!
//! 威胁窗口（[`crate::threat`]）只回答"有人在打我"。这一层回答"我要怎么办"：
//!
//! ```text
//! CounterStrike{caster, ability}
//!   ├─ 窗口空？           → 忽略（没有可反制的对象）
//!   ├─ **让出**：对施法者正在 `#Invoking` 的技能发 AbortInvocation（= 旧设计的 replace）
//!   ├─ **裁决**：反制技能带 Interrupts 且威胁技能没有 SuperArmor → 打断威胁的前摇
//!   └─ **出手**：CastRequest（走既有门控 / 扣费 / 进状态机）
//! ```
//!
//! ## `replace` 在 gearbox 里怎么写
//!
//! gearbox 0.8 **没有**"强制转移"的 API（`grep force|set_state|abort` 零命中），
//! 但它给了正统做法：**写它正在监听的消息**——`StartInvoke` 就是这么驱动状态机的。
//! 所以：
//!
//! 1. 构造器在 `#Invoking` 上挂一条 `AbortInvocation` → `#Ready` 的边；
//! 2. "让出"就是给技能根发一条 `AbortInvocation`；
//! 3. 离开 `#WindUp` 时 gearbox 会**自动取消**那条前摇延时（`cancel_delay_timers`），
//!    所以不需要手工清计时器。
//!
//! ## 为什么交互语义必须是组件
//!
//! "反制能不能打断"是**内容**的事，不是引擎的事：霸体怪物的重击不该被一次轻反制打断，
//! 而某些技能天生带打断。引擎只提供两个标记组件（[`Interrupts`] / [`SuperArmor`]）
//! 与一条**裁决规则**（`Interrupts && !SuperArmor`），谁带哪个由内容写。
//!
//! 这与旧设计一致，区别只在于：旧引擎把裁决写死在 `Effect::Interrupt` 里，新栈把它拆出来，
//! 于是"加一种交互语义"= 加一个组件 + 一条规则，而不是改效果枚举。

use bevy::prelude::*;
use bevy_diesel::invoker::InvokedBy;
use bevy_gearbox::{AcceptAll, Active, GearboxMessage, RegistrationAppExt};

use crate::casting::CastRequest;
use crate::threat::ThreatWindow;

/// **放弃当前这次释放**（`#Invoking` → `#Ready`）。
///
/// 它是 `replace` 的机制：反制要"放弃自己正在做的事"，就是发这条消息。
#[derive(Message, Reflect, Clone, Copy, Debug)]
pub struct AbortInvocation {
    /// 哪个技能（技能根实体 = 状态机实体）。
    pub target: Entity,
}

impl GearboxMessage for AbortInvocation {
    type Validator = AcceptAll;
    fn target(&self) -> Entity {
        self.target
    }
}

/// **标记**：这一招能打断别人（内容里的 `interrupts: true`）。
///
/// 带 `bool` 而不是纯标记：内容决定要不要生效，而 BSN 里"有条件地插组件"会让场景类型分叉
/// （同一个理由见 [`crate::threat::ThreatensPlayer`]）。所以永远插，读的时候看字段。
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Interrupts(pub bool);

impl Interrupts {
    /// 生效吗。
    pub const fn yes(self) -> bool {
        self.0
    }
}

/// **标记**：这一招不可被打断（内容里的 `super_armor: true`）。
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct SuperArmor(pub bool);

impl SuperArmor {
    /// 生效吗。
    pub const fn yes(self) -> bool {
        self.0
    }
}

/// **标记**：这个状态是"正在释放"（构造器挂在 `#Invoking` 上）。
///
/// 用它把"谁正在做动作"问出来——这是 `replace` 要放弃的东西。
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Invoking;

/// **反制意图**（Message）：玩家/`AI` 要用某个技能反制当前威胁。
#[derive(Message, Clone, Copy, Debug)]
pub struct CounterStrike {
    /// 谁反制。
    pub caster: Entity,
    /// 用哪个技能。
    pub ability: Entity,
}

/// 执行反制：让出 → 裁决 → 出手。
///
/// **只发消息**（`AbortInvocation` / `CastRequest`），不直接改状态：
/// 状态转移归 gearbox，门控与扣费归 [`crate::casting`]，这里只做编排与裁决。
pub fn resolve_counters(
    mut strikes: MessageReader<CounterStrike>,
    window: Res<ThreatWindow>,
    invoking: Query<(&InvokedBy, &Active), With<Invoking>>,
    invokers: Query<&InvokedBy>,
    interrupts: Query<&Interrupts>,
    super_armor: Query<&SuperArmor>,
    mut abort: MessageWriter<AbortInvocation>,
    mut requests: MessageWriter<CastRequest>,
) {
    for strike in strikes.read() {
        // 没有威胁就没什么可反制的（内容层还会用 `HasThreat` 类的需求再挡一次）。
        if !window.is_open() {
            continue;
        }

        // ① 让出：施法者正在释放的技能全部放弃。
        for (invoked, _active) in &invoking {
            let ability = invoked.0;
            let owner = invokers.root_ancestor(ability);
            if owner == strike.caster {
                abort.write(AbortInvocation { target: ability });
            }
        }

        // ② 裁决：能打断吗？`Interrupts` 且对方**没有** `SuperArmor`。
        let can_interrupt = interrupts
            .get(strike.ability)
            .is_ok_and(|marker| marker.yes());
        if can_interrupt {
            for threat in window.pending() {
                // 威胁的凭据是**前摇态**；它的技能根是 `InvokedBy(wind_up).0`
                // （构造器在 `#WindUp` 上写了 `InvokedBy(#Ability)`）。
                let Some(threatening_ability) =
                    invokers.get(threat.action).ok().map(|invoked| invoked.0)
                else {
                    continue;
                };
                if super_armor
                    .get(threatening_ability)
                    .is_ok_and(|marker| marker.yes())
                {
                    continue; // 霸体：打断不了，威胁继续
                }
                abort.write(AbortInvocation {
                    target: threatening_ability,
                });
            }
        }

        // ③ 出手：走既有的释放门控（需求 / 费用 / 进状态机）。
        requests.write(CastRequest {
            caster: strike.caster,
            ability: strike.ability,
        });
    }
}

/// 注册反制：消息 + 转移 + 裁决系统。
pub struct CounterPlugin;

impl Plugin for CounterPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<CounterStrike>()
            .add_message::<AbortInvocation>()
            // `AbortInvocation` 是 gearbox 的转移消息，必须注册（**R34**：谁定义谁注册）。
            .register_transition::<AbortInvocation>()
            .add_systems(Update, resolve_counters);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_abort_addresses_the_ability_root() {
        // gearbox 的转移消息按**状态机实体**寻址（与 `StartInvoke` 一致）。
        let root = Entity::from_raw_u32(9).unwrap();
        let abort = AbortInvocation { target: root };
        assert_eq!(abort.target(), root);
    }
}
