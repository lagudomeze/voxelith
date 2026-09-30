//! L1 状态：**独立于伤害的生命周期 + 统一概率豁免框架**。
//!
//! 四条边界（对应需求 6 与不变量 I4）：
//!
//! 1. **独立生命周期**：一状态一实体（[`StatusInstance`] + `ChildOf(宿主)`），
//!    到期 / 净化 / 宿主销毁各有明确路径。
//! 2. **不在伤害管线里**：伤害结算完（`DamageResolvedMessage`）之后，
//!    由附加行为分派或技能独立发起 [`ApplyStatusRequest`]。
//! 3. **统一判定框架**：物理 / 法术 / 精神只是"取哪对攻防属性"（[`ContestKind`]），
//!    概率与时长共用 [`contest_chance`] / [`contest_duration`]，参数来自 [`ContestParams`]。
//! 4. **不占修饰符列表**：状态本身不是数值修正；它的数值效果由内容层在 `on_apply` / `on_tick`
//!    行为里**派生** `AddStatModifierMessage`（状态 → 修饰符，而不是状态 = 修饰符）。
//!
//! 文件划分：本文件放**标识 / 定义 / 注册表 / 组件**，[`contest`] 放判定公式，
//! [`systems`] 放生命周期系统与消息。

mod contest;
mod systems;

pub use contest::{ContestKind, ContestParams, contest_chance, contest_duration};
pub use systems::{
    ApplyStatusRequest, PurgeStatusMessage, StatusExpiredEvent, StatusRejectReason,
    StatusResolvedMessage, StatusTickMessage, purge_statuses, resolve_status_applications,
    tick_status_timers,
};

use core::time::Duration;

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;

/// 状态标识。
///
/// 它按变体开定宽数组（[`StatusImmunity`] 的位图、[`StatusRegistry`] 的列宽），
/// 所以保留 `COUNT` / `ALL` / `index()`；新增一种状态时必须一起补（漏了在 `match` 上编译期报错）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StatusId {
    /// 灼烧：每回合造成火焰伤害。
    Burning,
    /// 冰冻：降低行动能力。
    Frozen,
    /// 眩晕：无法行动。
    Stunned,
}

impl StatusId {
    /// 变体数量（位图 / 注册表列宽）。
    pub const COUNT: usize = 3;

    /// 全部变体，顺序 = 定义顺序。
    pub const ALL: [Self; Self::COUNT] = [StatusId::Burning, StatusId::Frozen, StatusId::Stunned];

    /// 紧凑下标。
    pub const fn index(self) -> usize {
        match self {
            StatusId::Burning => 0,
            StatusId::Frozen => 1,
            StatusId::Stunned => 2,
        }
    }

    /// 稳定标识串（存档 / 配置 / 日志用）。
    pub const fn name(self) -> &'static str {
        match self {
            StatusId::Burning => "burning",
            StatusId::Frozen => "frozen",
            StatusId::Stunned => "stunned",
        }
    }
}

/// 叠加规则。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Stacking {
    /// 重复施加刷新剩余时间，层数不变（默认）。
    #[default]
    Refresh,
    /// 重复施加叠层（上限 [`StatusDef::max_stacks`]）。
    Stack,
    /// 只能存在一份，重复施加被拒绝。
    Unique,
}

/// 行为标识：内容层注册的稳定 ID（L1 不认识行为细节）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BehaviorId(pub &'static str);

/// 状态在三个时机触发的行为（都可缺省）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StatusBehaviors {
    /// 施加成功时。
    pub on_apply: Option<BehaviorId>,
    /// 每个结算周期（回合）时。
    pub on_tick: Option<BehaviorId>,
    /// 到期 / 被净化时。
    pub on_expire: Option<BehaviorId>,
}

/// 状态定义（内容层数据）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StatusDef {
    /// 状态标识。
    pub id: StatusId,
    /// 判定类型（物理 / 法术 / 精神），只决定"取哪对攻防强度"。
    pub contest: ContestKind,
    /// 基础持续时间。
    pub base_duration: Duration,
    /// 最大层数（`Stacking::Stack` 时生效）。
    pub max_stacks: u8,
    /// 叠加规则。
    pub stacking: Stacking,
    /// 三个时机的行为。
    pub behaviors: StatusBehaviors,
}

/// 漏定义：新增 `StatusId` 却忘了写 [`StatusDef`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq, derive_more::Display, derive_more::Error)]
#[display("status definition missing for {status:?}")]
pub struct MissingStatusDefinition {
    /// 漏定义的状态。
    pub status: StatusId,
}

/// 状态定义注册表（**Resource**）：列宽跟随 `StatusId::COUNT`。
#[derive(Resource, Debug, Clone)]
pub struct StatusRegistry {
    defs: [Option<StatusDef>; StatusId::COUNT],
}

impl Default for StatusRegistry {
    /// 空注册表：所有状态都"未定义"（判定时会以 `Undefined` 拒绝，而不是静默生效）。
    fn default() -> Self {
        Self {
            defs: core::array::from_fn(|_| None),
        }
    }
}

impl StatusRegistry {
    /// 用构建器组装（会做覆盖检查）。
    pub fn builder() -> StatusRegistryBuilder {
        StatusRegistryBuilder::default()
    }

    /// 查定义。
    pub fn get(&self, status: StatusId) -> Option<&StatusDef> {
        self.defs[status.index()].as_ref()
    }

    /// 是否已定义。
    pub fn is_defined(&self, status: StatusId) -> bool {
        self.defs[status.index()].is_some()
    }

    /// 已定义的数量。
    pub fn defined_count(&self) -> usize {
        self.defs.iter().filter(|def| def.is_some()).count()
    }
}

/// 注册表构建器。
#[derive(Debug, Clone, Default)]
pub struct StatusRegistryBuilder {
    defs: [Option<StatusDef>; StatusId::COUNT],
}

impl StatusRegistryBuilder {
    /// 定义一种状态（重复定义以最后一次为准）。
    pub fn define(mut self, def: StatusDef) -> Self {
        self.defs[def.id.index()] = Some(def);
        self
    }

    /// 校验覆盖：每个 `StatusId` 都必须有定义。
    pub fn build(self) -> Result<StatusRegistry, MissingStatusDefinition> {
        for status in StatusId::ALL {
            if self.defs[status.index()].is_none() {
                return Err(MissingStatusDefinition { status });
            }
        }
        Ok(StatusRegistry { defs: self.defs })
    }
}

/// 目标身上的一个状态实例（组件，挂在**独立子实体**上）。
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct StatusInstance {
    /// 宿主实体。
    pub host: Entity,
    /// 施加者。
    pub source: Entity,
    /// 是哪种状态。
    pub status: StatusId,
    /// 当前层数。
    pub stacks: u8,
    /// 剩余时间。
    pub remaining: Duration,
    /// 距离下一个结算周期的累计时间。
    pub tick_accumulator: Duration,
}

impl StatusInstance {
    /// 新建一个实例（层数 1，从零开始累计结算周期）。
    pub fn new(host: Entity, source: Entity, status: StatusId, duration: Duration) -> Self {
        Self {
            host,
            source,
            status,
            stacks: 1,
            remaining: duration,
            tick_accumulator: Duration::ZERO,
        }
    }
}

/// 免疫表（组件）：逐状态一个开关。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusImmunity([bool; StatusId::COUNT]);

impl Default for StatusImmunity {
    fn default() -> Self {
        Self([false; StatusId::COUNT])
    }
}

impl StatusImmunity {
    /// 是否免疫该状态。
    pub fn is_immune(&self, status: StatusId) -> bool {
        self.0[status.index()]
    }

    /// 设置免疫。
    pub fn set_immunity(&mut self, status: StatusId, immune: bool) {
        self.0[status.index()] = immune;
    }
}

/// 状态结算配置（**Resource**：内容层 / 文件注入）。
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct StatusConfig {
    /// 每回合结算的时间步长（`Time` 驱动的"回合"）。
    pub tick_interval: Duration,
    /// 单次施加的时长上限（防止无限控）。
    pub max_duration: Duration,
}

impl Default for StatusConfig {
    fn default() -> Self {
        Self {
            tick_interval: Duration::from_secs(1),
            max_duration: Duration::from_secs(60),
        }
    }
}

/// 注册状态域：注册表 / 判定参数 / 结算配置 + 消息 + 生命周期系统。
///
/// 注册表默认是**空**的（所有状态视为未定义，判定会以 `Undefined` 拒绝）；
/// 内容层用 [`StatusRegistry::builder`] 组装后 `insert_resource` 覆盖。
pub struct StatusPlugin;

impl Plugin for StatusPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<StatusRegistry>()
            .init_resource::<ContestParams>()
            .init_resource::<StatusConfig>()
            .add_message::<ApplyStatusRequest>()
            .add_message::<StatusResolvedMessage>()
            .add_message::<PurgeStatusMessage>()
            .add_message::<StatusTickMessage>()
            .add_systems(
                Update,
                (
                    resolve_status_applications,
                    purge_statuses,
                    tick_status_timers,
                ),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(id: StatusId) -> StatusDef {
        StatusDef {
            id,
            contest: ContestKind::Spell,
            base_duration: Duration::from_secs(5),
            max_stacks: 3,
            stacking: Stacking::Stack,
            behaviors: StatusBehaviors::default(),
        }
    }

    #[test]
    fn registry_requires_every_status_to_be_defined() {
        let incomplete = StatusRegistry::builder()
            .define(def(StatusId::Burning))
            .build();
        assert_eq!(
            incomplete.unwrap_err(),
            MissingStatusDefinition {
                status: StatusId::Frozen
            }
        );
    }

    #[test]
    fn complete_registry_builds_and_looks_up() {
        let mut builder = StatusRegistry::builder();
        for status in StatusId::ALL {
            builder = builder.define(def(status));
        }
        let registry = builder.build().expect("覆盖完整应构建成功");
        assert_eq!(registry.defined_count(), StatusId::COUNT);
        assert_eq!(registry.get(StatusId::Stunned).unwrap().max_stacks, 3);
    }

    #[test]
    fn empty_registry_treats_everything_as_undefined() {
        let registry = StatusRegistry::default();
        assert!(!registry.is_defined(StatusId::Burning));
    }

    #[test]
    fn immunity_bitmap_follows_the_status_definition() {
        let mut immunity = StatusImmunity::default();
        assert!(!immunity.is_immune(StatusId::Frozen));
        immunity.set_immunity(StatusId::Frozen, true);
        assert!(immunity.is_immune(StatusId::Frozen));
        assert!(!immunity.is_immune(StatusId::Burning), "互不影响");
    }

    #[test]
    fn status_ids_have_stable_names_and_indices() {
        for id in StatusId::ALL {
            assert_eq!(StatusId::ALL[id.index()], id);
        }
        assert_eq!(StatusId::Burning.name(), "burning");
    }
}
