//! **周期效果**：每隔一段时间触发一次（状态用它做持续伤害 / 回复）。
//!
//! ## 为什么不用 diesel 的 `Repeater`
//!
//! `diesel::scenes::repeater(root, count_expr: &'static str, interval_expr: &'static str, on_fire)`
//! 的**次数与间隔都是 `&'static str`**——装不下内容里的动态秒数（第三次撞到同一个限制，
//! 前两次是 `invoked` 的技能名与 `invoked` 的冷却）。它的定位也是"计数齐射"（volley），
//! 而状态的 DoT 由**状态自己的时长**兜底，不需要"计数"这层语义。
//!
//! ## 与 diesel 管线的接法
//!
//! 本部件只回答"**什么时候**该响"，"打谁、打多少"仍旧交给 diesel：
//!
//! ```text
//! PeriodicEffect（挂在状态的生效态上）
//!   └─ tick_periodic_effects  累加虚拟时间 → 到点发 PeriodicTick
//!        └─ fire_periodic_effects::<B>  读该状态的 GoOffConfig → 解析目标
//!             └─ GoOffOrigin ──► diesel 的 propagate_system → GoOff
//!                  └─ instant_set_system（叶子效果）→ 改属性
//! ```
//!
//! 于是"周期伤害"= `PeriodicEffect` + `GoOffConfig` + `InstantModifierSet` 三个组件，
//! 与"一次性效果"共用同一条传播路径（叶子系统一行都没改）。
//!
//! ## ⚠️ 上状态即第一跳（刻意的语义）
//!
//! 那三个组件挂在状态的**生效态**上，而 diesel 的 `go_off_on_entry` 会在**进入生效态时**
//! 让 `GoOffConfig` 响一次。所以一个 `tick` 状态的伤害是：
//!
//! ```text
//! t = 0        应用时立刻一下（`go_off_on_entry`）
//! t = every    再一下（本模块的周期）
//! t = 2×every  再一下 …
//! ```
//!
//! 也就是"上毒即第一跳"，这是常见的 DoT 语义。**内容侧据此配时长**：
//! `duration: 4.0` + `every: 1.0` 一共跳 **5** 次（0/1/2/3/4 秒），要"4 次"就写 `duration: 3.0`。
//!
//! `fire_periodic_effects` 是**泛型 over `B`** 的，所以按本项目既有规则
//! **由后端注册**（带 `B::Context` 的系统只能在定义 `B` 的地方单态化）。

use std::marker::PhantomData;

use bevy::prelude::*;
use bevy_diesel::backend::SpatialBackend;
use bevy_diesel::effect::GoOffConfig;
use bevy_diesel::invoker::{InvokedBy, resolve_invoker, resolve_root};
use bevy_diesel::pipeline::generate_targets;
use bevy_diesel::target::{InvokerTarget, Target};

/// 周期效果：每 `every` 秒响一次。
///
/// 挂在**状态的生效态**上（与 `AttributeModifiers` 同一位置）。
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct PeriodicEffect {
    /// 间隔（秒），必须为正。
    pub every: f32,
    /// 已经攒了多久（内部状态）。
    pub accumulator: f32,
}

impl PeriodicEffect {
    /// 每 `every` 秒一次。
    pub const fn every(every: f32) -> Self {
        Self {
            every,
            accumulator: 0.0,
        }
    }
}

/// **消息**：某个周期状态该响一次了。
#[derive(Message, Clone, Copy, Debug)]
pub struct PeriodicTick {
    /// 哪个状态（生效态实体）。
    pub state: Entity,
}

/// 累加虚拟时间，到点发 [`PeriodicTick`]。
///
/// **只在生效态上累加**（`With<Active>`）：状态离开生效态后计时自然停止，
/// 不需要"暂停/恢复"这类状态。
pub fn tick_periodic_effects(
    time: Res<Time>,
    mut effects: Query<(Entity, &mut PeriodicEffect), With<bevy_gearbox::Active>>,
    mut ticks: MessageWriter<PeriodicTick>,
) {
    let delta = time.delta_secs();
    for (state, mut effect) in &mut effects {
        if effect.every <= 0.0 || !effect.every.is_finite() {
            continue;
        }
        effect.accumulator += delta;
        // `while` 而不是 `if`：一帧跨越多个间隔时该补几次就补几次
        // （上限防止极端帧长下雪崩）。
        let mut fired = 0;
        while effect.accumulator >= effect.every && fired < 16 {
            effect.accumulator -= effect.every;
            fired += 1;
        }
        for _ in 0..fired {
            ticks.write(PeriodicTick { state });
        }
    }
}

/// 把周期 tick 变成 diesel 的"效果该响了"——与 diesel 自己的 `go_off_on_entry` 同一条路。
///
/// 为什么要在后端注册：参数里有 `B::Context`。
pub fn fire_periodic_effects<B: SpatialBackend>(
    mut ticks: MessageReader<PeriodicTick>,
    configs: Query<&GoOffConfig<B>>,
    invokers: Query<&InvokedBy>,
    substates: Query<&bevy_gearbox::SubstateOf>,
    invoker_targets: Query<&InvokerTarget<B::Pos>>,
    mut ctx: B::Context<'_, '_>,
    mut origins: MessageWriter<bevy_diesel::effect::GoOffOrigin<B::Pos>>,
) {
    for tick in ticks.read() {
        let state = tick.state;
        let Ok(config) = configs.get(state) else {
            continue;
        };
        let invoker = resolve_invoker(&invokers, state);
        let root = resolve_root(&substates, state);
        let invoker_target: Target<B::Pos> = invoker_targets
            .get(invoker)
            .map(|aim| Target::from(*aim))
            .unwrap_or_default();

        let mut targets = generate_targets::<B>(
            &config.generator,
            &mut ctx,
            invoker,
            invoker_target,
            root,
            B::Pos::default(),
            invoker_target,
        );
        targets = B::apply_filter(
            &mut ctx,
            targets,
            &config.generator.filter,
            invoker,
            invoker_target.position,
        );
        for (target, scope) in targets {
            origins.write(bevy_diesel::effect::GoOffOrigin::with_scope(
                state, target, scope,
            ));
        }
    }
}

/// 注册周期效果的时间部分（与 `B` 无关）。
pub struct PeriodicPlugin;

impl Plugin for PeriodicPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<PeriodicTick>()
            .add_systems(Update, tick_periodic_effects);
    }
}

/// 让 `PhantomData` 的用法显式（泛型系统不需要它，但保留一个空类型便于将来扩展）。
#[derive(Debug, Default)]
pub struct PeriodicMarker(PhantomData<()>);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_non_positive_interval_never_fires() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, PeriodicPlugin));
        let state = app.world_mut().spawn(PeriodicEffect::every(0.0)).id();
        for _ in 0..3 {
            app.update();
        }
        let mut ticks = app.world_mut().resource_mut::<Messages<PeriodicTick>>();
        assert_eq!(ticks.drain().count(), 0, "间隔非正数不该触发");
        let _ = state;
    }
}
