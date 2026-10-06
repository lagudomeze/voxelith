//! **战斗面板**：新栈（diesel + gauge + gearbox）的 HUD 出口。
//!
//! 与 [`super::hud`]（读旧引擎）并存：旧内容管线还在跑，两边各读各的。
//!
//! ## 遵守 [`super`] 定下的原则：**不抄中间层**
//!
//! 面板直接读新栈已经算好的只读面：
//!
//! | 显示什么 | 直接读谁 |
//! |---|---|
//! | 资源（当前 / 上限） | gauge 的 `Attributes`（`Health` / `MaxHealth` …），资源清单来自 `RegenRules` |
//! | 身上的状态 | 状态实例的 `StatusIdentity` + `BlocksTags`（硬控封了哪几类） |
//! | 技能槽 | `Loadout` + 三个生命周期标记（`Ready` / `Invoking` / `Cooling`）+ `Active` |
//! | 有人在打我 | `ThreatWindow`（窗口非空 ⇒ 时间被冻住） |
//! | 为什么放不出来 | `CastRejected` 的理由（本模块只负责"存下最后一条给 UI"） |
//!
//! 抽出来给 UI 用的只有几个**纯函数**（[`pool_rows`] / [`slot_state`] / [`threat_line`]）：
//! 它们不碰世界，所以能单测；系统只负责把结果画到 `Text` 上。
//!
//! ## 为什么资源清单取自 `RegenRules`
//!
//! 新栈里"哪些属性是资源"没有单独登记（gauge 的属性只有名字与数值）。而
//! `attributes.ron` 的 `regen` 规则**已经把九条资源列全了**（旧 `vocabulary.ron` 的
//! 资源表就是这批名字），所以直接拿它当清单——少一处要同步的登记。
//! 将来若出现"不回复的资源"，再给属性表加一个显式的池清单字段。
//!
//! ## 还没做
//!
//! · 冷却/状态的**剩余时间**（gearbox 的 `Delay` 计时器挂在转移边上，要读出它还得多一层
//!   查询；现在只显示"在冷却 / 在释放"）；· 真排版（现在是一整块文本，先能看见）。

use bevy::prelude::*;
use voxelith_abilities::ability::{Cooling, Ready};
use voxelith_abilities::casting::{BlockedTags, CastRejected};
use voxelith_abilities::counter::Invoking;
use voxelith_abilities::numeric::RegenRules;
use voxelith_abilities::stacking::{StatusHostRule, StatusIdentity, resolve_status_host};
use voxelith_abilities::threat::ThreatWindow;
use voxelith_abilities::{
    Active, Attributes, EdgeTimer, InvokedBy, InvokerTarget, SubstateOf, Transitions, resolve_root,
};

use crate::combat::{Loadout, Playable};

// ============================================================================
// 纯函数（不碰世界，可单测）
// ============================================================================

/// HUD 上的一条资源。
#[derive(Debug, Clone, PartialEq)]
pub struct PoolRow {
    /// 属性名（`"Health"`）。
    pub name: String,
    /// 当前值。
    pub current: f32,
    /// 上限。
    pub max: f32,
}

impl PoolRow {
    /// 0..1 的比例（上限为 0 时按满处理，免得除零画出怪条）。
    pub fn ratio(&self) -> f32 {
        if self.max <= 0.0 {
            1.0
        } else {
            (self.current / self.max).clamp(0.0, 1.0)
        }
    }
}

/// 从一份属性集里取出资源行。**资源清单来自回复规则**（理由见模块文档）。
pub fn pool_rows(attributes: &Attributes, rules: &RegenRules) -> Vec<PoolRow> {
    rules
        .0
        .iter()
        .map(|rule| PoolRow {
            name: rule.attribute.clone(),
            current: attributes.value(&rule.attribute),
            max: attributes.value(&rule.max),
        })
        .collect()
}

/// 技能槽的状态机位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotState {
    /// 停在 `#Ready`。
    Ready,
    /// 正在释放（前摇或命中）。
    Invoking,
    /// 冷却中。
    Cooling,
    /// 没查到（技能实体没了 / 状态机还没初始化）。
    Dormant,
}

impl SlotState {
    /// 短标签（HUD 用）。
    pub fn label(self) -> &'static str {
        match self {
            SlotState::Ready => "就绪",
            SlotState::Invoking => "释放中",
            SlotState::Cooling => "冷却",
            SlotState::Dormant => "—",
        }
    }
}

/// 由三个标记判定槽位状态（**纯函数**：系统把查询结果传进来）。
///
/// 优先级：正在释放 > 冷却 > 就绪。理论上三者互斥，真同时出现时按"最忙的那个"显示。
pub fn slot_state(ready: bool, invoking: bool, cooling: bool) -> SlotState {
    if invoking {
        SlotState::Invoking
    } else if cooling {
        SlotState::Cooling
    } else if ready {
        SlotState::Ready
    } else {
        SlotState::Dormant
    }
}

/// "有人在打我"那一行（窗口空就什么都不显示）。
pub fn threat_line(window: &ThreatWindow) -> Option<String> {
    match window.len() {
        0 => None,
        1 => Some("⚠ 有人正在打我（时间已停，可反制）".to_string()),
        n => Some(format!("⚠ {n} 个威胁悬在头上（时间已停）")),
    }
}

/// 把"还剩几秒"写成给人看的短串（一位小数）。
///
/// **剩余时间读的是 gearbox 自己的计时器**（`EdgeTimer`，挂在延时转移边上），
/// 而不是本层另记一份——所以它和状态机**不会漂**。
pub fn format_remaining(seconds: f32) -> String {
    format!("{:.1}s", seconds.max(0.0))
}

// ============================================================================
// 系统
// ============================================================================

/// **最后一条释放被拒的理由**（给 UI 显示；读完不清，UI 自己决定显示多久）。
///
/// 理由本身是 L1 算出来的（`CastRejected`），这里只记下最后一条——不重算任何战斗规则。
#[derive(Resource, Debug, Clone, Default)]
pub struct CombatFeedback {
    /// 被拒的技能 + 理由。
    pub last_rejection: Option<(Entity, &'static str)>,
}

/// 记下最后一条 `CastRejected`（UI 的反馈来源）。
///
/// 与门控（[`voxelith_abilities::casting`]）同在 `Update`，两者之间没有排序保证，
/// 所以理由**最晚下一帧**才出现在面板上。UI 一个帧的延迟无感，换来的是不必把
/// 表现层钉进 L1 的系统顺序里。
///
/// **参数是可选的**：面板只是装饰。旧引擎那套 App 里没有新栈（没注册 `CastRejected`），
/// 这时它什么都不做，而不是让调度报 "Message not initialized"。
pub fn record_rejections(
    rejections: Option<MessageReader<CastRejected>>,
    mut feedback: ResMut<CombatFeedback>,
) {
    let Some(mut rejections) = rejections else {
        return;
    };
    for rejection in rejections.read() {
        feedback.last_rejection = Some((rejection.ability, rejection.reason));
    }
}

/// 面板根节点。
#[derive(Component)]
pub struct CombatPanelRoot;

/// 面板正文。
#[derive(Component)]
pub struct CombatPanelText;

/// 建面板骨架（一整块文本，先能看见）。
pub fn spawn_combat_panel(mut commands: Commands) {
    commands
        .spawn((
            CombatPanelRoot,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(12.0),
                top: Val::Px(12.0),
                flex_direction: FlexDirection::Column,
                ..default()
            },
        ))
        .with_children(|parent| {
            parent.spawn((CombatPanelText, Text::new("战斗面板")));
        });
}

/// 每帧把新栈的只读面画成文本。
///
/// **`RegenRules` / `ThreatWindow` 是可选的**：旧引擎那套 App 里没有新栈，
/// 面板这时只画它拿得到的东西（空面板），而不是让调度报 "Resource does not exist"。
/// 这是"两套并存"的约定：表现层不许**要求**任一栈存在。
#[allow(clippy::type_complexity)]
pub fn refresh_combat_panel(
    mut panel: Query<&mut Text, With<CombatPanelText>>,
    actors: Query<(Entity, &Attributes, &Loadout), With<Playable>>,
    rules: Option<Res<RegenRules>>,
    window: Option<Res<ThreatWindow>>,
    feedback: Res<CombatFeedback>,
    statuses: Query<(Entity, &StatusIdentity, &StatusHostRule, &InvokedBy), With<Active>>,
    blocks: Query<&BlockedTags>,
    invokers: Query<&InvokedBy>,
    invoker_targets: Query<&InvokerTarget<IVec2>>,
    // 技能的**状态实体**：三个生命周期标记都挂在状态上，所以要沿 `SubstateOf` 回到技能根。
    states: Query<(Entity, Has<Ready>, Has<Invoking>, Has<Cooling>), With<Active>>,
    substates: Query<&SubstateOf>,
    // 剩余时间：延时转移边上的计时器（gearbox 自己维护，本层只读）。
    transitions: Query<&Transitions>,
    timers: Query<&EdgeTimer>,
) {
    let Ok(mut text) = panel.single_mut() else {
        return;
    };

    // 某个状态实体上那条延时边还剩多久。
    let remaining_of = |state: Entity| -> Option<f32> {
        transitions
            .get(state)
            .ok()?
            .into_iter()
            .find_map(|edge| timers.get(*edge).ok())
            .map(|timer| timer.0.remaining_secs())
    };

    // 状态实体 → 技能根 → 槽位状态（顺便记住"冷却中"的那个状态，好读它的倒计时）。
    let mut slots: std::collections::HashMap<Entity, SlotState> = std::collections::HashMap::new();
    let mut cooling_state: std::collections::HashMap<Entity, Entity> =
        std::collections::HashMap::new();
    for (state, ready, invoking, cooling) in &states {
        let root = resolve_root(&substates, state);
        // 同一个技能可能有多个状态同时 Active（`#Invoking` 与其子状态），
        // 所以按"最忙的那个"合并。
        let candidate = slot_state(ready, invoking, cooling);
        let entry = slots.entry(root).or_insert(SlotState::Dormant);
        if rank(candidate) > rank(*entry) {
            *entry = candidate;
        }
        if cooling {
            cooling_state.insert(root, state);
        }
    }

    let mut lines: Vec<String> = Vec::new();
    if let Some(line) = window.as_deref().and_then(threat_line) {
        lines.push(line);
    }

    for (actor, attributes, loadout) in &actors {
        lines.push(format!("── 角色 {actor:?} ──"));
        if let Some(rules) = rules.as_deref() {
            for row in pool_rows(attributes, rules) {
                lines.push(format!(
                    "{:<12} {:>6.1} / {:<6.1}  {}",
                    row.name,
                    row.current,
                    row.max,
                    bar(row.ratio())
                ));
            }
        }

        // 身上的状态（同 id 多层就合并计数；宿主判定与叠层/净化同一处实现）。
        let mut counts: std::collections::BTreeMap<String, (usize, Option<f32>)> =
            Default::default();
        for (state, identity, host_rule, invoked) in &statuses {
            if resolve_status_host(host_rule, invoked, &invokers, &invoker_targets) != Some(actor) {
                continue;
            }
            // 状态自己的倒计时：那条 `#Active → #Expired` 延时边上的计时器（取最长的那个当代表）。
            let remaining = remaining_of(state);
            let entry = counts.entry(identity.0.clone()).or_insert((0, None));
            entry.0 += 1;
            if let Some(remaining) = remaining {
                entry.1 = Some(entry.1.map_or(remaining, |old: f32| old.max(remaining)));
            }
        }
        for (id, (count, remaining)) in counts {
            let hint = match blocks.get(actor).map(|blocked| blocked.0.names()) {
                Ok(names) if !names.is_empty() => format!("（封 {names:?}）"),
                _ => String::new(),
            };
            let clock = remaining.map_or(String::new(), |s| format!("  {}", format_remaining(s)));
            lines.push(format!("状态：{id} ×{count}{clock}{hint}"));
        }

        for (slot, ability) in loadout.0.iter().enumerate() {
            let state = slots.get(ability).copied().unwrap_or(SlotState::Dormant);
            // 冷却中就把倒计时一起画出来（读的是 gearbox 的 `EdgeTimer`，不会漂）。
            let clock = if state == SlotState::Cooling {
                cooling_state
                    .get(ability)
                    .and_then(|state| remaining_of(*state))
                    .map_or(String::new(), |s| format!(" {}", format_remaining(s)))
            } else {
                String::new()
            };
            lines.push(format!("[{slot}] {ability:?} {}{clock}", state.label()));
        }
    }

    if let Some((ability, reason)) = feedback.last_rejection {
        lines.push(format!("（{ability:?} 被拒：{reason}）"));
    }

    text.0 = lines.join("\n");
}

/// 一行资源条（纯文本，先不引入图片）。
fn bar(ratio: f32) -> String {
    let filled = (ratio * 10.0).round().clamp(0.0, 10.0) as usize;
    format!("[{}{}]", "#".repeat(filled), ".".repeat(10 - filled))
}

fn rank(state: SlotState) -> u8 {
    match state {
        SlotState::Invoking => 3,
        SlotState::Cooling => 2,
        SlotState::Ready => 1,
        SlotState::Dormant => 0,
    }
}

/// 注册战斗面板。
pub struct CombatPanelPlugin;

impl Plugin for CombatPanelPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CombatFeedback>()
            .add_systems(Startup, spawn_combat_panel)
            .add_systems(
                Update,
                (
                    record_rejections,
                    refresh_combat_panel.after(record_rejections),
                ),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_state_prefers_the_busiest_marker() {
        assert_eq!(slot_state(true, false, false), SlotState::Ready);
        assert_eq!(slot_state(false, true, false), SlotState::Invoking);
        assert_eq!(slot_state(false, false, true), SlotState::Cooling);
        assert_eq!(slot_state(false, false, false), SlotState::Dormant);
        assert_eq!(
            slot_state(true, true, false),
            SlotState::Invoking,
            "同时亮着时显示最忙的那个"
        );
    }

    #[test]
    fn a_zero_max_pool_does_not_divide_by_zero() {
        let row = PoolRow {
            name: "Soul".into(),
            current: 3.0,
            max: 0.0,
        };
        assert_eq!(row.ratio(), 1.0);
    }
}
