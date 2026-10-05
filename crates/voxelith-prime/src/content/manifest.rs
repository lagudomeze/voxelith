//! 内容清单：**六份静态配置的装载入口**。
//!
//! ```text
//! ContentManifest（SceneComponent，路径全写在 scene() 里）
//!   ├─ data/vocabulary.ron ─► VocabularyAsset ─┐
//!   ├─ data/skills.ron     ─► SkillsAsset      │
//!   ├─ data/statuses.ron   ─► StatusesAsset    │  PreStartup 里**阻塞**等到全部就绪
//!   ├─ data/players.ron    ─► PlayersAsset     │  （失败 / 超时立刻 panic，带文件名）
//!   ├─ data/monsters.ron   ─► MonstersAsset    │
//!   └─ data/world.ron      ─► WorldAsset      ─┘
//!                                             ↓ 拼成 RawContent，insert_resource
//!                                        Startup 照旧，一行没改
//! ```
//!
//! ## 为什么用 `SceneComponent`
//!
//! 加载逻辑**内聚在 [`ContentManifest::scene`] 里**：路径字符串写在那个函数里，
//! `bsn!` 把 `"data/skills.ron"` 自动变成 `HandleTemplate::Path`（装载时调
//! `AssetServer::load`）。调用方只写 `world.spawn_scene(bsn! { @ContentManifest })`，
//! **不需要知道任何一个文件名**——加一份配置只改这一个文件。
//!
//! ## 为什么在 `PreStartup` 阻塞
//!
//! `Startup` 里一串系统（图集 / 材质 / 地形 / 精灵表 / HUD）都假设"内容已经在"，
//! 而它们的参数是 `Option<Res<ContentData>>`——**读不到就静默建一张空图集**，
//! 战场空掉却不报错。与其把那一串系统全改成异步，不如在这里守住
//! "`Startup` 一定发生在内容就绪之后"这条不变式。
//!
//! 阻塞是安全的：装载跑在 `IoTaskPool`（独立线程）上，主线程泵
//! `handle_internal_asset_events` 就能收到结果，两边不会互相等。六份本地小文件，
//! 实测在毫秒级；超时与失败都**当场叫出来**，不会挂死。

use std::time::{Duration, Instant};

use bevy::asset::LoadState;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::scene::{Scene, WorldSceneExt, bsn};

use super::asset::{
    ContentAsset, ContentAssetKind, MonstersAsset, PlayersAsset, SkillsAsset, StatusesAsset,
    VocabularyAsset, WorldAsset,
};
use super::loader::{ContentError, RawContent};

/// 等配置落地的上限。
///
/// 六份本地小文件正常在毫秒级完成；上限存在的意义不是"给它足够时间"，
/// 而是**别在配置写坏时挂死**——超时会 panic 并把六份的状态都打出来。
const MANIFEST_TIMEOUT: Duration = Duration::from_secs(10);

/// 轮询间隔：让出 CPU 给 IO 线程。
const POLL_INTERVAL: Duration = Duration::from_millis(1);

/// 六份静态配置的清单（**`SceneComponent`**）。
///
/// 路径全部写在 [`ContentManifest::scene`] 里；这个结构体的字段就是六个句柄，
/// 谁想知道"这份内容从哪来"看这里，而不是去翻 `main.rs`。
#[derive(SceneComponent, FromTemplate, Debug, Default, Clone)]
pub struct ContentManifest {
    /// 词汇表。
    pub vocabulary: Handle<VocabularyAsset>,
    /// 技能。
    pub skills: Handle<SkillsAsset>,
    /// 状态。
    pub statuses: Handle<StatusesAsset>,
    /// 玩家模板。
    pub players: Handle<PlayersAsset>,
    /// 怪物模板。
    pub monsters: Handle<MonstersAsset>,
    /// 体素世界。
    pub world: Handle<WorldAsset>,
}

impl ContentManifest {
    /// 六份配置的路径——**装载路径的唯一出处**。
    ///
    /// `bsn!` 会把字符串字段转成 `HandleTemplate::Path`，装载时才落到
    /// `AssetServer::load`。所以这里只写"从哪来"，用的人只见到 `Handle<T>`。
    ///
    /// 这些字符串与 [`ContentAssetKind::file`] 是同一批文件名的两处写法（`bsn!` 要字面量），
    /// `the_manifest_paths_match_the_labels` 拿真实句柄把两者钉在一起。
    fn scene() -> impl Scene {
        bsn! {
            ContentManifest {
                vocabulary: "data/vocabulary.ron",
                skills: "data/skills.ron",
                statuses: "data/statuses.ron",
                players: "data/players.ron",
                monsters: "data/monsters.ron",
                world: "data/world.ron",
            }
        }
    }

    /// 六份配置的加载状态（顺序 = 上面的字段顺序，方便对照报错）。
    pub fn load_states(&self, server: &AssetServer) -> [(ContentAssetKind, LoadState); 6] {
        [
            (
                ContentAssetKind::Vocabulary,
                server.load_state(self.vocabulary.id()),
            ),
            (
                ContentAssetKind::Skills,
                server.load_state(self.skills.id()),
            ),
            (
                ContentAssetKind::Statuses,
                server.load_state(self.statuses.id()),
            ),
            (
                ContentAssetKind::Players,
                server.load_state(self.players.id()),
            ),
            (
                ContentAssetKind::Monsters,
                server.load_state(self.monsters.id()),
            ),
            (ContentAssetKind::World, server.load_state(self.world.id())),
        ]
    }

    /// 句柄真实指向的路径（测试用来钉住 [`ContentAssetKind::file`]）。
    pub fn paths(&self, server: &AssetServer) -> [String; 6] {
        [
            label(server, self.vocabulary.id()),
            label(server, self.skills.id()),
            label(server, self.statuses.id()),
            label(server, self.players.id()),
            label(server, self.monsters.id()),
            label(server, self.world.id()),
        ]
    }
}

/// 一个资产 id 的路径字符串。
fn label<A: Asset>(server: &AssetServer, id: AssetId<A>) -> String {
    server
        .get_path(id)
        .map_or_else(|| "<无路径>".to_owned(), |path| path.to_string())
}

/// **`PreStartup`：装载清单并等到六份配置全部就绪。**
///
/// 顺序是契约——它在 `PreStartup` 里跑，而 `Startup` 在它之后，
/// 于是 `Startup` 里那几个 `Option<Res<ContentData>>` 一定读得到东西。
pub fn load_content_manifest(world: &mut World) {
    // 1. 场景装载：`@ContentManifest` 一展开，六个 `AssetServer::load` 就发出去了。
    let entity = world
        .spawn_scene(bsn! { @ContentManifest })
        .unwrap_or_else(|error| panic!("内容清单装载失败：{error}"))
        .id();

    // 2. 等磁盘。配置写坏要**当场**叫出来，不能留给用户一个空战场。
    let started = Instant::now();
    loop {
        bevy::asset::handle_internal_asset_events(world);

        let states = {
            let manifest = world
                .get::<ContentManifest>(entity)
                .expect("刚 spawn 的清单实体不该消失");
            manifest.load_states(world.resource::<AssetServer>())
        };

        if let Some((kind, error)) = states.iter().find_map(|(kind, state)| match state {
            LoadState::Failed(error) => Some((*kind, error.to_string())),
            _ => None,
        }) {
            panic!("配置 {} 加载失败：{error}", kind.file());
        }
        if states.iter().all(|(_, state)| state.is_loaded()) {
            break;
        }

        assert!(
            started.elapsed() < MANIFEST_TIMEOUT,
            "配置加载超时（{:?}）：{:?}",
            MANIFEST_TIMEOUT,
            states
                .iter()
                .map(|(kind, state)| (kind.file(), format!("{state:?}")))
                .collect::<Vec<_>>()
        );
        std::thread::sleep(POLL_INTERVAL);
    }

    // 3. 拼成 `RawContent` 直接插进世界。
    //
    // **`insert_resource` 而不是 `Commands`**：`Startup` 就在下一个阶段，
    // 命令要到帧末才落地，那样 `Startup` 还是读不到。
    let raw = raw_from_world(world).unwrap_or_else(|error| panic!("内容读取失败：{error}"));
    world.insert_resource(raw);
}

/// 从世界里的六份资产拼出 [`RawContent`]（exclusive 系统用）。
pub fn raw_from_world(world: &mut World) -> Result<RawContent, ContentError> {
    let manifest = {
        let mut state = world.query::<&ContentManifest>();
        state.iter(world).next().cloned()
    }
    .ok_or(ContentError::MissingManifest)?;

    Ok(RawContent {
        vocabulary: fetch(world, &manifest.vocabulary, ContentAssetKind::Vocabulary)?,
        skills: fetch(world, &manifest.skills, ContentAssetKind::Skills)?,
        statuses: fetch(world, &manifest.statuses, ContentAssetKind::Statuses)?,
        players: fetch(world, &manifest.players, ContentAssetKind::Players)?,
        monsters: fetch(world, &manifest.monsters, ContentAssetKind::Monsters)?,
        world: fetch(world, &manifest.world, ContentAssetKind::World)?,
    })
}

/// 取一份已经落地的配置（返回其内部描述结构）。
fn fetch<A: ContentAsset>(
    world: &World,
    handle: &Handle<A>,
    kind: ContentAssetKind,
) -> Result<A::Ron, ContentError> {
    world
        .resource::<Assets<A>>()
        .get(handle)
        .map(|asset| asset.ron().clone())
        .ok_or(ContentError::AssetNotLoaded { kind })
}

/// 在系统里读六份资产（`SystemParam`：一个参数顶八个）。
#[derive(SystemParam)]
pub struct LoadedContentAssets<'w, 's> {
    /// 清单本身：句柄都在它身上。
    pub manifest: Query<'w, 's, &'static ContentManifest>,
    vocabulary: Res<'w, Assets<VocabularyAsset>>,
    skills: Res<'w, Assets<SkillsAsset>>,
    statuses: Res<'w, Assets<StatusesAsset>>,
    players: Res<'w, Assets<PlayersAsset>>,
    monsters: Res<'w, Assets<MonstersAsset>>,
    world: Res<'w, Assets<WorldAsset>>,
}

impl LoadedContentAssets<'_, '_> {
    /// 按清单里的句柄把六份配置取出来，拼成 [`RawContent`]。
    pub fn raw(&self) -> Result<RawContent, ContentError> {
        fn take<A: ContentAsset>(
            assets: &Assets<A>,
            handle: &Handle<A>,
            kind: ContentAssetKind,
        ) -> Result<A::Ron, ContentError> {
            assets
                .get(handle)
                .map(|asset| asset.ron().clone())
                .ok_or(ContentError::AssetNotLoaded { kind })
        }

        let manifest = self
            .manifest
            .iter()
            .next()
            .ok_or(ContentError::MissingManifest)?;

        Ok(RawContent {
            vocabulary: take(
                &self.vocabulary,
                &manifest.vocabulary,
                ContentAssetKind::Vocabulary,
            )?,
            skills: take(&self.skills, &manifest.skills, ContentAssetKind::Skills)?,
            statuses: take(
                &self.statuses,
                &manifest.statuses,
                ContentAssetKind::Statuses,
            )?,
            players: take(&self.players, &manifest.players, ContentAssetKind::Players)?,
            monsters: take(
                &self.monsters,
                &manifest.monsters,
                ContentAssetKind::Monsters,
            )?,
            world: take(&self.world, &manifest.world, ContentAssetKind::World)?,
        })
    }
}

/// 六份配置的变更流（热重载监听）。
#[derive(SystemParam)]
pub struct ContentChanges<'w, 's> {
    vocabulary: MessageReader<'w, 's, AssetEvent<VocabularyAsset>>,
    skills: MessageReader<'w, 's, AssetEvent<SkillsAsset>>,
    statuses: MessageReader<'w, 's, AssetEvent<StatusesAsset>>,
    players: MessageReader<'w, 's, AssetEvent<PlayersAsset>>,
    monsters: MessageReader<'w, 's, AssetEvent<MonstersAsset>>,
    world: MessageReader<'w, 's, AssetEvent<WorldAsset>>,
}

impl ContentChanges<'_, '_> {
    /// 这一帧有没有配置**被改动**。
    ///
    /// 只看 [`AssetEvent::Modified`]：`Added` / `LoadedWithDependencies` 是首次装载，
    /// 那时内容还没翻译过，不该触发"重载"。
    pub fn take(&mut self) -> bool {
        fn modified<A: Asset>(reader: &mut MessageReader<AssetEvent<A>>) -> bool {
            reader
                .read()
                .any(|event| matches!(event, AssetEvent::Modified { .. }))
        }
        // 六个都要读完（哪怕前面已经为真）：留下没读的消息会在下一帧再触发一次重载。
        let hits = [
            modified(&mut self.vocabulary),
            modified(&mut self.skills),
            modified(&mut self.statuses),
            modified(&mut self.players),
            modified(&mut self.monsters),
            modified(&mut self.world),
        ];
        hits.into_iter().any(|hit| hit)
    }
}
