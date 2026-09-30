//! L2 顶层装配：`VoxelithPlugin`。
//!
//! 按 **R42/R43**，`main.rs` 只注册**顶层**插件；逻辑层与调试层的具体装配收在本插件内部，
//! 子插件不暴露给 `main.rs`。
//!
//! 配置注入落点：**L2 负责加载 / 构造配置，L1 只消费 Resource**。
//! `CombatConfig` 就是这条链的入口——真实项目里它来自文件（关卡 / 平衡表），
//! 加载完 `insert_resource` 一次，`CombatPlugin` 会把它分发到各子域。

use core::time::Duration;

use bevy::prelude::*;
use voxelith_axiom::atoms::HealthPlugin;
use voxelith_axiom::atoms::modifiers::{Modifier, ModifierSource};
use voxelith_axiom::atoms::stats::{
    AddStatModifierMessage, Level, Stat, StatBlock, StatId, StatPlugin,
};
use voxelith_axiom::behaviors::combat::{CombatConfig, CombatPlugin};
use voxelith_axiom::behaviors::resistance::Resistance;

use crate::debug::{self, DEFAULT_BRP_PORT};

/// Voxelith 的顶层插件（**R41.3** 对外发布的组装入口）。
pub struct VoxelithPlugin {
    /// BRP 监听端口，供 AI / 脚本连接。
    pub brp_port: u16,
}

impl VoxelithPlugin {
    /// 用默认端口构造。
    pub fn new() -> Self {
        Self {
            brp_port: DEFAULT_BRP_PORT,
        }
    }

    /// 覆盖 BRP 端口。
    pub fn with_brp_port(mut self, port: u16) -> Self {
        self.brp_port = port;
        self
    }
}

impl Default for VoxelithPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for VoxelithPlugin {
    fn build(&self, app: &mut App) {
        // ---- 内容层配置（Resource 注入点）----
        // 真实项目：从文件 / 平衡表加载后构造；这里先用默认值。
        // `CombatPlugin` 在缺失时也会退回默认值，所以这一行是可选的示范。
        app.insert_resource(CombatConfig::new());

        // ---- L0 / L1：核心机制（子插件嵌套，不外泄给 main.rs，R42）----
        // L0：`StatPlugin` 管账本与唯一写入口，`HealthPlugin` 管血量执行；
        // L1：`CombatPlugin` 装配属性公式 / 成长 / 伤害 / 抵抗 / 判定 / 管线 / 状态。
        app.add_plugins((HealthPlugin, StatPlugin, CombatPlugin));

        // ---- L2 调试设施 ----
        // 需在 `DefaultPlugins` 之后：BRP 与 egui 检查器都依赖它提供的
        // 窗口 / 输入消息 / 渲染。
        debug::install(app, self.brp_port);

        // 骨架阶段的演示实体：让调试通道一启动就有组件可查。
        app.add_systems(Startup, spawn_debug_sample);
    }
}

/// 生成演示实体，并把**修饰符链路**真正跑起来：装备（永久）+ 药水（临时）。
///
/// 组件约定（都是自成一体：收到消息自己刷新视图）：
/// - `Stat`（L0）：账本 + 修饰符槽位 + 最终值视图，服装 / 药水的修饰符按消息发进去；
/// - `Resistance`（L1）：抵抗基础值 + 修饰符 + 视图，伤害管线只读它的视图；
/// - `Level`（L0）：经验与升级由 L1 的成长系统驱动，升级发下来的点数再交给 `Stat`。
fn spawn_debug_sample(
    mut commands: Commands,
    mut modifier_requests: MessageWriter<AddStatModifierMessage>,
) {
    // 显式字段：默认全 10，只覆盖需要变化的两项（内容层构造属性就是这样写）。
    let base = StatBlock {
        dexterity: 8,
        constitution: 12,
        ..StatBlock::new(10)
    };

    let actor = commands
        .spawn((
            Name::new("debug-sample"),
            voxelith_axiom::atoms::Health::new(100),
            Stat::from_base(base),
            Resistance::new(),
            Level::new(),
        ))
        .id();

    // 永久：腰带 +5 力量（来源实体 = 腰带，卸下时按来源一次性移除）
    let belt = commands.spawn(Name::new("giant-belt")).id();
    modifier_requests.write(AddStatModifierMessage {
        entity: actor,
        stat: StatId::Strength,
        modifier: Modifier::flat(5.0, ModifierSource::new(belt)),
    });

    // 临时：药水 +50% 力量，5 秒后由 `tick_stat_modifier_lifetimes` 自动移除
    let potion = commands.spawn(Name::new("potion-of-strength")).id();
    modifier_requests.write(AddStatModifierMessage {
        entity: actor,
        stat: StatId::Strength,
        modifier: Modifier::percent_add(0.5, ModifierSource::new(potion))
            .lasting(Duration::from_secs(5)),
    });

    info!(
        "已生成 `debug-sample`（血量 100，力量 10 +5 腰带，再 +50% 药水 5 秒；敏捷 8，体质 12，1 级）"
    );
}
