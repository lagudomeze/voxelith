//! 描述结构 → 运行时定义：技能 / 状态 / 怪物的 `resolve` 流程。
//!
//! 只做"翻译"，不做语义判断；查不到名字就报 [`LoaderError`](super::LoaderError)。

use crate::atoms::vocabulary::ActorTagId;
use crate::behaviors::content::descriptor::{
    ActorRon, AiChoiceRon, RoleRon, SkillRon, StackingRon, StatusRon, TargetingRon,
};
use crate::behaviors::content::descriptor_world::{FaceTextureRon, PatternRon, WorldRon};
use crate::behaviors::content::vocabulary::{SkillId, StatusId, Vocab};
use crate::behaviors::monster::{AiChoice, MonsterDef};
use crate::behaviors::requirement::{Cost, Targeting};
use crate::behaviors::skill::{Skill, SkillTags};
use crate::behaviors::status::{ModifierDef, Stacking, StatusDef};

use super::resolve::{resolve_condition, resolve_effect, resolve_requirement, resolve_value};
use super::types::{ActorTemplate, LoaderError, WorldTemplate};
/// 解析一个技能定义。
pub(super) fn resolve_skill(
    ron: &SkillRon,
    id: SkillId,
    vocab: &Vocab,
) -> Result<Skill, LoaderError> {
    let mut requirements = Vec::new();
    for requirement in &ron.requirements {
        requirements.push(resolve_requirement(requirement, vocab)?);
    }

    let mut costs = Vec::new();
    for cost in &ron.costs {
        costs.push(Cost {
            pool: vocab.resource(&cost.pool)?,
            amount: cost.amount,
        });
    }

    let mut effects = Vec::new();
    for effect in &ron.effects {
        effects.push(resolve_effect(effect, vocab)?);
    }

    Ok(Skill {
        id,
        name: ron.name.clone(),
        icon: ron.icon.clone(),
        tags: ron.tags.iter().copied().collect(),
        roles: ron.roles.clone(),
        duration: ron.duration,
        requirements,
        costs,
        targeting: resolve_targeting(ron.targeting),
        effects,
    })
}

/// 目标解析方式。
pub(super) fn resolve_targeting(targeting: TargetingRon) -> Targeting {
    match targeting {
        TargetingRon::SelfOnly => Targeting::SelfOnly,
        TargetingRon::ThreatSource => Targeting::ThreatSource,
        TargetingRon::CurrentSelection => Targeting::CurrentSelection,
        TargetingRon::NearestEnemy => Targeting::NearestEnemy,
    }
}

/// 解析一个状态定义。
pub(super) fn resolve_status(
    ron: &StatusRon,
    id: StatusId,
    vocab: &Vocab,
) -> Result<StatusDef, LoaderError> {
    let mut modifiers = Vec::new();
    for modifier in &ron.modifiers {
        modifiers.push(ModifierDef {
            stat: vocab.stat(&modifier.stat)?,
            delta: resolve_value(&modifier.delta, vocab)?,
        });
    }

    let mut blocks_tags = SkillTags::NONE;
    for tag in &ron.blocks_tags {
        blocks_tags.insert(SkillTags::from_tag(*tag));
    }

    let mut on_apply = Vec::new();
    for effect in &ron.on_apply {
        on_apply.push(resolve_effect(effect, vocab)?);
    }
    let mut on_tick = Vec::new();
    for effect in &ron.on_tick {
        on_tick.push(resolve_effect(effect, vocab)?);
    }
    let mut on_expire = Vec::new();
    for effect in &ron.on_expire {
        on_expire.push(resolve_effect(effect, vocab)?);
    }
    let mut on_remove = Vec::new();
    for effect in &ron.on_remove {
        on_remove.push(resolve_effect(effect, vocab)?);
    }

    Ok(StatusDef {
        id,
        name: ron.name.clone(),
        default_duration: ron.default_duration,
        stacking: match ron.stacking {
            StackingRon::Refresh => Stacking::Refresh,
            StackingRon::Stack { max } => Stacking::Stack { max },
            StackingRon::Ignore => Stacking::Ignore,
            StackingRon::Replace => Stacking::Replace,
        },
        modifiers,
        blocks_tags,
        on_apply,
        on_tick,
        on_expire,
        on_remove,
    })
}

/// 解析角色模板（玩家与怪物共用），按 `role` 分到两组。
///
/// 三个启动期检查都在这里：
/// - AI 引用的技能必须存在（否则怪物永远不出手却不报错）；
/// - `id` 不能重复（生成时无法判断要哪一个）；
/// - `role: Monster` 必须有 AI 候选（否则是个站着挨打的靶子）。
pub(super) fn resolve_actors(
    vocab: &Vocab,
    actors: &[ActorRon],
) -> Result<(Vec<ActorTemplate>, Vec<ActorTemplate>), LoaderError> {
    let mut players = Vec::new();
    let mut monsters = Vec::new();
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();

    for ron in actors {
        if !seen.insert(ron.id.as_str()) {
            return Err(LoaderError::DuplicateActor { id: ron.id.clone() });
        }
        if ron.role == RoleRon::Monster && ron.ai.is_empty() {
            return Err(LoaderError::MonsterWithoutAi { id: ron.id.clone() });
        }

        let mut ai: Vec<AiChoice> = Vec::new();
        for AiChoiceRon {
            skill,
            when,
            weight,
        } in &ron.ai
        {
            let skill_id = vocab.skill(skill).map_err(|_| LoaderError::UnknownSkill {
                actor: ron.id.clone(),
                skill: skill.clone(),
            })?;
            ai.push(AiChoice {
                skill: skill_id,
                when: resolve_condition(when, vocab)?,
                weight: *weight,
            });
        }

        let mut resources = Vec::new();
        for (name, value) in &ron.resources {
            resources.push((vocab.resource(name)?, *value));
        }
        let mut stats = Vec::new();
        for (name, value) in &ron.stats {
            stats.push((vocab.stat(name)?, *value));
        }

        // 特性：内容是名字列表（`traits: ["beast"]`），这里换成词汇 ID。
        // **漏配不会静默** —— 没在 `vocabulary.ron` 里登记过的特性名直接报错。
        let traits: Vec<ActorTagId> = ron
            .traits
            .iter()
            .map(|name| vocab.tag(name))
            .collect::<Result<_, _>>()?;

        let template = ActorTemplate {
            key: ron.id.clone(),
            name: ron.name.clone(),
            role: ron.role,
            faction: ron.faction,
            traits,
            definition: MonsterDef {
                name: ron.name.clone(),
                ai,
            },
            resources,
            stats,
            energy_rate: ron.energy_rate,
            energy_threshold: ron.energy_threshold,
        };

        if template.is_monster() {
            monsters.push(template);
        } else {
            players.push(template);
        }
    }

    Ok((players, monsters))
}

/// 解析体素世界定义：方块名 → ID，地形参数填上 ID。
///
/// **检查**：地形引用的三个方块名必须都在 `blocks` 里（否则生成出来的地形会有
/// "看不见的方块"——ID 查不到贴图，网格化直接跳过那一面，地表会出现空洞）。
pub(super) fn resolve_world(ron: &WorldRon) -> WorldTemplate {
    let mut names = crate::world::VoxelNames::default();
    let mut blocks = Vec::with_capacity(ron.blocks.len());
    for block in &ron.blocks {
        names.register(&block.id);
        blocks.push(crate::world::BlockDef {
            id: block.id.clone(),
            side: resolve_face(block.side),
            top: block.top.map(resolve_face),
            bottom: block.bottom.map(resolve_face),
            opaque: block.opaque,
        });
    }

    // 名字查不到时退回 0（空气）：**不 panic**——地形少一层比启动崩掉好，
    // 而且"配错"会在 `world.ron` 的守卫测试里被抓到。
    let lookup = |name: &str| names.id(name).unwrap_or(crate::world::VoxelId(0));
    let terrain = crate::world::TerrainParams {
        base_height: ron.terrain.base_height,
        amplitude: ron.terrain.amplitude,
        height: crate::world::NoiseParams {
            frequency: ron.terrain.height.frequency,
            octaves: ron.terrain.height.octaves,
            lacunarity: ron.terrain.height.lacunarity,
            persistence: ron.terrain.height.persistence,
            seed: ron.terrain.height.seed,
        },
        soil_depth: ron.terrain.soil_depth,
        surface: lookup(&ron.terrain.surface),
        soil: lookup(&ron.terrain.soil),
        deep: lookup(&ron.terrain.deep),
        floor_y: ron.terrain.floor_y,
    };

    WorldTemplate {
        terrain,
        blocks,
        names,
        sprite: (ron.sprite.world_size, ron.sprite.height),
    }
}

/// 图案与颜色：`u8` 数组直接搬过去（RON 与运行时同形，只是类型不同名）。
fn resolve_face(face: FaceTextureRon) -> crate::world::FaceTexture {
    crate::world::FaceTexture {
        base: face.base,
        pattern: match face.pattern {
            PatternRon::Solid => crate::world::TexturePattern::Solid,
            PatternRon::Speckle { amount } => crate::world::TexturePattern::Speckle { amount },
            PatternRon::Stripes { spacing, amount } => {
                crate::world::TexturePattern::Stripes { spacing, amount }
            }
            PatternRon::Topped { rows, amount } => {
                crate::world::TexturePattern::Topped { rows, amount }
            }
        },
    }
}
