//! L1 伤害管线：**固定四阶段、确定性、输出唯一**（**R50**、**R54**）。
//!
//! ```text
//! [判定层 rolls] → base → attacker_bonus → defender_mitigation → finalize → DamageResolvedMessage
//! ```
//!
//! - **顺序固定，不允许插队**：四个阶段用 [`DamageStage`] 串成一条链，
//!   扩展方式是"往阶段里加规则"，不是插新阶段。
//! - **确定性**：这里没有随机（`missed` / `crit` 已由判定层写好）。
//! - **唯一输出**：[`DamageResolvedMessage`]（含最终数值与伤害类型）；状态判定在那之后独立发起。
//! - **不吃修饰符**：阶段只读"最终值视图"（`Stat` 的 `cached`、`Resistance` 的 `Deref` 视图）。

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;

use crate::atoms::health::ModifyHealthMessage;
use crate::atoms::modifiers::Rounding;
use crate::behaviors::damage::{DamageRequest, DamageResolvedMessage};
use crate::behaviors::resistance::{Resistance, ResistanceCaps, mitigate};

/// 管线阶段（**固定顺序**；判定层不在其中，它排在 `Base` 之前）。
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DamageStage {
    /// 阶段一：基础伤害（技能 / 武器数据 → 数值）。
    Base,
    /// 阶段二：攻击方加成（属性缩放、增伤、暴击倍率）。
    AttackerBonus,
    /// 阶段三：防御方减伤（读抵抗视图，纯确定）。
    DefenderMitigation,
    /// 阶段四：上限与最终值（取整、保底、定稿）。
    Finalize,
}

/// 管线配置（**Resource**：内容层 / 文件注入）。
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct PipelineConfig {
    /// 最终伤害的取整口径。
    pub rounding: Rounding,
    /// 暴击倍率（判定层打上 `crit` 后在这里生效）。
    pub crit_multiplier: f32,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            rounding: Rounding::Floor,
            crit_multiplier: 2.0,
        }
    }
}

/// 阶段一：基础伤害。
///
/// 当前把 `amount` 重置为 `base_amount`（保证重入安全）；技能等级 / 武器基伤的解析
/// 由内容层在发请求前填好 `base_amount`。
pub fn stage_base(mut requests: MessageMutator<DamageRequest>) {
    for request in requests.read() {
        request.amount = request.base_amount;
    }
}

/// 阶段二：攻击方加成。
///
/// 当前只处理暴击倍率；属性缩放 / 增伤规则表由内容层提供后在这里按**固定顺序**逐条应用。
pub fn stage_attacker_bonus(
    mut requests: MessageMutator<DamageRequest>,
    config: Res<PipelineConfig>,
) {
    for request in requests.read() {
        if request.missed {
            continue;
        }
        if request.crit {
            request.amount *= config.crit_multiplier;
        }
        // TODO(M5)：读攻击方 `Stat` 最终值视图 + 增伤规则表（规则顺序 = 注册顺序，可测）。
    }
}

/// 阶段三：防御方减伤（读抵抗最终值视图 + 上限）。
pub fn stage_defender_mitigation(
    mut requests: MessageMutator<DamageRequest>,
    targets: Query<&Resistance>,
    caps: Res<ResistanceCaps>,
) {
    for request in requests.read() {
        if request.missed {
            request.amount = 0.0;
            continue;
        }
        let Ok(resistance) = targets.get(request.target) else {
            continue;
        };
        request.amount = mitigate(
            request.amount,
            resistance.per_type[request.damage_type.index()],
            resistance.armor,
            &caps,
        );
    }
}

/// 阶段四：上限与最终值 → 发 [`DamageResolvedMessage`]（管线的唯一输出）。
pub fn stage_finalize(
    mut requests: MessageMutator<DamageRequest>,
    config: Res<PipelineConfig>,
    mut resolved: MessageWriter<DamageResolvedMessage>,
) {
    for request in requests.read() {
        let amount = if request.missed {
            0
        } else {
            config.rounding.apply(request.amount)
        };
        resolved.write(DamageResolvedMessage {
            source: request.source,
            target: request.target,
            damage_type: request.damage_type,
            amount,
            missed: request.missed,
            crit: request.crit,
        });
    }
}

/// 把结算结果交给 L0 血量：**唯一写 `Health` 的地方是 `atoms::health`**（**R56**）。
///
/// 被规避（`amount == 0`）时不发消息，避免"0 伤害也算一次命中"。
pub fn forward_resolved_damage(
    mut resolved: MessageReader<DamageResolvedMessage>,
    mut out: MessageWriter<ModifyHealthMessage>,
) {
    for message in resolved.read() {
        if message.amount == 0 {
            continue;
        }
        out.write(ModifyHealthMessage {
            entity: message.target,
            amount: -(message.amount as i32),
        });
    }
}

/// 注册管线：配置、阶段顺序、四个阶段与转发系统。
pub struct DamagePipelinePlugin;

impl Plugin for DamagePipelinePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PipelineConfig>()
            .configure_sets(
                Update,
                (
                    DamageStage::Base,
                    DamageStage::AttackerBonus,
                    DamageStage::DefenderMitigation,
                    DamageStage::Finalize,
                )
                    .chain(),
            )
            .add_systems(Update, stage_base.in_set(DamageStage::Base))
            .add_systems(
                Update,
                stage_attacker_bonus.in_set(DamageStage::AttackerBonus),
            )
            .add_systems(
                Update,
                stage_defender_mitigation.in_set(DamageStage::DefenderMitigation),
            )
            .add_systems(Update, stage_finalize.in_set(DamageStage::Finalize))
            .add_systems(Update, forward_resolved_damage.after(DamageStage::Finalize));
    }
}
