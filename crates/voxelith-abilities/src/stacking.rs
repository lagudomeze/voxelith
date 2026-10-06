//! **叠层规则**：同一个状态第二次挂到同一个人身上时怎么办（旧 `stacking`）。
//!
//! ## 为什么这件事在新栈里必须显式做
//!
//! diesel 的 `spawn_system` 只会**无条件地把状态实例生成出来**——它不知道"这个人身上
//! 已经有同一种状态了"。旧的单机引擎把这条规则写死在 `apply_status` 里（它直接改
//! `ActiveStatus.stacks` 字段）；新栈里状态是**独立的实体树**，所以规则要自己实现：
//!
//! ```text
//! 新实例出现（Added<Stacking>）
//!   ├─ 找出"同一个 id + 同一个宿主"的旧实例
//!   └─ 按规则处理：Refresh/Replace 留下新的；Ignore 丢新的；Stack(n) 超过 n 丢最旧的
//! ```
//!
//! ## 四条规则（沿用旧内容的名字）
//!
//! | 规则 | 语义 | 典型用法 |
//! |---|---|---|
//! | `Refresh`（默认） | 只保留最新那一个（时长与效果都刷新） | 格挡、眩晕 |
//! | `Replace` | 同上；**当有"按层数缩放"的效果时**它才和 `Refresh` 不同 | 更强的同类减益 |
//! | `Ignore` | 已经有就不重挂（时长也不刷新） | 一次性减益 |
//! | `Stack(n)` | 最多同时 n 个（超了丢最旧的） | 可叠的中毒 / 灼烧 |
//!
//! ## 宿主怎么认
//!
//! 与硬控同一条路：`who=Caster` 的宿主是发起者，`who=Target` 的宿主是**发起者当时瞄的人**。
//! 所以判定发生在"新实例刚出现"的那一刻（那时 `InvokerTarget` 还是对的），
//! 而不是以后每帧重解——怪物重新选目标不该让老状态改挂到别人身上。
//!
//! ## 还没做的
//!
//! **按层数缩放效果**（旧 `Value::Mul(Stacks, ...)`）。现在的 `Stack(n)` 只保证
//! "同时最多 n 个实例"，每个实例各算各的效果。要做"3 层中毒每跳 ×3"，
//! 需要让效果表达式的角色上下文里带一个"当前层数"——那属于数值层的活。

use bevy::prelude::*;
use bevy_diesel::invoker::{InvokedBy, resolve_invoker};
use bevy_diesel::target::InvokerTarget;
use bevy_gearbox::{AcceptAll, GearboxMessage, RegistrationAppExt};

/// **让一个状态实例退场**（`#Active` → `#Expired`）。
///
/// 叠层对账**不销毁**被顶掉的实例，而是发这条消息让它走自己的结局路径。理由是踩出来的：
///
/// > diesel 回收持续修饰符要读**被销毁实体上的 `AttributeModifiers`**。
/// > 直接 `despawn` 的话，实体在回收系统看到它之前就没了，于是修饰符**静默留在身上**——
/// > 现场表现是"刷新格挡一次，护甲 +5 变成了 +10"。
///
/// 走 `#Expired` 则一切按既有收场逻辑办：离开 `#Active` ⇒ 修饰符被精确卸下；
/// `GoOffConfig::root()` + `DespawnEffect` ⇒ 实例自己销毁。
#[derive(Message, Reflect, Clone, Copy, Debug)]
pub struct SupersedeStatus {
    /// 哪个状态实例（状态机根实体）。
    pub target: Entity,
}

impl GearboxMessage for SupersedeStatus {
    type Validator = AcceptAll;
    fn target(&self) -> Entity {
        self.target
    }
}

/// 状态实例的**身份**（内容里的 id）：叠层判定按它认亲。
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct StatusIdentity(pub String);

/// 这个状态的宿主是谁（内容里的 `who`）：`true` = 发起者自己，`false` = 发起者瞄的人。
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusHostRule {
    /// 挂在施法者自己身上（增益）。
    Caster,
    /// 挂在施法者的目标身上（减益）。
    Target,
}

/// 叠层规则（内容里的 `stacking`）。
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Stacking {
    /// 只保留最新那一个（时长与效果都刷新）。**默认**。
    #[default]
    Refresh,
    /// 用新的顶掉旧的。
    Replace,
    /// 已经有就不重挂。
    Ignore,
    /// 最多同时 `n` 个，超了丢最旧的。
    Stack(u32),
}

/// **标记**：这个状态实例已经走完（进了终态：`#Expired` 或 `#Removed`）。
///
/// 构造器把它挂在两个终态上，**取代 diesel 的 `DespawnEffect`**——
/// 因为"立刻销毁"正是下面这条坑的来源。
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct StatusExpired;

/// **标记**：这个实例下帧销毁（已经留过一帧给它卸修饰符）。
#[derive(Component, Clone, Copy, Debug, Default)]
struct PendingReap;

/// 销毁上一帧标记过的实例。
///
/// 必须在**下一帧**做，而且要在 `Update`（diesel 在这里卸修饰符）之后——
/// 所以它与 [`mark_expired_statuses`] 一起放在 `PostUpdate`，且本系统排在前面。
fn despawn_marked_statuses(mut commands: Commands, marked: Query<Entity, With<PendingReap>>) {
    for entity in &marked {
        commands.entity(entity).try_despawn();
    }
}

/// 把刚进入 `#Expired` 的实例标记为"下帧销毁"。
///
/// ## 为什么不能当场销毁（踩出来的）
///
/// > diesel 卸持续修饰符要读**被卸实体上的 `AttributeModifiers`**。
/// > 状态一进 `#Expired` 就被 `DespawnEffect` 销毁的话，卸除系统看到的实体已经没了，
/// > 于是修饰符**静默留在宿主身上**。
///
/// 现场表现：刷新一次格挡，护甲从 +5 变成 **+10**（诊断输出：实例只剩 1 个、生效态也换了，
/// 但护甲是 13 = 3 + 5 + 5）。
///
/// 所以收场分两步：**这一帧离开生效态（修饰符开始卸），下一帧才销毁实体**。
fn mark_expired_statuses(
    mut commands: Commands,
    substates: Query<&bevy_gearbox::SubstateOf>,
    // ⚠️ `With<Active>` 不能少：`#Expired` 那个**实体**从场景生成时就在，
    // 少了这个过滤，每个状态实例一挂上就会被判为"已经走完"并立刻销毁。
    expired: Query<Entity, (With<StatusExpired>, With<bevy_gearbox::Active>)>,
    already_marked: Query<(), With<PendingReap>>,
) {
    for state in &expired {
        let root = bevy_diesel::invoker::resolve_root(&substates, state);
        if already_marked.contains(root) {
            continue;
        }
        commands.entity(root).insert(PendingReap);
    }
}

/// **解析一个状态实例挂在谁身上**（叠层 / 净化 / HUD 共用这一处）。
///
/// ```text
/// who=Caster ⇒ 宿主是发起者本人
/// who=Target ⇒ 宿主是**发起者当时瞄的人**（`InvokerTarget`）
/// ```
///
/// ⚠️ 它依赖 `InvokerTarget`，而那个值是会变的（怪物重新选目标）。所以调用它的时机很关键：
/// **状态刚挂上 / 刚被看见的那一刻**用它是对的；过了很久再解就可能指到别人身上。
pub fn resolve_status_host(
    host_rule: &StatusHostRule,
    invoked: &InvokedBy,
    invokers: &Query<&InvokedBy>,
    invoker_targets: &Query<&InvokerTarget<IVec2>>,
) -> Option<Entity> {
    let source = resolve_invoker(invokers, invoked.0);
    match host_rule {
        StatusHostRule::Caster => Some(source),
        StatusHostRule::Target => invoker_targets.get(source).ok().and_then(|aim| aim.entity),
    }
}

/// 执行叠层规则：新实例出现时，按内容写的规则处理同 id 同宿主的旧实例。
///
/// **只在实例刚出现那一刻判定一次**（`Added<Stacking>`）：宿主靠 `InvokerTarget` 解析，
/// 而那个值是会变的。
pub fn enforce_stacking_rules(
    instances: Query<(
        Entity,
        &StatusIdentity,
        &StatusHostRule,
        &InvokedBy,
        &Stacking,
    )>,
    invokers: Query<&InvokedBy>,
    invoker_targets: Query<&InvokerTarget<IVec2>>,
    newly_added: Query<Entity, Added<Stacking>>,
    mut supersede: MessageWriter<SupersedeStatus>,
) {
    // 把所有实例解析成"身份 + 宿主 + 规则"三元组，后面只在这个快照上判定，
    // 免得一边读查询一边改世界。
    let resolved: Vec<(Entity, &str, Option<Entity>, Stacking)> = instances
        .iter()
        .map(|(entity, identity, host_rule, invoked, stacking)| {
            let host = resolve_status_host(host_rule, invoked, &invokers, &invoker_targets);
            (entity, identity.0.as_str(), host, *stacking)
        })
        .collect();

    for (entity, identity, host, stacking) in &resolved {
        if !newly_added.contains(*entity) {
            continue;
        }
        // 同 id + 同宿主的旧实例（按实体序 = 生成序，最小的最旧）。
        let mut older: Vec<Entity> = resolved
            .iter()
            .filter(|(other, other_identity, other_host, _)| {
                other != entity && *other_identity == *identity && *other_host == *host
            })
            .map(|(other, _, _, _)| *other)
            .collect();
        older.sort_by_key(|other| other.index());

        match stacking {
            Stacking::Ignore => {
                if !older.is_empty() {
                    // 已经有同种状态：让**新的这条**退场（旧的时长也不刷新）。
                    supersede.write(SupersedeStatus { target: *entity });
                }
            }
            Stacking::Refresh | Stacking::Replace => {
                // 留下最新的：让旧的退场（它会卸掉自己的修饰符再销毁）。
                for old in older {
                    supersede.write(SupersedeStatus { target: old });
                }
            }
            Stacking::Stack(max) => {
                let max = (*max).max(1) as usize;
                // 算上自己，最多留 `max` 个：超出部分从最旧的开始退场。
                let mut alive = older.len() + 1;
                for old in older {
                    if alive <= max {
                        break;
                    }
                    supersede.write(SupersedeStatus { target: old });
                    alive -= 1;
                }
            }
        }
    }
}

/// 注册叠层规则（放在**帧末**：与威胁窗口、硬控派生同一条原则）。
pub struct StackingPlugin;

impl Plugin for StackingPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<SupersedeStatus>()
            // 它是 gearbox 的转移消息（`#Active` → `#Expired`），必须注册。
            .register_transition::<SupersedeStatus>()
            .add_systems(
                PostUpdate,
                (
                    // 顺序是契约：先销毁**上一帧**标记的，再标记这一帧走完的，
                    // 最后才做叠层对账（它只管发 `SupersedeStatus`）。
                    despawn_marked_statuses,
                    mark_expired_statuses,
                    enforce_stacking_rules,
                )
                    .chain(),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_rule_is_refresh() {
        // 内容不写 `stacking` 时是"只保留最新"——旧内容里最常见的也是它。
        assert_eq!(Stacking::default(), Stacking::Refresh);
    }
}
