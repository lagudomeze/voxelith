//! **战斗存档**：把新栈的战斗状态交给 `moonshine-save`。
//!
//! ## 为什么中间要一层"快照组件"
//!
//! gauge 的 `Attributes` **不能反射**（内部是 `pub(crate)` 的节点表，也没有 `Reflect`），
//! 所以它没法直接进存档。于是存档里放的是一份**显式快照**：
//!
//! ```text
//! 存  capture_combat_snapshot   读 `Attributes` + 技能栏（`AbilityId`）→ 写进 `SavedCombat`
//!     trigger_save              moonshine 把带 `save` 字段的实体写进文件
//! 读  trigger_load              moonshine 把 `SavedCombat` 还原回来
//!     apply_combat_snapshot     把快照推回 `Attributes`（`push_set`）并按 id 重建技能栏
//! ```
//!
//! ## 存哪些属性名：**角色模板自己写过的那几条**
//!
//! 不存派生值（`MaxHealth` 之类的）。gauge 的修改是**累加**，把派生的 `MaxHealth`
//! 也当字面量恢复回去，就会变成"表达式算一遍 + 存档再加一遍"✗。
//! 这和 `actors.rs` 里"角色不许写全局定义过的名字"是同一条规则。
//!
//! ## 技能栏为什么要存 id 而不是实体
//!
//! 技能是**实体**（状态机 + 计时器），实体 id 不能跨存档。所以存 `AbilityId`
//! （内容里的 `id`），读档时按 id 重建——`AbilityId` 这个组件就是为它加的。
//!
//! ## ⚠️ 已知缺口：**读档还没有真正打通**
//!
//! moonshine 的读档是"**重建实体**"（不是就地覆盖）：它会把存档里的实体重新生成。
//! 而我们只存了 `SavedActor` / `SavedCombat` 两个快照组件，所以读档之后实体身上
//! **只有这两个组件**（连 `Playable` / `CellPos` / `Attributes` 都没有）。
//!
//! 要真正可用还差一步：读档后按 `SavedActor.template` **重建一个完整角色**
//! （模板属性 → 覆盖快照的值 → 补 `Playable` / `CellPos` / 瞄点 → 重建技能栏），
//! 再收掉那个只带快照的壳实体。
//!
//! 现在验的是**两半**（各有测试）：① 抄快照 + moonshine 落盘；
//! ② 把快照推回数值层 + 按 id 重建技能栏。
//!
//! ## 还没存
//!
//! **生效中的状态**（中毒还剩几秒、叠了几层）。它们同样是实体 + 计时器，
//! 要存就得连"剩余时长"一起存再重建；现在读档后状态会清空（这是**有意的**：
//! 宁可干净，不要半截状态）。存档格式里留了 `version`（见 `save`）以便将来加字段。

use std::path::PathBuf;

use bevy::prelude::*;

use moonshine_save::prelude::*;
use voxelith_abilities::ability::AbilityId;
use voxelith_abilities::builder::build_ability;
use voxelith_abilities::{Attributes, AttributesMut, InstantExt, InstantModifierSet, InvokedBy};

use crate::combat::{CombatContent, Loadout};

/// 一条存下来的属性（`Vec<(String, f32)>` 不可反射，所以用这个结构）。
#[derive(Reflect, Clone, Debug, Default, PartialEq)]
pub struct SavedAttribute {
    /// 属性名。
    pub name: String,
    /// 存档那一刻的值。
    pub value: f32,
}

/// 这个实体是**要存档的角色**，以及它用的是哪个角色模板。
///
/// ⚠️ moonshine 的 `Save` 是一个**独立标记组件**（不是本结构体的字段）：
/// 实体上挂了它才会被写进存档。所以这里两个组件与 `Save` 一起构成"可存档的角色"。
#[derive(Component, Reflect, Clone, Debug)]
#[reflect(Component)]
pub struct SavedActor {
    /// 角色模板 id（`actors.ron` 里的 `id`）：读档时据此知道该恢复哪些属性名。
    pub template: String,
}

/// 战斗快照（挂在角色身上；`save` 字段让它进存档）。
#[derive(Component, Reflect, Clone, Debug, Default)]
#[reflect(Component)]
pub struct SavedCombat {
    /// 角色模板定义过的那几条属性。
    pub attributes: Vec<SavedAttribute>,
    /// 技能栏（技能的**内容 id**，按栏位顺序）。
    pub bar: Vec<String>,
}

/// 请求存一份战斗存档（L2 / 输入 / 调试命令发它）。
#[derive(Message, Clone, Debug)]
pub struct SaveCombat {
    /// 写到哪。
    pub path: PathBuf,
}

/// 请求读一份战斗存档。
#[derive(Message, Clone, Debug)]
pub struct LoadCombat {
    /// 从哪读。
    pub path: PathBuf,
}

/// 待处理的存 / 读请求（把消息收进资源，好让"抄快照"与"落盘"各看一次）。
///
/// ⚠️ 为什么需要它：最初 `capture_combat_snapshot` **每帧都抄**——那既是一份无谓的镜像，
/// 又会让"读档恢复"失效（刚写进 `SavedCombat` 的值当帧就被实时值覆盖回去）。
#[derive(Resource, Debug, Clone, Default)]
pub struct PersistenceQueue {
    /// 待写的存档。
    pub saves: Vec<PathBuf>,
    /// 待读的存档。
    pub loads: Vec<PathBuf>,
}

/// 读档后的**沉降窗口**：moonshine 的读档是观察者式的（消息落地的时机在调度之后），
/// 所以"把快照推回数值层"这件事要给它几帧机会，而不是抢在同一帧里做。
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct CombatLoadPending {
    /// 还剩几帧去做"推回数值层"。
    pub frames: u8,
}

/// 读档后留几帧给 moonshine 落地。
const LOAD_SETTLE_FRAMES: u8 = 3;

/// 存之前把实时状态抄进快照。
///
/// 抄的是**角色模板自己写过的那几条属性**（不抄派生值，理由见模块文档）。
pub fn capture_combat_snapshot(
    content: Res<CombatContent>,
    queue: Res<PersistenceQueue>,
    actors: Query<(Entity, &SavedActor, &Attributes, &Loadout)>,
    ability_ids: Query<&AbilityId>,
    mut commands: Commands,
) {
    // **只在有存档请求时抄**：平时不碰这些组件（否则就是每帧一份镜像，
    // 而且会把读档恢复的值当帧覆盖掉）。
    if queue.saves.is_empty() {
        return;
    }
    for (entity, actor, attributes, loadout) in &actors {
        let Some(names) = content.actors.get(&actor.template).map(|definition| {
            definition
                .attributes
                .iter()
                .map(|modifier| modifier.name.clone())
                .collect::<Vec<_>>()
        }) else {
            warn!("[save] 存档要求角色模板 `{}`，但内容里没有", actor.template);
            continue;
        };

        let attributes = names
            .into_iter()
            .map(|name| SavedAttribute {
                value: attributes.value(&name),
                name,
            })
            .collect();

        // 技能栏：按栏位顺序翻译成内容 id（技能是实体，存不进去）。
        let bar = loadout
            .0
            .iter()
            .filter_map(|ability| ability_ids.get(*ability).ok().map(|id| id.0.clone()))
            .collect();

        commands
            .entity(entity)
            .insert((SavedCombat { attributes, bar }, Save));
    }
}

/// 把消息收进队列（**排在最前**：后面的系统各看一次，不必共享消息游标）。
pub fn collect_persistence_requests(
    mut saves: MessageReader<SaveCombat>,
    mut loads: MessageReader<LoadCombat>,
    mut queue: ResMut<PersistenceQueue>,
) {
    queue
        .saves
        .extend(saves.read().map(|request| request.path.clone()));
    queue
        .loads
        .extend(loads.read().map(|request| request.path.clone()));
}

/// 真正让 moonshine 写文件（**必须排在抄快照之后**，否则抄不到）。
pub fn trigger_combat_save(mut queue: ResMut<PersistenceQueue>, mut commands: Commands) {
    for path in queue.saves.drain(..) {
        info!("[save] 写战斗存档：{}", path.display());
        commands.trigger_save(SaveWorld::default_into_file(path));
    }
}

/// 请求读档：交给 moonshine，并把"沉降窗口"打开。
pub fn trigger_combat_load(
    mut queue: ResMut<PersistenceQueue>,
    mut pending: ResMut<CombatLoadPending>,
    mut commands: Commands,
) {
    for path in queue.loads.drain(..) {
        info!("[save] 读战斗存档：{}", path.display());
        pending.frames = LOAD_SETTLE_FRAMES;
        commands.trigger_load(LoadWorld::default_from_file(path));
    }
}

/// 把快照推回数值层，并按 id 重建技能栏。
///
/// **用"加差值"实现"设成"**。
///
/// gauge 的即时修改是**加性**的，而 `push_set` 会和 `AttributeInitializer`（基础值）
/// 打架——实测下来值纹丝不动。所以这里先读当前值、算出差值再 `push_add`：
/// 语义上仍然是"设成快照里的那个数"，而且与初始化器互不干扰。
///
/// 读写要拆开（`AttributesMut` 内部就是 `Query<&mut Attributes>`，同系统里再放一个
/// 只读查询会撞 B0001），所以用 `ParamSet`——本项目的惯用手法。
#[allow(clippy::too_many_arguments)]
pub fn apply_combat_snapshot(
    mut pending: ResMut<CombatLoadPending>,
    content: Res<CombatContent>,
    mut gauge: ParamSet<(Query<&Attributes>, AttributesMut)>,
    actors: Query<(Entity, &SavedCombat)>,
    mut loadouts: Query<&mut Loadout>,
    mut commands: Commands,
) {
    if pending.frames == 0 {
        return;
    }
    pending.frames -= 1;

    // ① 只读：算出每个角色每条属性要"加多少"。
    let mut plan: Vec<(Entity, Vec<(String, f32)>)> = Vec::new();
    {
        let readers = gauge.p0();
        for (entity, saved) in &actors {
            let Ok(current) = readers.get(entity) else {
                continue;
            };
            let pushes = saved
                .attributes
                .iter()
                .map(|attribute| {
                    let delta = attribute.value - current.value(&attribute.name);
                    (attribute.name.clone(), delta)
                })
                .filter(|(_, delta)| delta.abs() > f32::EPSILON)
                .collect::<Vec<_>>();
            plan.push((entity, pushes));
        }
    }

    // ② 写：一次性推给数值层。
    {
        let mut writer = gauge.p1();
        for (entity, pushes) in &plan {
            let mut instant = InstantModifierSet::new();
            for (name, delta) in pushes {
                instant.push_add(name, *delta);
            }
            writer.apply_instant(&instant, &[], *entity);
        }
    }

    for (entity, saved) in &actors {
        // ② 技能栏：按内容 id 重建，再把旧的技能实体收掉。
        let mut fresh: Vec<Entity> = Vec::new();
        for id in &saved.bar {
            let Some(definition) = content.abilities.iter().find(|ability| &ability.id == id)
            else {
                warn!("[save] 存档里的技能 `{id}` 在内容里找不到，跳过");
                continue;
            };
            let ability = commands
                .spawn_scene(build_ability(definition, &content.statuses))
                .id();
            commands.entity(ability).insert(InvokedBy(entity));
            fresh.push(ability);
        }
        if let Ok(mut loadout) = loadouts.get_mut(entity) {
            for old in &loadout.0 {
                commands.entity(*old).try_despawn();
            }
            loadout.0 = fresh;
        }
    }
}

/// 注册战斗存档。
pub struct CombatSavePlugin;

impl Plugin for CombatSavePlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<SaveCombat>()
            .add_message::<LoadCombat>()
            .init_resource::<CombatLoadPending>()
            .init_resource::<PersistenceQueue>()
            // moonshine 走反射读写，所以每个进存档的类型都要登记（嵌套的也要）。
            .register_type::<SavedAttribute>()
            .register_type::<SavedActor>()
            .register_type::<SavedCombat>()
            .register_type::<Vec<SavedAttribute>>()
            .register_type::<Vec<String>>()
            .add_systems(
                Update,
                (
                    // 顺序是契约：收请求 → 抄快照 → 落盘 → 读档 → 把快照推回数值层。
                    collect_persistence_requests,
                    ApplyDeferred,
                    capture_combat_snapshot,
                    ApplyDeferred,
                    trigger_combat_save,
                    trigger_combat_load,
                    apply_combat_snapshot,
                )
                    .chain(),
            );
    }
}
