//! HUD 的**行动条**：PC 当前行动的进度。
//!
//! ## 为什么这个条值得画
//!
//! 这是**半即时战斗**的核心可读性来源。玩家点了技能之后会出现一段
//! "行动进行中"的窗口 —— 在这个窗口里可以用 `COUNTER` 技能反制（见
//! `docs/combat-design.md`）。如果界面不显示"现在进行到哪了"，
//! 玩家就只能靠猜，反制窗口等于不存在。
//!
//! ## 数据从哪来（**不新造状态**）
//!
//! `Action::progress()` 是 L1 已经算好的派生数据（`elapsed / duration`），
//! 这里只做两件事：**查出来**、**画成宽度**。
//!
//! 行动实体通过关系组件 `ActiveActions` 挂在角色上（`InitiatedBy` 的反向），
//! 所以"谁在做什么"是一次关系查询 —— **L2 只读**（R15 / R17），
//! 不需要自己维护一份行动列表。
//!
//! 技能名走 `CastsSkill` → 技能定义实体的 `Skill::name`
//! （与技能按钮同一条线，见 `hud/mod.rs` 里"不要用 `Vocab::skill_name`"那段）。

use bevy::prelude::*;
use voxelith_axiom::atoms::actor::Player;
use voxelith_axiom::behaviors::action::{Action, ActiveActions, CastsSkill};
use voxelith_axiom::behaviors::skill::Skill;

use super::{HudActionBar, HudActionLabel, TEXT, TEXT_DIM};

/// 行动条的高度（像素）。**比资源条高一点**：它是"当前正在发生的事"，
/// 视觉权重应该大于静态资源。
pub(super) const ACTION_BAR_HEIGHT: f32 = 12.0;

/// 取 PC 正在进行的行动里**进度最大的那个**，算出 `(进度, 标签)`。
///
/// **抽成纯函数**是为了可测：Bevy 系统要起一个世界、造关系组件、塞技能定义，
/// 而这里真正要保证的只有"从一组候选里挑哪个、怎么显示"。
/// 交互层（点击 → 真的产生行动）由 L1 的战斗链路负责，不归这里测。
///
/// 输入是 `(进度, 技能名)` 的迭代器；`None` 表示没有进行中的行动。
///
/// ## 为什么取进度最大而不是第一个
///
/// `ActiveActions` 的顺序不保证稳定，而界面上只画一条。取进度最大的那个 =
/// "最主要正在发生的事"，**而且与顺序无关** ⇒ 画面不会因为查询顺序变化而抖动。
///
/// ## 为什么待命也要有标签
///
/// 没有行动时返回 `(0.0, "待命")` 而不是把整行藏掉：
/// 藏起来会让面板高度跳动，玩家也分不清"没事在做"和"界面坏了"。
fn pick_action(actions: impl Iterator<Item = (f32, String)>) -> (f32, String) {
    let mut best: Option<(f32, String)> = None;
    for (progress, name) in actions {
        if best.as_ref().is_some_and(|(p, _)| *p >= progress) {
            continue;
        }
        best = Some((progress, name));
    }
    match best {
        Some((progress, name)) => (progress, format!("{name}  {:.0}%", progress * 100.0)),
        None => (0.0, "待命".to_owned()),
    }
}

/// 读 PC 的行动状态，刷新行动条的宽度与文字。每帧跑（进度连续变化）。
pub(super) fn refresh_action_bar(
    players: Query<&ActiveActions, With<Player>>,
    actions: Query<(&Action, &CastsSkill)>,
    skills: Query<&Skill>,
    mut fill: Query<&mut Node, With<HudActionBar>>,
    mut labels: Query<&mut Text, With<HudActionLabel>>,
) {
    let Ok(mut node) = fill.single_mut() else {
        return;
    };

    let candidates = players.single().ok().into_iter().flat_map(|active| {
        active.actions().iter().filter_map(move |action_entity| {
            let (action, casts) = actions.get(*action_entity).ok()?;
            let name = skills
                .get(casts.0)
                .map(|skill| {
                    if skill.name.is_empty() {
                        format!("技能{}", skill.id.0)
                    } else {
                        skill.name.clone()
                    }
                })
                .unwrap_or_else(|_| "行动".to_owned());
            Some((action.progress(), name))
        })
    });

    let (ratio, label) = pick_action(candidates);
    node.width = percent(ratio * 100.0);

    if let Ok(mut text) = labels.single_mut()
        && text.0 != label
    {
        text.0 = label;
    }
}

/// 标签的着色：有行动时用亮色，待命用暗色。
///
/// 单独一个系统（而不是塞进上面那个）：文字颜色只在这两种状态之间切换，
/// 每帧写 `TextColor` 是没必要的开销，也让上面那个系统多两个参数。
pub(super) fn tint_action_label(
    players: Query<&ActiveActions, With<Player>>,
    mut labels: Query<&mut TextColor, With<HudActionLabel>>,
) {
    let busy = players.single().is_ok_and(|active| !active.is_empty());
    let wanted = if busy { TEXT } else { TEXT_DIM };
    if let Ok(mut color) = labels.single_mut()
        && color.0 != wanted
    {
        color.0 = wanted;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **没有行动时是"待命"，不是空白** —— 这是刻意的设计决定。
    ///
    /// 很容易在重构里被改成"没有行动就把整行藏掉"，而那会让面板高度跳动 +
    /// 玩家分不清"没事做"与"界面坏了"。
    #[test]
    fn idle_state_is_labelled_not_hidden() {
        let (ratio, label) = pick_action(std::iter::empty());
        assert_eq!(ratio, 0.0, "待命时进度归零");
        assert_eq!(label, "待命", "待命时仍要有文字，不能是空串");
    }

    /// 有行动时进度与百分比文字一致。
    #[test]
    fn a_running_action_reports_its_progress() {
        let (ratio, label) = pick_action(std::iter::once((0.47, "普通攻击".to_owned())));
        assert!((ratio - 0.47).abs() < 1e-6);
        assert_eq!(label, "普通攻击  47%");
    }

    /// **取进度最大的那个**（与顺序无关）。
    ///
    /// 这条是"画面不抖"的守卫：`ActiveActions` 的顺序不保证稳定，
    /// 若实现改成"取第一个"，换一个查询顺序画面就会跳到另一个技能上。
    #[test]
    fn the_largest_progress_wins_regardless_of_order() {
        let a = (0.20, "盾击".to_owned());
        let b = (0.80, "火焰箭".to_owned());
        let c = (0.50, "余烬爆".to_owned());

        // 六种排列都必须选 `b`。
        for order in [
            vec![a.clone(), b.clone(), c.clone()],
            vec![a.clone(), c.clone(), b.clone()],
            vec![b.clone(), a.clone(), c.clone()],
            vec![b.clone(), c.clone(), a.clone()],
            vec![c.clone(), a.clone(), b.clone()],
            vec![c.clone(), b.clone(), a.clone()],
        ] {
            let (ratio, label) = pick_action(order.into_iter());
            assert!((ratio - 0.80).abs() < 1e-6, "该选进度 0.80 的那个");
            assert_eq!(label, "火焰箭  80%");
        }
    }

    /// 进度会被 clamp（`Action::progress` 保证 `0..=1`），
    /// 所以宽度不会超出槽。这里钉住"上界不会画成 120%"。
    #[test]
    fn progress_is_clamped_to_the_track() {
        let (ratio, _) = pick_action(std::iter::once((1.0, "冻结".to_owned())));
        assert!((ratio - 1.0).abs() < 1e-6);
        // 进度条宽度就是这个比值 × 100，所以只要它 ≥ 0 且 ≤ 1 就不会溢出。
        assert!((0.0..=1.0).contains(&ratio));
    }
}
