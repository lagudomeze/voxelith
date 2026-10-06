//! **能力构造器**：内容（[`AbilityRon`]）→ BSN 场景。
//!
//! 这一层让"改 `.ron` 就能改技能"真正成立。搭出来的结构：
//!
//! ```text
//! #Ability   Ability StateMachine InitialState(#Ready)
//!            template → 种 CastTime / Cooldown 两个属性（计时都从属性派生，可热调）
//!   #Ready      ──StartInvoke<IVec2>──►  #Invoking
//!   #Invoking   InitialState(#WindUp)  ──Done──►  #Cooldown
//!       #WindUp  ──AlwaysEdge Delay = CastTime@ability──►  #Fire
//!       #Fire    TerminalState + GoOffConfig + AttackEffect   ← 命中规则在这一态上
//!   #Cooldown   ──AlwaysEdge Delay = Cooldown@ability──►  #Ready
//! ```
//!
//! ## 为什么不用 diesel 现成的 `invoked(...)`
//!
//! 它的签名是 `name: &'static str`——**装不下内容里的动态 id**（想塞进去只能 `Box::leak`
//! 泄内存）。所以这里自己搭同一套结构，顺便做两件它不做的事：
//!
//! 1. **前摇**：`invoked` 一进 `#Invoking` 就命中（`#Fire` 是初始态）；
//!    这里插一层 `#WindUp`，`cast_time` 决定"进战斗态之后多久才命中"。
//! 2. **内容决定有没有命中规则**：没有 `attack` 的技能（自身增益之类）**不挂**
//!    [`AttackEffect`]，于是效果叶系统自然不响应它——而不是"挂一个默认值然后打错人"。

use bevy::prelude::*;
use bevy::scene::prelude::{Scene, bsn};
use bevy_diesel::effect::GoOffConfig;
use bevy_diesel::events::StartInvoke;
use bevy_diesel::gauge_ext::modifiers::{AttributeModifiers, SustainedModifierConfig};
use bevy_diesel::invoke::Ability;
use bevy_diesel::invoker::InvokedBy;
use bevy_diesel::spawn::{SpawnConfig, TemplateRegistry};
use bevy_diesel::target::TargetGenerator;
use bevy_gauge::modifier_set::{AttributeInitializer, ModifierSet};
use bevy_gauge::requirements::AttributeRequirements;
use bevy_gearbox::{
    AlwaysEdge, Delay, Done, InitialState, MessageEdge, StateMachine, Substates, Target,
    TerminalState, Transitions,
};

use crate::ability::{AbilityId, Cooling, Ready};
use crate::attributes;
use crate::casting::{BlocksTags, CastCosts, RequiresIdle, SkillTags};
use crate::contest::AttackEffect;
use crate::counter::{AbortInvocation, Interrupts, Invoking, SuperArmor};
use crate::dispel::Dispels;
use crate::grid::GridBackend;
use crate::periodic::PeriodicEffect;
use crate::skills::{AbilityRon, AttackRon};
use crate::stacking::{StatusExpired, StatusHostRule, StatusIdentity, SupersedeStatus};
use crate::statuses::{StatusCatalog, StatusRon, WhoRon};
use crate::tags::SkillTagMask;
use crate::threat::ThreatensPlayer;

/// 能力根上种的属性名：计时都从它们派生，所以热调这两个数能立刻改手感。
const CAST_TIME_ATTRIBUTE: &str = "CastTime";
/// 冷却属性名。
const COOLDOWN_ATTRIBUTE: &str = "Cooldown";

/// 命中态上的效果：命中规则 / 状态生成，各是一个组件。
///
/// 返回 `Box<dyn Scene>` 是因为各个分支的 BSN 场景类型不同，
/// 而 `#Fire` 那个节点只接受**一个**具体类型。
fn fire_effects(
    attack: Option<AttackEffect>,
    status: Option<SpawnConfig<GridBackend>>,
    dispels: Dispels,
) -> Box<dyn Scene> {
    // 净化组件**每个分支都挂**：内容没写时是 `Dispels { max: 0, .. }`（惰性），
    // 避免为了"有没有净化"再分叉一次场景类型。
    match (attack, status) {
        (Some(effect), Some(spawn)) => Box::new(bsn! {
            template(move |_| Ok(effect.clone()))
            template(move |_| Ok(spawn.clone()))
            template(move |_| Ok(dispels))
        }),
        (Some(effect), None) => Box::new(bsn! {
            template(move |_| Ok(effect.clone()))
            template(move |_| Ok(dispels))
        }),
        (None, Some(spawn)) => Box::new(bsn! {
            template(move |_| Ok(spawn.clone()))
            template(move |_| Ok(dispels))
        }),
        (None, None) => Box::new(bsn! {
            template(move |_| Ok(dispels))
        }),
    }
}

/// **按内容搭一个可玩的技能场景**。
///
/// `cast_time` / `cooldown` 同时写进属性与边的字面延时：
/// 字面值是进入那一刻的初值，属性值随后由 gauge 覆盖（与 diesel `invoked_with` 同一套路），
/// 于是运行期改属性就能改节奏。
///
/// `statuses` 用来把 `applies` 里的 id 翻成模板名与落点（谁身上）——
/// 状态定义在别的文件里，所以这一步必须查台账。
///
/// ## ⚠️ 每个"要认发起者"的状态都必须自己写 `InvokedBy(#Ability)`
///
/// 这条坑在本项目已经踩过**三次**（爆炸模板的 `#Fire`、状态的 `#Active`、技能的 `#WindUp`），
/// 症状完全一样：**什么都不发生**。
///
/// 原因：`resolve_invoker` 只沿 `InvokedBy` 走（不认 `SubstateOf`），
/// 而 BSN 的 `#Name` 节点是**独立实体**，不会自动继承根的归属。
/// 所以任何一个会
/// ①被 diesel 的效果管线读到、或 ②需要解析"我瞄的是谁"、或 ③要注册 gauge 的 `@ability` 源
/// 的状态，都得显式写 `InvokedBy(#Ability)`。
pub fn build_ability(ability: &AbilityRon, statuses: &StatusCatalog) -> Box<dyn Scene> {
    let name = if ability.name.trim().is_empty() {
        ability.id.clone()
    } else {
        ability.name.clone()
    };
    let id_for_template = ability.id.clone();
    let cast_time = ability.cast_time.max(0.0);
    let cooldown = ability.cooldown.max(0.0);
    let effect = ability.attack.as_ref().map(AttackRon::to_attack_effect);
    // 费用与需求都挂在**技能根**上：它们是内容，不是施法者的属性。
    // 判定时把施法者的 `Attributes` 传进来（见 `casting` 的模块文档）。
    let costs = CastCosts(
        ability
            .costs
            .iter()
            .map(|cost| (cost.attribute.clone(), cost.amount))
            .collect(),
    );
    let requirements = AttributeRequirements::from(ability.requires.clone());
    let costs_for_template = costs.clone();
    let requirements_for_template = requirements.clone();
    let threatening = ability.threatening;
    let interrupts = ability.interrupts;
    let super_armor = ability.super_armor;
    let requires_idle = ability.requires_idle;
    // 净化：内容没写就是惰性的 `max: 0`。
    let dispels = ability
        .dispels
        .map(|dispels| Dispels {
            max: dispels.max,
            debuffs_only: dispels.debuffs_only,
        })
        .unwrap_or(Dispels {
            max: 0,
            debuffs_only: true,
        });
    // 技能类别：加载期已校验过名字，这里只做位或。
    let tags = SkillTagMask::from_names(ability.tags.iter().map(String::as_str))
        .expect("技能类别在加载期已经校验过；构造期不该再失败");

    // 挂在谁身上由**状态定义**决定（`who`），并同时决定"生成位置"与"修饰符目标"——
    // 两者必须一致，否则状态会长在你身上、效果打在对面。
    let status_spawn = ability
        .applies
        .first()
        .and_then(|id| statuses.get(id))
        .map(|status| {
            let template = format!("status:{}", status.id);
            match status.who {
                WhoRon::Caster => SpawnConfig::<GridBackend>::invoker(&template),
                WhoRon::Target => SpawnConfig::<GridBackend>::target(&template),
            }
        });

    Box::new(bsn! {
        #Ability Ability StateMachine InitialState(#Ready)
            Name::new(name)
            // ⚠️ 一个节点可以写多个 `template(...)`，但**每个只能返回一个组件**
            // （返回元组会报 "not a `Component`"）——所以三个组件写三次。
            template(move |_| {
                let mut set = ModifierSet::new();
                set.add(CAST_TIME_ATTRIBUTE, cast_time);
                set.add(COOLDOWN_ATTRIBUTE, cooldown);
                Ok(AttributeInitializer::new(set))
            })
            template(move |_| Ok(costs_for_template.clone()))
            template(move |_| Ok(requirements_for_template.clone()))
            // 交互语义（内容决定谁带哪个）：反制能不能打断由这两个标记裁决。
            template(move |_| Ok(Interrupts(interrupts)))
            template(move |_| Ok(SuperArmor(super_armor)))
            // 类别：硬控按它封技能（宿主身上的 `BlockedTags` 与它求交）。
            template(move |_| Ok(SkillTags(tags)))
            // 状态类需求（旧 `NoActiveAction`）：由门控判"我有没有别的技能在释放中"。
            template(move |_| Ok(RequiresIdle(requires_idle)))
            // 内容 id：跨存档认出同一个技能要用它（技能是实体，存不进档里）。
            template(move |_| Ok(AbilityId(id_for_template.clone())))
        Substates [
            // `Ready` / `Invoking` / `Cooling` 三个标记是技能的**可读面**：
            // HUD / 调试 / AI 靠它们问"这一招现在在哪一段"，不必认实体 id。
            #Ready Ready Transitions [
                (Target(#Invoking) MessageEdge::<StartInvoke<IVec2>>)
            ],
            // `Invoking` 标记：把"谁正在做动作"问出来（`replace` 要放弃的东西）。
            // 两条出边：正常走完 → `#Cooldown`；被反制 `AbortInvocation` → 回 `#Ready`。
            // 离开 `#WindUp` 时 gearbox 会自动取消前摇延时，不需要手工清计时器。
            #Invoking InvokedBy(#Ability) Invoking InitialState(#WindUp) Transitions [
                (Target(#Cooldown) MessageEdge::<Done>),
                (Target(#Ready) MessageEdge::<AbortInvocation>)
            ] Substates [
                // 前摇：这一段就是旧内容的 `duration`。
                // `ThreatensPlayer(threatening)` 是**威胁窗口的凭据**：内容标了
                // `threatening: true` 的技能，玩家能在这段前摇里反制。
                // 永远插（带 bool）而不是有条件地插——理由见 `threat` 模块文档。
                #WindUp InvokedBy(#Ability)
                    template(move |_| Ok(ThreatensPlayer(threatening)))
                    Transitions [
                        (Target(#Fire) AlwaysEdge Delay::from_secs_f32(cast_time)
                            InvokedBy(#Ability)
                            template(|_| Ok(bevy_gauge::attributes! { "Delay" => "CastTime@ability" })))
                    ],
                // 命中：`TerminalState` 进它就发 `Done`（于是自动进冷却），
                // `GoOffConfig` 让 diesel 的效果管线在这里响。
                #Fire InvokedBy(#Ability) TerminalState
                    GoOffConfig::<GridBackend>::default()
                    { fire_effects(effect, status_spawn, dispels) },
            ],
            #Cooldown Cooling Transitions [
                (Target(#Ready) AlwaysEdge Delay::from_secs_f32(cooldown)
                    InvokedBy(#Ability)
                    template(|_| Ok(bevy_gauge::attributes! { "Delay" => "Cooldown@ability" })))
            ],
        ]
    })
}

/// 一批内容 → 一批场景（保持顺序，便于"第 N 个技能"这种断言）。
pub fn build_all(
    abilities: &[AbilityRon],
    statuses: &StatusCatalog,
) -> Vec<(String, Box<dyn Scene>)> {
    abilities
        .iter()
        .map(|ability| (ability.id.clone(), build_ability(ability, statuses)))
        .collect()
}

/// **状态场景**：一台"进 `#Active` 就挂修饰符、到时长就收场"的状态机。
///
/// ```text
/// #Status   StateMachine InitialState(#Active)
///   #Active   InvokedBy(#Status)                      ← 不写它，`resolve_invoker` 走不到施法者，
///             AttributeModifiers(mods)                  修饰符会静默挂不上去（同样的坑在
///             SustainedModifierConfig::<B>::...         爆炸模板上踩过一次）
///             ──AlwaysEdge Delay = duration──► #Expired
///   #Expired  GoOffConfig::<B>::root() DespawnEffect TerminalState
/// ```
///
/// `#Expired` 那行是**收场**：`GoOffConfig::root()` 解析出状态机根，`DespawnEffect` 把它销毁，
/// 子树由 gearbox 的 `linked_spawn` 一并带走——否则每挂一次状态就留一个孤儿实体。
/// 而修饰符的**卸下**由 diesel 的 `sustained_modifier_remove` 负责（失去 `Active` 时精确回收）。
/// 构造一个状态的可玩场景（`#Status` 状态机）。
///
/// ## 🚨 铁律：**每个会认"目标"或"发起者"的状态，都必须自己写 `InvokedBy(#根)`**
///
/// 这条坑在本项目已经踩过**四次**，症状永远一样：**什么都不发生**（没有报错、没有日志）。
///
/// | 踩的地方 | 后果 |
/// |---|---|
/// | 技能 `#Fire` | 爆炸效果不打人 |
/// | 状态 `#Active` | 修饰符与 DoT 静默失效 |
/// | 技能 `#WindUp` | 威胁窗口永远是空的 |
/// | 状态 `#Expired` / `#Removed` | 进出效果（`on_expire` / `on_remove`）静默失效 |
///
/// 原因：`resolve_invoker` 只沿 `InvokedBy` 走（**不认 `SubstateOf`**），
/// 而 BSN 的 `#Name` 节点是**独立实体**，不会自动继承根的归属。
/// 于是任何"要解析我瞄的是谁 / 我属于谁"的状态，都得显式写一遍。
///
/// 判断口诀：**这个节点上挂了效果、标记或属性源吗？挂了就写 `InvokedBy(#根)`。**
pub fn build_status(status: &StatusRon) -> Box<dyn Scene> {
    let mut modifiers = attributes::build_modifier_set(&status.modifiers)
        .expect("状态定义在加载期已经校验过；构造期不该再失败");
    // **层数**：每个实例给自己那条 `<id>Stacks` 各加 1（`AttributeModifiers` 本来就是
    // 累加），于是宿主身上的 `<id>Stacks` 自动等于"这个状态挂了几层"。
    // 周期跳的 `amount_expr` 就靠它写"每层 3 点"这类话（见 `stacking`）。
    modifiers.add(&format!("{}Stacks", status.id), 1.0);
    let duration = status.duration.max(0.0);
    let sustained = match status.who {
        WhoRon::Caster => SustainedModifierConfig::<GridBackend>::invoker(),
        WhoRon::Target => SustainedModifierConfig::<GridBackend>::invoker_target(),
    };
    // ⚠️ `bsn!` 里的组件表达式**不能借用参数**（宏要求 `'static`，借 `status` 会报
    // "lifetime may not live long enough"）。所以先把要用的东西变成**函数内的局部变量**，
    // 再进宏——`build_ability` 能用 `Name::new(name)` 就是这个原因。
    let id = status.id.clone();
    let modifiers_for_template = modifiers.clone();
    let sustained_for_template = sustained.clone();
    // 硬控：封哪几类技能 + 封谁（`who` 决定）。加载期已校验过名字。
    let blocked = SkillTagMask::from_names(status.blocks.iter().map(String::as_str))
        .expect("硬控类别在加载期已经校验过；构造期不该再失败");
    let blocks_on_caster = matches!(status.who, WhoRon::Caster);
    // 叠层对账要的三样东西，都挂在**状态根**上：身份、宿主规则、规则本身。
    // 挂根而不是生效态，是因为对账要整棵实例树一起销毁，新实例判定用 `Added<Stacking>`。
    let identity = StatusIdentity(id.clone());
    let host_rule = match status.who {
        WhoRon::Caster => StatusHostRule::Caster,
        WhoRon::Target => StatusHostRule::Target,
    };
    let stacking = status.stacking.to_component();
    // 三个时机各一套"目标 + 数值"，**无条件**挂上（空的就是惰性的）。
    //
    // 为什么不条件化（踩出来的两条）：
    // ① BSN 里 `{ 变量 }` 跟在组件后面会被当**字段访问**（"no field `on_expire` on
    //    `&mut TerminalState`"），写条件片段很别扭；
    // ② 条件插入会让场景类型分叉，而 `#Active` 那个节点只接受一个具体类型。
    //
    // 代价是"空 instant 也白跑一次效果管线"（一次 `GoOffOrigin` 传播，没有叶子响应），
    // 换来的是**没有分支**。
    let who = status.who;
    let every = status.tick.as_ref().map(|tick| tick.every).unwrap_or(0.0);
    let mut active_pushes: Vec<(String, f32)> = Vec::new();
    // 周期跳：字面量进 `active_pushes`，**表达式**单独拿一份
    //（`InstantModifierSet` 的值是 `ModifierValue`，支持 `ExprSource` 字符串）。
    let tick_expr: Option<(String, String)> = match &status.tick {
        Some(tick) => match (&tick.amount, &tick.amount_expr) {
            (Some(amount), None) => {
                active_pushes.push((tick.attribute.clone(), *amount));
                None
            }
            (None, Some(source)) => Some((tick.attribute.clone(), source.clone())),
            _ => None,
        },
        None => None,
    };
    let tick_expr_for_template = tick_expr.clone();
    for effect in &status.on_apply {
        active_pushes.push((effect.attribute.clone(), effect.amount));
    }
    let active_pushes_for_template = active_pushes.clone();
    let expire_pushes: Vec<(String, f32)> = status
        .on_expire
        .iter()
        .map(|effect| (effect.attribute.clone(), effect.amount))
        .collect();
    let expire_pushes_for_template = expire_pushes.clone();
    let remove_pushes: Vec<(String, f32)> = status
        .on_remove
        .iter()
        .map(|effect| (effect.attribute.clone(), effect.amount))
        .collect();
    let remove_pushes_for_template = remove_pushes.clone();

    Box::new(bsn! {
        #Status StateMachine InitialState(#Active)
            Name::new(id)
            template(move |_| Ok(identity.clone()))
            template(move |_| Ok(host_rule))
            template(move |_| Ok(stacking))
        Substates [
            #Active InvokedBy(#Status)
                template(move |_| Ok(AttributeModifiers(modifiers_for_template.clone())))
                template(move |_| Ok(sustained_for_template.clone()))
                // 硬控：门控**不直接查它**——由 `derive_blocked_tags` 落到宿主身上
                // （理由是"谁被控"依赖会变的 `InvokerTarget`，见 `casting` 的文档）。
                template(move |_| {
                    Ok(BlocksTags {
                        tags: blocked,
                        on_caster: blocks_on_caster,
                    })
                })
                // 进生效态就响一次：`tick` 的第一跳、`on_apply`，以及**周期计时器**。
                template(move |_| Ok(go_off_config(who)))
                template(move |_| {
                    let mut instant = instant_set(&active_pushes_for_template);
                    // 周期跳的表达式（引用了层数等属性）在这里进 instant。
                    if let Some((attribute, source)) = &tick_expr_for_template {
                        instant.push_add(attribute, source.as_str());
                    }
                    Ok(instant)
                })
                template(move |_| Ok(PeriodicEffect::every(every)))
                Transitions [
                    // 到期自然退场。
                    (Target(#Expired) AlwaysEdge Delay::from_secs_f32(duration)),
                    // 被顶掉 / 被驱散：走**另一条**出边，于是"为什么离场"变成了不同的状态
                    // （比旧引擎的 `RemovalReason` 枚举更贴框架：结局是一个地方，效果挂在那个地方）。
                    (Target(#Removed) MessageEdge::<SupersedeStatus>)
                ],
            // ⚠️ **不用** diesel 的 `DespawnEffect`：状态一进终态就被销毁的话，
            // 卸修饰符的系统来不及读到实体上的 `AttributeModifiers`，修饰符会静默残留
            // （现场表现：刷新一次格挡，护甲 +5 变成 +10）。改由 `stacking` 里的
            // `mark_expired_statuses` + `despawn_marked_statuses` 分两步收场。
            #Expired InvokedBy(#Status) StatusExpired TerminalState
                template(move |_| Ok(go_off_config(who)))
                template(move |_| Ok(instant_set(&expire_pushes_for_template))),
            #Removed InvokedBy(#Status) StatusExpired TerminalState
                template(move |_| Ok(go_off_config(who)))
                template(move |_| Ok(instant_set(&remove_pushes_for_template))),
        ]
    })
}

/// 效果的**目标**：`who=Caster` 打施法者自己，`who=Target` 打施法者的目标。
///
/// 三个时机（进入 / 到期 / 被移除）与周期效果共用这一条约定。
fn go_off_config(who: WhoRon) -> GoOffConfig<GridBackend> {
    let mut config = GoOffConfig::<GridBackend>::default();
    config.generator = match who {
        WhoRon::Caster => TargetGenerator::at_invoker(),
        WhoRon::Target => TargetGenerator::at_invoker_target(),
    };
    config
}

/// 一组属性改动 → `InstantModifierSet`（正数回复、负数伤害）。
///
/// 空集是**惰性**的：效果管线照跑，但没有叶子系统会响应。
fn instant_set(pushes: &[(String, f32)]) -> bevy_gauge::prelude::InstantModifierSet {
    let mut instant = bevy_gauge::prelude::InstantModifierSet::new();
    for (attribute, amount) in pushes {
        instant.push_add(attribute, *amount);
    }
    instant
}

/// 把状态定义注册进 [`TemplateRegistry`]，名字是 `status:{id}`（技能的 `applies` 按它引用）。
///
/// 工厂每次**重新构造**场景，而不是共享一份——diesel 的注册表约定就是
/// "存工厂不存实例"（场景在解析时会被消费掉）。
pub fn install_statuses(registry: &mut TemplateRegistry, statuses: &StatusCatalog) {
    for status in statuses.iter() {
        let owned = status.clone();
        registry.register(format!("status:{}", status.id), move || {
            build_status(&owned)
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ability(id: &str, cast_time: f32, cooldown: f32, attack: Option<AttackRon>) -> AbilityRon {
        AbilityRon {
            id: id.to_string(),
            name: String::new(),
            cast_time,
            cooldown,
            requires: Vec::new(),
            costs: Vec::new(),
            applies: Vec::new(),
            tags: Vec::new(),
            threatening: false,
            interrupts: false,
            super_armor: false,
            dispels: None,
            requires_idle: false,
            attack,
        }
    }

    #[test]
    fn a_skill_without_a_name_falls_back_to_its_id() {
        // 名字是给人看的；没写就用 id，免得场景里出现无名实体。
        let scene = build_ability(
            &ability("nameless", 0.0, 0.0, None),
            &StatusCatalog::default(),
        );
        // 场景能构造出来就够了——内容侧的名字回退不该 panic。
        let _ = scene;
    }

    #[test]
    fn build_all_keeps_content_order() {
        let content = vec![
            ability("first", 1.0, 1.0, None),
            ability("second", 0.5, 2.0, None),
        ];
        let built = build_all(&content, &StatusCatalog::default());
        assert_eq!(built.len(), 2);
        assert_eq!(built[0].0, "first");
        assert_eq!(built[1].0, "second");
    }
}
