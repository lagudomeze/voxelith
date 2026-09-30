//! L1 伤害类型：**只负责标识 + 附加行为分派**（**R47**、**R48**、**R98**）。
//!
//! 三件"不做"的事：
//!
//! - 不算减伤 —— 那是 [`crate::behaviors::resistance`] 的确定性公式；
//! - 不做状态判定 —— 那是 [`crate::behaviors::status`] 的独立豁免框架；
//! - 不认识 `Health` —— 那是 L0 的纯数值执行器（`atoms::health`）。
//!
//! 附加行为通过**注册表 + 消息**分派，因此与抗性算法零耦合：内容层注册
//! `DamageBehaviorId` → 具体请求的映射，L1 只按标签筛选后写出。

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;

/// 伤害类型（**R48**：定义在 L1，不进 `health`）。
///
/// 它需要按变体开定宽数组（抵抗表按类型一列），所以这里保留 `COUNT` / `ALL` / `index()`
/// 三个辅助项；新增一种类型时**必须同时补上**（漏了会在下面几个 `match` 上编译期报错）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DamageType {
    /// 物理：走护甲与物理减伤。
    Physical,
    /// 火焰。
    Fire,
    /// 冰霜。
    Frost,
    /// 奥术。
    Arcane,
}

impl DamageType {
    /// 变体数量（抵抗表列宽）。
    pub const COUNT: usize = 4;

    /// 全部变体，顺序 = 定义顺序（`index()` 的下标来源）。
    pub const ALL: [Self; Self::COUNT] = [
        DamageType::Physical,
        DamageType::Fire,
        DamageType::Frost,
        DamageType::Arcane,
    ];

    /// 紧凑下标（按类型索引的数组用）。
    pub const fn index(self) -> usize {
        match self {
            DamageType::Physical => 0,
            DamageType::Fire => 1,
            DamageType::Frost => 2,
            DamageType::Arcane => 3,
        }
    }

    /// 稳定标识串（存档 / 配置 / 表现映射用）。
    pub const fn name(self) -> &'static str {
        match self {
            DamageType::Physical => "physical",
            DamageType::Fire => "fire",
            DamageType::Frost => "frost",
            DamageType::Arcane => "arcane",
        }
    }
}

/// 伤害标签：附加行为按标签筛选，避免每来一个需求就往请求里加字段。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct DamageTags(u16);

impl DamageTags {
    /// 空标签。
    pub const NONE: Self = Self(0);
    /// 持续伤害（DOT）。
    pub const DOT: Self = Self(1 << 0);
    /// 近战来源。
    pub const MELEE: Self = Self(1 << 1);
    /// 暴击（判定层写入）。
    pub const CRIT: Self = Self(1 << 2);

    /// 是否包含全部给定标签。
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// 追加标签。
    pub fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }
}

/// 伤害请求（**Message**）：判定层与管线的共同输入。
///
/// `base_amount` 是原始基础伤害（只读，供日志 / 表现）；`amount` 是**管线推进中的当前值**，
/// 阶段系统逐个改写它，最终由 [`crate::behaviors::damage_pipeline`] 定稿成
/// [`DamageResolvedMessage`]。
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub struct DamageRequest {
    /// 伤害来源。
    pub source: Entity,
    /// 伤害目标。
    pub target: Entity,
    /// 伤害类型。
    pub damage_type: DamageType,
    /// 原始基础伤害（只读）。
    pub base_amount: f32,
    /// 管线中的当前伤害值。
    pub amount: f32,
    /// 标签（附加行为按它筛选）。
    pub tags: DamageTags,
    /// 判定层写入：被规避。
    pub missed: bool,
    /// 判定层写入：暴击。
    pub crit: bool,
}

impl DamageRequest {
    /// 构造一次伤害请求（`amount` 先等于 `base_amount`，管线仍会从阶段一重新初始化）。
    pub fn new(source: Entity, target: Entity, damage_type: DamageType, base_amount: f32) -> Self {
        Self {
            source,
            target,
            damage_type,
            base_amount,
            amount: base_amount,
            tags: DamageTags::NONE,
            missed: false,
            crit: false,
        }
    }
}

/// 伤害已结算（**Message**）：管线的**唯一输出**。
///
/// 表现层（L2）与附加行为分派都监听它；被规避的伤害也会发一条 `amount = 0` 的记录，
/// 让表现能演"闪避"（见 `docs/OPEN-QUESTIONS.md` Q9）。
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct DamageResolvedMessage {
    /// 伤害来源。
    pub source: Entity,
    /// 伤害目标。
    pub target: Entity,
    /// 伤害类型。
    pub damage_type: DamageType,
    /// 最终伤害（已按 [`crate::atoms::modifiers::Rounding`] 取整）。
    pub amount: u32,
    /// 是否被规避。
    pub missed: bool,
    /// 是否暴击。
    pub crit: bool,
}

/// 附加行为标识：内容层注册的稳定 ID，L1 只做分派，不认识行为细节。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DamageBehaviorId(pub &'static str);

/// 一条附加行为绑定；`requires` 为空表示"该类型的所有伤害都触发"。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DamageBehaviorBinding {
    /// 行为 ID。
    pub behavior: DamageBehaviorId,
    /// 需要的标签（必须全部满足）。
    pub requires: DamageTags,
}

/// 附加行为注册表（**Resource**）：按伤害类型索引，列宽跟随 `DamageType::COUNT`。
#[derive(Resource, Debug, Clone, Default)]
pub struct DamageBehaviorRegistry {
    by_type: [Vec<DamageBehaviorBinding>; DamageType::COUNT],
}

impl DamageBehaviorRegistry {
    /// 某种伤害类型绑定的附加行为。
    pub fn behaviors(&self, damage_type: DamageType) -> &[DamageBehaviorBinding] {
        &self.by_type[damage_type.index()]
    }

    /// 用构建器组装（会做**覆盖检查**，见 [`DamageBehaviorRegistryBuilder::build`]）。
    pub fn builder() -> DamageBehaviorRegistryBuilder {
        DamageBehaviorRegistryBuilder::default()
    }
}

/// 漏定义：新增伤害类型却忘了配附加行为（或显式声明"无附加行为"）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, derive_more::Display, derive_more::Error)]
#[display("damage behavior missing for {damage_type:?}")]
pub struct MissingDamageBehavior {
    /// 漏定义的伤害类型。
    pub damage_type: DamageType,
}

/// 注册表构建器：**加载期兜底**——漏定义直接返回 `Err`，不会静默跑起来。
#[derive(Debug, Clone, Default)]
pub struct DamageBehaviorRegistryBuilder {
    by_type: [Vec<DamageBehaviorBinding>; DamageType::COUNT],
    declared_empty: [bool; DamageType::COUNT],
}

impl DamageBehaviorRegistryBuilder {
    /// 给某种伤害类型绑定一条附加行为。
    pub fn bind(
        mut self,
        damage_type: DamageType,
        behavior: DamageBehaviorId,
        requires: DamageTags,
    ) -> Self {
        self.by_type[damage_type.index()].push(DamageBehaviorBinding { behavior, requires });
        self
    }

    /// 显式声明"这种伤害类型没有附加行为"（否则 [`Self::build`] 会报漏定义）。
    pub fn declare_no_behaviors(mut self, damage_type: DamageType) -> Self {
        self.declared_empty[damage_type.index()] = true;
        self
    }

    /// 校验覆盖：每种伤害类型要么绑定了行为，要么被显式声明为空。
    pub fn build(self) -> Result<DamageBehaviorRegistry, MissingDamageBehavior> {
        for damage_type in DamageType::ALL {
            let index = damage_type.index();
            if self.by_type[index].is_empty() && !self.declared_empty[index] {
                return Err(MissingDamageBehavior { damage_type });
            }
        }
        Ok(DamageBehaviorRegistry {
            by_type: self.by_type,
        })
    }
}

/// 分派附加行为：**只发消息**，不算减伤、不判状态（**R48**）。
///
/// 当前完成"按标签筛选"这一步；`DamageBehaviorId` → 具体请求消息的映射表由**内容层**提供
/// （L1 不认识内容），接到表之后在这里逐条写出。
pub fn dispatch_damage_behaviors(
    mut resolved: MessageReader<DamageResolvedMessage>,
    registry: Res<DamageBehaviorRegistry>,
) {
    for message in resolved.read() {
        if message.missed {
            continue;
        }
        let tags = if message.crit {
            DamageTags::CRIT
        } else {
            DamageTags::NONE
        };
        for binding in registry.behaviors(message.damage_type) {
            if !tags.contains(binding.requires) {
                continue;
            }
            // TODO(M5)：按 `binding.behavior` 写出对应的请求消息（需要内容层的行为表）。
            let _ = binding;
        }
    }
}

/// 注册伤害类型域的消息与分派系统（**R34**：谁定义谁注册）。
pub struct DamagePlugin;

impl Plugin for DamagePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DamageBehaviorRegistry>()
            .add_message::<DamageRequest>()
            .add_message::<DamageResolvedMessage>()
            .add_systems(Update, dispatch_damage_behaviors);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_requires_every_damage_type_to_be_decided() {
        let incomplete = DamageBehaviorRegistry::builder()
            .declare_no_behaviors(DamageType::Physical)
            .build();
        assert_eq!(
            incomplete.unwrap_err(),
            MissingDamageBehavior {
                damage_type: DamageType::Fire
            },
            "第一个没被决定的类型应报错"
        );
    }

    #[test]
    fn fully_decided_registry_builds() {
        let mut builder = DamageBehaviorRegistry::builder();
        for damage_type in DamageType::ALL {
            builder = builder.declare_no_behaviors(damage_type);
        }
        builder = builder.bind(
            DamageType::Fire,
            DamageBehaviorId("burning"),
            DamageTags::DOT,
        );

        let registry = builder.build().expect("覆盖完整应构建成功");
        assert_eq!(registry.behaviors(DamageType::Fire).len(), 1);
        assert!(registry.behaviors(DamageType::Frost).is_empty());
    }

    #[test]
    fn tags_are_subsets() {
        let mut tags = DamageTags::DOT;
        tags.insert(DamageTags::CRIT);
        assert!(tags.contains(DamageTags::DOT));
        assert!(tags.contains(DamageTags::CRIT));
        assert!(!tags.contains(DamageTags::MELEE));
    }
}
