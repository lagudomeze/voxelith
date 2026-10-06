//! 行动提交：**唯一入口**（`try_start` / `replace`）。
//!
//! 所有写入方都只能发 [`StartAction`] 消息，由 [`commit_actions`] 这一个系统串行处理：
//!
//! ```text
//! AI 意图 / 玩家请求 / 效果 RequestAction
//!        └─► StartAction 消息 ─► commit_actions ─► 生成 Action 实体（唯一 spawn 点）
//! ```
//!
//! ## 为什么必须是"一个系统 + 一个循环"
//!
//! "检查槽 →（怪物还要）检查窗口 → 占槽 → 建实体"必须是**一段不可分割的代码**：
//! 中间不能有"别人还能看到旧状态"的间隙。原因有二：
//!
//! 1. **检查读的是 Query，写入走的是 `Commands`（延迟）**——同一个系统里连着处理两条请求时，
//!    第二条**看不见**第一条刚建的行动。所以本系统用 `Local<EntityHashSet>` 记住
//!    "本帧已提交过的 actor"，让"每帧至多一次"成为真的，而不是靠运气。
//! 2. **窗口是全局资源**：两只怪同帧都满足条件时，必须按同一份状态串行裁决。
//!
//! ## 语义
//!
//! | 情况 | 结果 |
//! |---|---|
//! | 槽被占 + `replace = false` | **这一帧不出手**（意图每帧重算，失败即无副作用） |
//! | 槽被占 + `replace = true` | **放弃自己当前的行动**（`cancel(self)`，不退费用/冷却），再建新的 |
//! | 是怪物 + 窗口非空 | 不出手（等窗口关掉；冻结期间时间也是停的） |
//! | 提交成功 | 生成行动；是怪物就打 `Threat` 并发 [`ThreatensPlayer`] 通知 |
//! | 玩家用 `replace` 提交 | **清空威胁窗口**（= 解冻 + 关闭本次决策机会；**不取消**未被反制的威胁） |
//!
//! 取消别人的行动不走这里：那是 [`Effect::CancelAction`](crate::behaviors::effect::Effect)
//! 在效果结算时由**组件裁决**（`Interrupts` / `SuperArmor`）决定的。

use bevy_ecs::entity::EntityHashSet;
use bevy_ecs::prelude::*;
use bevy_ecs::system::Local;

use crate::atoms::action::{Action, ActiveActions, CastsSkill, InitiatedBy, ResolveNow, Threat};
use crate::atoms::actor::{InputDriven, Monster};
use crate::behaviors::skill::Skill;
use crate::behaviors::threat::{ThreatWindow, ThreatensPlayer};

/// 请求开始一条行动（**Message**，请求型）。
///
/// 生产者：AI 意图、玩家释放请求、效果 `RequestAction`。
/// 消费者：[`commit_actions`]。
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartAction {
    /// 谁出手。
    pub actor: Entity,
    /// 释放哪个技能（**技能定义实体**）。
    pub skill: Entity,
    /// 目标（`Targeting` 的第一步输入；可以没有，由技能自己的 `Targeting` 解析）。
    pub target: Option<Entity>,
    /// 是否**替换**当前行动（先 `cancel(self)` 再建）。
    ///
    /// 反制走这条：它要放弃自己正在做的事。
    pub replace: bool,
}

impl StartAction {
    /// 普通提交（槽空才能进）。
    pub fn new(actor: Entity, skill: Entity, target: Option<Entity>) -> Self {
        Self {
            actor,
            skill,
            target,
            replace: false,
        }
    }

    /// 替换式提交（反制）。
    pub fn replacing(actor: Entity, skill: Entity, target: Option<Entity>) -> Self {
        Self {
            actor,
            skill,
            target,
            replace: true,
        }
    }
}

/// **唯一**的提交系统。
///
/// 顺序契约：它必须排在所有意图产生者**之后**（否则本帧的意图要等下一帧），
/// 且排在 `collect_threats` 与 `update_phase` **之前**。
pub fn commit_actions(
    mut requests: MessageReader<StartAction>,
    mut commands: Commands,
    mut committed: Local<EntityHashSet>,
    mut window: ResMut<ThreatWindow>,
    mut threats: MessageWriter<ThreatensPlayer>,
    slots: Query<Option<&ActiveActions>>,
    skills: Query<&Skill>,
    monsters: Query<(), With<Monster>>,
    players: Query<Entity, InputDriven>,
) {
    // `Local` 跨帧存活，所以每帧开头清一次 —— "本帧已提交"只在**本帧**有意义。
    committed.clear();
    let pc = players.iter().next();

    for request in requests.read() {
        // ① 每帧一次：同一个 actor 的第二条请求一律丢弃（不静默替换）。
        if committed.contains(&request.actor) {
            continue;
        }
        let Ok(skill) = skills.get(request.skill) else {
            continue;
        };

        // ② 自己的槽：占着就得 `replace` 才进得去。
        let held: Vec<Entity> = slots
            .get(request.actor)
            .ok()
            .flatten()
            .map(|active| active.actions().to_vec())
            .unwrap_or_default();
        if !held.is_empty() && !request.replace {
            continue;
        }

        // ③ 怪物还要等窗口空（窗口是全局的：它由"未处理的威胁"定义）。
        let monster = monsters.contains(request.actor);
        if monster && window.is_open() {
            continue;
        }

        // ④ 检查都过了，才动世界：先放弃自己的旧行动（`cancel(self)`）。
        if request.replace {
            for action in &held {
                commands.entity(*action).try_despawn();
            }
            if !monster {
                // 玩家做了决策：关闭窗口（= 解冻 + 不再提供本次决策机会）。
                // **不取消**未被反制的威胁 —— 它们照旧推进并结算。
                window.clear();
            }
        }

        // ⑤ 唯一 spawn 点。
        let entity = commands
            .spawn((
                Action::wind_up(skill.duration, request.target),
                CastsSkill(request.skill),
                InitiatedBy(request.actor),
            ))
            .id();
        if skill.duration <= 0.0 {
            commands.entity(entity).insert(ResolveNow);
        }
        if monster {
            commands.entity(entity).insert(Threat);
            if let Some(pc) = pc {
                threats.write(ThreatensPlayer {
                    action: entity,
                    source: request.actor,
                    target: pc,
                });
            }
        }
        committed.insert(request.actor);
    }
}
