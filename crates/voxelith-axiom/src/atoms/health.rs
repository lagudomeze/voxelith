//! L0：血量原子数据与**纯数值执行**。
//!
//! 本模块是唯一允许修改 [`Health`] 的地方（**R56**），并且**不认识**
//! `DamageType` / `Armor` / 闪避 / 暴击（**R47**、**R98**）——
//! 那些概念属于 L1 `behaviors::combat`。
//!
//! 伤害与治疗共用 [`ModifyHealthMessage`]：负数=伤害，正数=治疗（**R49**、**R51**）。
//! 事件定义在本模块并由 [`HealthPlugin`] 注册（**R33**、**R34**）。

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_reflect::prelude::*;

/// 血量组件。
///
/// > 待确认：字段当前为 `pub`，L2 可以直接写 `current`，与 **R56**（唯一写入口）
/// > 存在张力。收敛方案见 `docs/OPEN-QUESTIONS.md` 的 Q2。
#[derive(Component, Reflect, Debug, Clone, Copy, PartialEq, Eq)]
#[reflect(Component)]
pub struct Health {
    pub current: i32,
    pub max: i32,
}

impl Default for Health {
    fn default() -> Self {
        Self::new(100)
    }
}

impl Health {
    /// 满血单位。
    pub fn new(max: i32) -> Self {
        Self { current: max, max }
    }

    /// 是否还活着。
    pub fn is_alive(&self) -> bool {
        self.current > 0
    }
}

/// 纯数值血量变更：负数=伤害，正数=治疗（**R49**、**R51**）。
///
/// 命名以 `Message` 结尾：它是**广播型核心数据变更**，必须走 Bevy 的 `Message` 机制
/// （拉取式、可被多个系统并行观察、可被公式拦截），不能用 `EntityEvent`。
/// 判定依据见 `docs/bevy-events.md`。
///
/// 不含伤害类型、来源、抗性等任何上下文——那些属于 L1 的 `DamageRequest`（**R47**）。
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModifyHealthMessage {
    /// 被修改血量的实体。
    pub entity: Entity,
    /// 变更量：负数=伤害，正数=治疗。
    pub amount: i32,
}

/// 血量归零的实体事件。
///
/// 命名以 `Event` 结尾：它是**绑定到具体实体的即时通知**，用 Bevy 的 `EntityEvent` +
/// observer（表现层即时反应，不做核心结算）。判定依据见 `docs/bevy-events.md`。
///
/// 事件定义在发出它的模块（**R33**）；由 [`apply_health_change`] 在血量降到 0 时触发。
/// 表现层（L2）用 observer 监听它播死亡表现（**R50** 流水线末端）。
#[derive(EntityEvent, Debug, Clone, Copy, PartialEq)]
pub struct DeathEvent {
    /// 阵亡的实体。
    #[event_target]
    pub entity: Entity,
}

/// 消费 [`ModifyHealthMessage`]，是**唯一**写入 [`Health`] 的系统（**R56**）。
///
/// 纯数值执行：夹取到 `0..=max`，不做任何公式（**R47**、**R98**）。
/// 血量归零时触发一次 [`DeathEvent`]。
pub fn apply_health_change(
    mut messages: MessageReader<ModifyHealthMessage>,
    mut healths: Query<&mut Health>,
    mut commands: Commands,
) {
    for message in messages.read() {
        let Ok(mut health) = healths.get_mut(message.entity) else {
            continue;
        };
        let was_alive = health.is_alive();
        health.current = (health.current + message.amount).clamp(0, health.max);
        if was_alive && !health.is_alive() {
            commands.trigger(DeathEvent {
                entity: message.entity,
            });
        }
    }
}

/// 注册血量模块的系统与事件（**R34**）。
pub struct HealthPlugin;

impl Plugin for HealthPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<ModifyHealthMessage>()
            .add_systems(Update, apply_health_change);
    }
}
