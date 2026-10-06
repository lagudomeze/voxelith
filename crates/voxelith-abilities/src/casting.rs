//! **释放请求与门控**：校验需求 → 扣费 → 发 `StartInvoke`。
//!
//! 这一层对应旧引擎的 `CastRequest` / `cast_requests`：**想要放技能就发请求，
//! 而不是直接写 `StartInvoke`**。因为门控必须发生在"进状态机之前"：
//!
//! ```text
//! CastRequest → check_cast_requests → CastAccepted / CastRejected
//!                                     └─► apply_cast → 扣费 + StartInvoke
//! ```
//!
//! ## 为什么门控要自己做
//!
//! gauge 有 [`AttributeRequirements`]（布尔属性需求），但它是**手动查的**：
//!
//! ```text
//! AttributeRequirements::met(&attributes) -> bool     // 它不会自己拦任何东西
//! ```
//!
//! gearbox 0.8 里也**没有守卫概念**（`grep Requirement|guard` 零命中）——它的边只看消息。
//! 所以"不满足就不能放"必须由请求入口实现。这条与旧引擎的做法一致，
//! 只是判定从闭集枚举换成了 **gauge 表达式**（`"Action >= 1.0"`）。
//!
//! ## 判定与结算分成两个系统
//!
//! `check_cast_requests` 只读（`Query<&Attributes>`），`apply_cast` 只写
//! （`AttributesMut`）。分开放不只是"清晰"：Bevy 的 `AttributesMut` 内部是
//! `Query<&mut Attributes>`，与同一个系统里的只读 `Query<&Attributes>` **会撞 B0001**。
//!
//! ## 需求组件挂在哪、按谁的属性算
//!
//! [`AttributeRequirements`] 挂在**技能**上（它是内容：这一招需要什么），
//! 但 `met()` 收的是**施法者的** `Attributes`（"我需要什么" ⇒ "你有没有"）。
//! gauge 的默认用法是同实体自评，这里是**跨实体**用法——`met` 的签名允许，
//! 语义也对，但值得写下来免得下次看到困惑。

use bevy::prelude::*;
use bevy_diesel::events::StartInvoke;
use bevy_diesel::invoker::{InvokedBy, resolve_invoker};
use bevy_diesel::target::InvokerTarget;
use bevy_gauge::prelude::{Attributes, AttributesMut, InstantExt, InstantModifierSet};
use bevy_gauge::requirements::AttributeRequirements;

use crate::tags::SkillTagMask;

/// 一次释放请求（**Message**）：想用某个技能。
///
/// L2 的输入 / AI 都发它，而不是直接写 `StartInvoke`——否则绕过门控与扣费。
#[derive(Message, Clone, Copy, Debug)]
pub struct CastRequest {
    /// 谁想放。
    pub caster: Entity,
    /// 放哪个技能（技能根实体，由构造器搭出来的那个）。
    pub ability: Entity,
}

/// 放出来了（内部消息）：门控通过，准备扣费 + 进状态机。
#[derive(Message, Clone, Copy, Debug)]
pub struct CastAccepted {
    /// 谁放。
    pub caster: Entity,
    /// 哪个技能。
    pub ability: Entity,
}

/// 被拒（**Message**）：给 UI 反馈用（灰按钮 / 提示音）。
#[derive(Message, Clone, Copy, Debug)]
pub struct CastRejected {
    /// 谁被拒。
    pub caster: Entity,
    /// 哪个技能。
    pub ability: Entity,
    /// 为什么（人话，可直接显示）。
    pub reason: &'static str,
}

/// 技能的费用（组件，挂在技能根上，由内容构造）。
///
/// `Vec<(属性名, 数量)>` 而不是固定结构：费用种类是内容决定的（旧内容里有
/// `action` 与 `reaction` 两种池）。
#[derive(Component, Clone, Debug, Default, PartialEq)]
pub struct CastCosts(pub Vec<(String, f32)>);

/// **技能类别**（组件，挂在技能根上，由内容构造）。
///
/// 硬控按它封技能：宿主身上的 [`BlockedTags`] 与它求交，非空就放不出来。
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SkillTags(pub SkillTagMask);

/// **硬控**（组件，挂在状态的生效态上，由内容构造）：封掉哪几类技能。
///
/// `on_caster` 决定它封的是谁：
///
/// | `who` | 谁被控 | 例子 |
/// |---|---|---|
/// | `Caster` | 施法者自己 | 需要专注的架势（自己不能动） |
/// | `Target` | 施法者的目标 | 眩晕 / 沉默（对手放不出招） |
///
/// ⚠️ 这个组件**不直接在门控里查**：门控只看宿主身上的 [`BlockedTags`]。
/// 理由是"谁被控"要沿 `InvokerTarget` 解析，而那是**会变**的（怪物重新选目标之后，
/// 老状态就会指到别人身上）。所以由 [`derive_blocked_tags`] 每帧把它**落到宿主身上**，
/// 门控只做一次位与——见那个系统的文档。
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BlocksTags {
    /// 封掉哪些类别。
    pub tags: SkillTagMask,
    /// `true` = 封施法者自己；`false` = 封施法者的目标。
    pub on_caster: bool,
}

/// **宿主身上被封掉的技能类别**（派生组件，每帧由 [`derive_blocked_tags`] 整批重建）。
///
/// 挂在**有属性的实体**（角色）上。UI 也读它："为什么这个按钮点不动"有答案了。
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BlockedTags(pub SkillTagMask);

/// 把生效中的 [`BlocksTags`] 汇总到**宿主**身上。
///
/// 为什么要有这一步（而不是门控里现场反查）：
///
/// ```text
/// 状态 → 发起者（`InvokedBy` 上溯）
///      → 宿主：who=Caster 就是发起者本人；who=Target 是**发起者当时瞄的人**
/// ```
///
/// 最后那一步依赖 `InvokerTarget`，而怪物会**重新选目标**——于是"三秒前挂的眩晕"
/// 会在门控现场被解析到别人身上。**在状态生效时落一次盘**就没有这个问题。
///
/// 整批重建（而不是增量加减）的理由与 gauge 的修饰符一样：**状态到期后自动消失，
/// 不需要清理消息**——少一条"忘记清理"的失败路径。
pub fn derive_blocked_tags(
    mut commands: Commands,
    hosts: Query<Entity, With<Attributes>>,
    statuses: Query<(&BlocksTags, &InvokedBy), With<bevy_gearbox::Active>>,
    invokers: Query<&InvokedBy>,
    invoker_targets: Query<&InvokerTarget<IVec2>>,
    mut previous: Local<Vec<Entity>>,
) {
    // 先把"谁被封了什么"算出来（不写世界，避免借用冲突）。
    let mut masks: Vec<(Entity, SkillTagMask)> = Vec::new();
    for (blocks, invoked) in &statuses {
        if blocks.tags.is_empty() {
            continue;
        }
        let source = invokers.root_ancestor(invoked.0);
        let host = if blocks.on_caster {
            Some(source)
        } else {
            invoker_targets.get(source).ok().and_then(|aim| aim.entity)
        };
        let Some(host) = host else {
            continue;
        };
        if !hosts.contains(host) {
            continue;
        }
        match masks.iter_mut().find(|(entity, _)| *entity == host) {
            Some((_, mask)) => mask.0 |= blocks.tags.0,
            None => masks.push((host, blocks.tags)),
        }
    }

    // 上一帧被封、这一帧不再被封的宿主：摘掉组件。
    for host in previous.iter() {
        if !masks.iter().any(|(entity, _)| entity == host) {
            commands.entity(*host).remove::<BlockedTags>();
        }
    }
    for (host, mask) in &masks {
        commands.entity(*host).insert(BlockedTags(*mask));
    }
    *previous = masks.into_iter().map(|(host, _)| host).collect();
}

/// **要求施法者此刻空闲**（组件，挂在技能根上，由内容构造）。
///
/// 带 `bool` 的理由与别处一致：内容决定要不要生效，而 BSN 里条件插组件会让场景类型分叉。
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RequiresIdle(pub bool);

impl RequiresIdle {
    /// 生效吗。
    pub const fn yes(self) -> bool {
        self.0
    }
}

/// 门控：需求满足 + 没被硬控 + 付得起 → `CastAccepted`，否则 `CastRejected`。
pub fn check_cast_requests(
    mut requests: MessageReader<CastRequest>,
    abilities: Query<(
        &CastCosts,
        Option<&AttributeRequirements>,
        Option<&SkillTags>,
        Option<&RequiresIdle>,
    )>,
    attributes: Query<&Attributes>,
    blocked: Query<&BlockedTags>,
    // "谁正在释放"：`#Invoking` 上挂着 `counter::Invoking` 标记。
    invoking: Query<(&InvokedBy, &bevy_gearbox::Active), With<crate::counter::Invoking>>,
    invokers: Query<&InvokedBy>,
    // 技能自己的状态机在第几段：`#Ready` 才接受新的释放。
    states: Query<
        (
            Entity,
            Has<crate::ability::Cooling>,
            Has<crate::counter::Invoking>,
        ),
        With<bevy_gearbox::Active>,
    >,
    substates: Query<&bevy_gearbox::SubstateOf>,
    mut accepted: MessageWriter<CastAccepted>,
    mut rejected: MessageWriter<CastRejected>,
) {
    // 先把"还没就绪"的技能根收进一个集合（状态标记挂在状态实体上，要沿 `SubstateOf` 回根）。
    let mut not_ready: std::collections::HashSet<Entity> = std::collections::HashSet::new();
    for (state, cooling, invoking) in &states {
        if cooling || invoking {
            not_ready.insert(bevy_diesel::invoker::resolve_root(&substates, state));
        }
    }

    for request in requests.read() {
        let (costs, requirements, tags, requires_idle) = match abilities.get(request.ability) {
            Ok(found) => found,
            Err(_) => {
                rejected.write(CastRejected {
                    caster: request.caster,
                    ability: request.ability,
                    reason: "技能实体不存在",
                });
                continue;
            }
        };

        // 需求：表达式在**组件挂上时**就编译好了（gauge 的组件钩子），这里只求值。
        let caster_attributes = match attributes.get(request.caster) {
            Ok(attributes) => attributes,
            Err(_) => {
                rejected.write(CastRejected {
                    caster: request.caster,
                    ability: request.ability,
                    reason: "施法者没有属性（gauge 的 Attributes 没铺上）",
                });
                continue;
            }
        };
        if let Some(requirements) = requirements
            && !requirements.met(caster_attributes)
        {
            rejected.write(CastRejected {
                caster: request.caster,
                ability: request.ability,
                reason: "需求不满足",
            });
            continue;
        }

        // 技能自己就绪了吗。
        //
        // ⚠️ 少了这条会**白扣费**：第二次请求通过门控、扣了行动力，而 `StartInvoke`
        // 因为没有 `#Ready → #Invoking` 这条边被 gearbox **静默忽略**（什么都不发生）。
        // 冷却与"正在释放"都算没就绪（标记见 `crate::ability`）。
        if not_ready.contains(&request.ability) {
            rejected.write(CastRejected {
                caster: request.caster,
                ability: request.ability,
                reason: "技能还没就绪（冷却中或正在释放）",
            });
            continue;
        }

        // 状态类需求（旧 `NoActiveAction`）：它不是属性表达式，所以单独判一次。
        // "我正忙"= 我有**另一个**技能停在 `#Invoking`（那个标记在 `counter` 里定义）。
        if requires_idle.is_some_and(|marker| marker.yes()) {
            let mut busy = false;
            for (invoked, _) in &invoking {
                if resolve_invoker(&invokers, invoked.0) == request.caster {
                    busy = true;
                    break;
                }
            }
            if busy {
                rejected.write(CastRejected {
                    caster: request.caster,
                    ability: request.ability,
                    reason: "动作还没结束（这一招要求空闲）",
                });
                continue;
            }
        }

        // 硬控：宿主身上封掉的类别与这一招的类别有交集就放不出来。
        // 这是**位与**一条——"谁被封了"早就由 `derive_blocked_tags` 落到宿主身上了。
        if let Some(blocked) = blocked.get(request.caster).ok()
            && let Some(tags) = tags
            && blocked.0.intersects(tags.0)
        {
            rejected.write(CastRejected {
                caster: request.caster,
                ability: request.ability,
                reason: "被控制：这一类技能被封",
            });
            continue;
        }

        // 付得起吗（不预扣；真扣在 `apply_cast`，那一步是唯一写属性的地方）。
        if let Some((name, _)) = costs
            .0
            .iter()
            .find(|(name, amount)| caster_attributes.value(name) < *amount)
        {
            rejected.write(CastRejected {
                caster: request.caster,
                ability: request.ability,
                reason: if name == "Action" {
                    "行动点不够"
                } else {
                    "资源不够"
                },
            });
            continue;
        }

        accepted.write(CastAccepted {
            caster: request.caster,
            ability: request.ability,
        });
    }
}

/// 结算：扣费 + 触发状态机。
///
/// **只写属性**：没有只读 `Query<&Attributes>`（会与 `AttributesMut` 撞 B0001），
/// 所以它完全不重新判定——判定已经在 `check_cast_requests` 做完了。
pub fn apply_cast(
    mut accepted: MessageReader<CastAccepted>,
    abilities: Query<&CastCosts>,
    mut attributes: AttributesMut,
    mut starts: MessageWriter<StartInvoke<IVec2>>,
) {
    for cast in accepted.read() {
        if let Ok(costs) = abilities.get(cast.ability)
            && !costs.0.is_empty()
        {
            let mut instant = InstantModifierSet::new();
            for (name, amount) in &costs.0 {
                instant.push_sub(name, *amount);
            }
            attributes.apply_instant(&instant, &[], cast.caster);
        }

        starts.write(StartInvoke::<IVec2> {
            entity: cast.ability,
            target: Default::default(),
        });
    }
}

/// 注册释放门控：三条消息 + 两个系统（顺序是契约：判定 → 结算）。
pub struct CastingPlugin;

impl Plugin for CastingPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<CastRequest>()
            .add_message::<CastAccepted>()
            .add_message::<CastRejected>()
            // ⚠️ 派生放在**帧末**（`PostUpdate`），门控留在 `Update`：
            // 状态的生效/失效是 gearbox 在 `Update` 里改的，而 `Update` 内部的先后没保证；
            // 派生若排在 gearbox 之前就会晚一帧才反映（曾让"打晕后立刻查 `BlockedTags`"随机为空）。
            // 代价是门控读到的是**上一帧**的派生态——对硬控来说一个帧的延迟无感，
            // 换来的是与系统顺序无关的确定性。
            .add_systems(PostUpdate, derive_blocked_tags)
            .add_systems(Update, (check_cast_requests, apply_cast).chain());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn costs_are_a_plain_list_of_attribute_amounts() {
        // 费用是内容决定的（旧内容里有 `action` 与 `reaction` 两种），所以是列表不是结构体。
        let costs = CastCosts(vec![("Action".to_string(), 1.0)]);
        assert_eq!(costs.0.len(), 1);
        assert_eq!(CastCosts::default().0.len(), 0, "默认不收费");
    }
}
