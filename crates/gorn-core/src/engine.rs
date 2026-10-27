//! Ядро трекера: держит состояние, принимает строки лога и отдаёт готовые
//! срезы для интерфейса. Перенос `main/engine.ts`.
//!
//! В отличие от эталона движок не владеет потоками: дочитывание лога, пульс
//! и опрос цен сообщества запускает обвязка (приложение), а движок даёт
//! методы `consume`, `on_log_rotate`, `heartbeat` и признак «пора отправить
//! срез» (`take_emit`). Часы подменяются — для сверки срезов с эталоном.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use indexmap::{IndexMap, IndexSet};
use regex::Regex;
use serde::Serialize;

use crate::analysis::*;
use crate::bag::bag_value;
use crate::community::{parse_iso_ms, Clock, CommunityPriceFeed};
use crate::csv::{js_num_str, js_round, to_csv, CsvCell};
use crate::history::{daily_totals, group_by_map, next_day_start, session_totals, DayTotals, MapStats, SessionBounds, SessionTotals};
use crate::inventory::InventoryTracker;
use crate::labels::{hero_name, season_name, Lang};
use crate::locate::find_log_directory;
use crate::mapnames::{MapCatalogue, MapNames};
use crate::parser::EventParser;
use crate::patterns::{is_map_key_type, is_untracked_loot_type, patterns, UNTRACKED_LOOT_IDS};
use crate::prices::{choose_price, community_reference, is_bound_name, price_trend, PriceSource, PriceStore, PriceTrend, OWN_PRICE_TTL_MS};
use crate::retroscan::{retro_scan, run_cost, run_value, RetroScanProgress};
use crate::segmenter::RunSegmenter;
use crate::stats::{percentile, per_hour, sort_num, summarize_timed, RateSummary};
use crate::storage::Storage;
use crate::trade::{merge_by_key, recall_key, TradeLedger};
use crate::types::*;

/// Источник цены для интерфейса: исключённый предмет стоит ноль по воле игрока
/// (или вне учёта добычи вовсе: `ItemInfo::untracked`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ViewPriceSource {
    Own,
    Community,
    Excluded,
}

impl From<PriceSource> for ViewPriceSource {
    fn from(s: PriceSource) -> Self {
        match s {
            PriceSource::Own => ViewPriceSource::Own,
            PriceSource::Community => ViewPriceSource::Community,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Picked {
    fe: f64,
    source: ViewPriceSource,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DropView {
    pub config_base_id: ConfigBaseId,
    pub name: String,
    pub icon: Option<String>,
    pub qty: i64,
    pub unit_fe: Option<f64>,
    pub total_fe: Option<f64>,
    pub price_source: Option<ViewPriceSource>,
}

/// Строка блока «Цены».
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PriceRow {
    pub config_base_id: ConfigBaseId,
    pub name: String,
    pub icon: Option<String>,
    /// Цена, по которой сейчас считается добыча.
    pub fe: Option<f64>,
    pub source: Option<ViewPriceSource>,
    /// Когда пробил сам — по свежей пробивке.
    pub own_observed_at: Option<i64>,
    pub trend: PriceTrend,
    /// Сколько выпало и ушло на карты в видимых забегах.
    pub seen_qty: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BagItemView {
    pub config_base_id: ConfigBaseId,
    pub qty: i64,
    pub total_fe: f64,
    pub name: String,
    pub icon: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BagView {
    pub value_fe: f64,
    pub fe_cash: i64,
    pub unpriced: usize,
    pub top: Vec<BagItemView>,
}

/// Слот сумки с ценой: как в игре, по страницам и номерам слотов.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BagSlotView {
    pub slot_id: i64,
    /// Предмет есть в справочнике. Вне его — то, чем нельзя торговать:
    /// ни названия, ни иконки, ни цены сообщества.
    pub known: bool,
    #[serde(flatten)]
    pub item: DropView,
}

/// Страница сумки: 100 снаряжение, 101 навыки, 102 валюта, 103 прочее; 1, 2… — тайник.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BagPageView {
    pub page_id: i64,
    pub value_fe: f64,
    /// Занятых слотов без цены (кроме непродаваемого вне справочника): на них оценка страницы занижена.
    pub unpriced: usize,
    pub slots: Vec<BagSlotView>,
}

/// Сумка по слотам — вкладка «Сумка» пульта (по запросу, не в каждом срезе).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BagGrid {
    pub pages: Vec<BagPageView>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunView {
    /// Ключ забега: стабилен между срезами.
    pub key: String,
    pub area_id: i64,
    pub name: String,
    pub started_at: i64,
    /// Полная длительность; у открытого — None.
    pub duration_ms: Option<i64>,
    /// Активное время без простоя; у открытого — до сейчас.
    pub active_ms: i64,
    pub fe_gained: i64,
    pub value_fe: f64,
    pub cost_fe: f64,
    pub net_fe: f64,
    pub per_hour: f64,
    pub drops: Vec<DropView>,
    /// Чем открывалась карта.
    pub costs: Vec<DropView>,
    /// Тюрьма Талоса: в итоге сессии есть, в темпе нет.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prison: Option<bool>,
}

/// Результат поиска по всей истории забегов.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunQueryResult {
    pub total: usize,
    pub net_fe: f64,
    pub in_map_ms: i64,
    pub rate: RateSummary,
    /// Свежие сверху, не больше RUN_QUERY_LIMIT.
    pub runs: Vec<RunView>,
}

/// Сколько забегов поиск отдаёт в интерфейс: итоги считаются по всем.
pub const RUN_QUERY_LIMIT: usize = 100;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerView {
    pub player_id: String,
    /// Ник без решёточного суффикса.
    pub name: Option<String>,
    pub hero_name: Option<String>,
    pub level: Option<f64>,
    pub season_id: Option<f64>,
    pub season_name: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerOption {
    #[serde(flatten)]
    pub view: PlayerView,
    /// Фарм-забегов у персонажа.
    pub runs: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CostItemView {
    pub config_base_id: ConfigBaseId,
    pub name: String,
    pub icon: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StrategyView {
    pub key: String,
    pub runs: usize,
    pub rate: RateSummary,
    pub cost_per_run: f64,
    pub net_per_run: f64,
    pub total_active_ms: i64,
    /// Человекочитаемое имя: из чего открывалась карта.
    pub title: String,
    pub cost_items: Vec<CostItemView>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDropView {
    pub config_base_id: ConfigBaseId,
    pub name: String,
    pub icon: Option<String>,
    pub qty: i64,
    pub total_fe: f64,
    pub at: i64,
    pub run_name: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MissingPriceView {
    pub config_base_id: ConfigBaseId,
    pub qty: i64,
    pub runs: usize,
    pub is_cost: bool,
    pub name: String,
    pub icon: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    Searching,
    Idle,
    Running,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Totals {
    pub runs: usize,
    pub in_map_ms: i64,
    /// Активное время в тюрьме за сессию — входит в «в час» вместе с in_map_ms.
    pub prison_ms: i64,
    /// Время сессии по часам: от её начала до сейчас; игра закрыта — до её последней строки лога.
    pub session_ms: i64,
    pub fe_gained: i64,
    pub value_fe: f64,
    pub cost_fe: f64,
    pub realized_fe: f64,
    pub realization_ratio: Option<f64>,
    /// Бумажная оценка выставленного на продажу.
    pub listed_paper_fe: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PeriodSummary {
    pub net_fe: f64,
    pub in_map_ms: i64,
    pub runs: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub started_at: i64,
    pub previous: Option<PeriodSummary>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrisonInfo {
    pub net_fe: f64,
    pub runs: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotDiagnostics {
    pub unknown_protos: Vec<(String, i64)>,
    pub priced_items: usize,
    pub unpriced_drops: usize,
    pub community_prices: usize,
    pub community_enabled: bool,
    pub community_fetched_at: Option<i64>,
    pub community_stale: bool,
}

/// Срез для интерфейса.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub status: Status,
    pub hint: Option<String>,
    pub last_activity_at: i64,
    pub log_dir: Option<String>,
    pub season_id: Option<i64>,
    pub season_name: Option<String>,
    pub all_seasons: bool,
    pub other_season_runs: usize,
    pub player: Option<PlayerView>,
    pub players: Vec<PlayerOption>,
    pub player_filter: Option<String>,
    pub current: Option<RunView>,
    pub recent: Vec<RunView>,
    pub rate: RateSummary,
    pub fe_rate: RateSummary,
    pub live_rate: f64,
    pub norm: f64,
    pub jackpot_auto_fe: f64,
    pub strategies: Vec<StrategyView>,
    pub maps: Vec<MapStats>,
    pub days: Vec<DayTotals>,
    pub sessions: Vec<SessionTotals>,
    pub bag: Option<BagView>,
    pub min_drop_fe: f64,
    pub missing: Vec<MissingPriceView>,
    pub session_top: Vec<SessionDropView>,
    pub prison: PrisonInfo,
    pub session: SessionInfo,
    pub totals: Totals,
    pub diagnostics: SnapshotDiagnostics,
}

/// Какие названия предметов показывать.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NameLanguage {
    #[default]
    Ru,
    En,
    Both,
}

/// Предмет справочника (data/items.json).
#[derive(Clone, Debug, Default)]
pub struct CatalogueItem {
    pub id: ConfigBaseId,
    pub name: String,
    pub name_ru: Option<String>,
    pub type_cn: Option<String>,
    pub icon: Option<String>,
    /// База снаряжения из `gear.json`: название есть, но цена — как у
    /// снаряжения вне справочника (нижняя цена сообщества: зависит от ролла).
    pub gear: bool,
    /// Легендарка: номер её базы. Подбор пишет в лог базу, а пробивка — саму
    /// легендарку; по базе выпавший экземпляр опознаётся (`gear::identify`).
    pub base: Option<ConfigBaseId>,
    /// Легендарное снаряжение (тип «Legendary» в `gear.json`, «Legendary Gear»
    /// в `items.json`): в учёте добычи, в отличие от прочего снаряжения.
    pub legendary: bool,
}

impl CatalogueItem {
    /// Справочник из JSON эталона.
    pub fn parse_list(text: &str) -> serde_json::Result<Vec<CatalogueItem>> {
        let v: Vec<serde_json::Value> = serde_json::from_str(text)?;
        Ok(v.iter()
            .filter_map(|it| {
                Some(CatalogueItem {
                    id: it["id"].as_i64()?,
                    name: it["name"].as_str().unwrap_or_default().to_string(),
                    name_ru: it["nameRu"].as_str().map(str::to_string),
                    type_cn: it["typeCn"].as_str().map(str::to_string),
                    icon: it["icon"].as_str().map(str::to_string),
                    gear: false,
                    base: it["base"].as_i64(),
                    legendary: matches!(it["type"].as_str(), Some("Legendary" | "Legendary Gear"))
                        || it["typeCn"].as_str() == Some("传奇装备"),
                })
            })
            .collect())
    }
}

#[derive(Clone, Debug, Default)]
pub struct EngineOptions {
    pub community_prices: bool,
    pub player_filter: Option<String>,
    pub all_seasons: bool,
    pub excluded_items: Vec<ConfigBaseId>,
    pub min_drop_fe: f64,
    /// Справочник названий карт; None — имена ассетов.
    pub maps: Option<MapCatalogue>,
    pub language: NameLanguage,
    /// Язык интерфейса (подписи, подсказки, CSV).
    pub ui: Lang,
    /// Проверки: цены сообщества обновляются сразу, без потоков опроса.
    pub sync_community: bool,
    /// Адрес сервера цен сообщества (проверки); None — настоящий.
    pub community_url: Option<String>,
    /// Начало адреса иконки (`<начало><id>.webp`); None — `gorn-icon://item/`, как у эталона.
    pub icon_base: Option<String>,
    /// Цена экземпляра: пробивка снаряжения с фильтрами по аффиксам ложится на
    /// последний выпавший экземпляр этой базы, а пробивка или сравнение
    /// легендарки опознаёт выпавшую базу ею (`gear::identify`). У эталона ни
    /// того, ни другого нет — для сверки с ним выключено.
    pub instance_prices: bool,
}

struct ItemInfo {
    name: String,
    name_ru: Option<String>,
    icon: Option<String>,
    gear: bool,
    /// Вне учёта добычи: стоит ноль и не показывается в добыче. Нелегендарное
    /// снаряжение (из `gear.json`: цена сообщества — цена редких роллов)
    /// и «Флешбэки героя»; легендарки из `UNTRACKED_LOOT_IDS` — по номеру, и вне
    /// справочника. Выпадение базы хранится: опознанная легендарка
    /// подменяет её (`gear::identify`) и уже в учёте.
    untracked: bool,
}

struct PastSeason {
    own: PriceStore,
    community: Option<CommunityPriceFeed>,
}

struct ClosedRuns {
    all_runs: Vec<Run>,
    farming: Vec<Run>,
}

#[derive(Clone)]
struct SessionAgg {
    rated_runs: usize,
    prison_runs: usize,
    prison_fe: f64,
    prison_ms: i64,
    in_map_ms: i64,
    value_fe: f64,
    cost_fe: f64,
    fe_gained: i64,
    top: Vec<TopDrop>,
}

struct HistoryAggregates {
    version: u64,
    valid_until: i64,
    rate: RateSummary,
    fe_rate: RateSummary,
    norm: f64,
    recent: Vec<RunView>,
    strategies: Vec<StrategyView>,
    maps: Vec<MapStats>,
    days: Vec<DayTotals>,
    sessions: Vec<SessionTotals>,
    previous: Option<PeriodSummary>,
    other_season_runs: usize,
    player_runs: IndexMap<String, usize>,
    missing: MissingTally,
    unpriced_drops: usize,
    season_span: (f64, f64),
    session: SessionAgg,
}

fn tr(lang: Lang, ru: &str, en: &str) -> String {
    if lang == Lang::En { en } else { ru }.to_string()
}

/// Что дочитывать после старта: путь текущего лога и байт, с которого (None — с конца).
pub type TailFrom = (PathBuf, Option<u64>);

/// Адрес иконки для интерфейса: собственная схема, иконка — из локального кэша.
pub fn icon_url(id: ConfigBaseId) -> String {
    format!("gorn-icon://item/{id}.webp")
}

pub struct Engine {
    storage: Storage,
    prices: PriceStore,
    items: IndexMap<ConfigBaseId, ItemInfo>,
    diagnostics: Diagnostics,
    /// Ключи от карт по справочнику: их серверное списание — вложение в карту.
    map_keys: HashSet<ConfigBaseId>,
    parser: EventParser,
    inventory: InventoryTracker,
    segmenter: RunSegmenter,
    trade: TradeLedger,
    /// Что уже записано в базу: забег — с отпечатком (длительность и состав).
    persisted_runs: HashMap<String, String>,
    /// Записанная торговля: торговля только дописывается, поэтому — длины.
    persisted_trade: (usize, usize, usize),
    /// Цены на время одного среза: (сезон, предмет) → цена.
    price_memo: RefCell<Option<HashMap<(Option<i64>, ConfigBaseId), Option<Picked>>>>,
    /// Версия данных агрегатов истории.
    version: Cell<u64>,
    closed_cache: RefCell<Option<(u64, Rc<ClosedRuns>)>>,
    aggregates_cache: RefCell<Option<Rc<HistoryAggregates>>>,
    log_dir: Option<PathBuf>,
    season_id: Option<i64>,
    history: Vec<Run>,
    current_player: Option<PlayerProfile>,
    known_players: IndexMap<String, PlayerProfile>,
    player_filter: Option<String>,
    all_seasons: bool,
    bound_ids: Vec<ConfigBaseId>,
    session_started_at: i64,
    previous_session: Option<(i64, i64)>,
    sessions: Vec<SessionBounds>,
    excluded: IndexSet<ConfigBaseId>,
    min_drop_fe: f64,
    last_activity_at: i64,
    last_event_ts: i64,
    /// Метка последней строки лога игры — по ней идут часы сессии.
    last_line_at: i64,
    community: CommunityPriceFeed,
    /// Лента сообщила об обновлении: агрегаты пересобираются, срез отправляется.
    community_dirty: Arc<AtomicBool>,
    community_enabled: bool,
    data_dir: PathBuf,
    past_seasons: RefCell<HashMap<i64, PastSeason>>,
    language: NameLanguage,
    ui: Lang,
    maps: MapNames,
    booted: bool,
    stopped: bool,
    clock: Clock,
    sync_community: bool,
    community_url: Option<String>,
    /// Пора отправить срез (там, где эталон шлёт событие 'snapshot').
    emit: Cell<bool>,
    last_idle_hint: Option<String>,
    icon_base: String,
    use_instance_prices: bool,
    /// Цены экземпляров: (начало забега, зона, база) → ФЕ.
    instance_prices: HashMap<(i64, i64, ConfigBaseId), f64>,
    /// Легендарка → её база (справочник снаряжения).
    gear_bases: HashMap<ConfigBaseId, ConfigBaseId>,
}

/// Когда игра начала этот лог: метка первой строки со временем. Первые
/// метки идут в самом начале — хватает 256 КБ.
fn log_started_at(path: &Path) -> Option<i64> {
    use std::io::Read;
    let mut buf = vec![0u8; 256 * 1024];
    let n = std::fs::File::open(path).ok()?.read(&mut buf).ok()?;
    String::from_utf8_lossy(&buf[..n]).lines().find_map(crate::parser::parse_timestamp)
}

/// Метка последней строки лога: игра пишет пинг каждые несколько секунд.
fn log_last_line_at(path: &Path) -> Option<i64> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    f.seek(SeekFrom::Start(len.saturating_sub(64 * 1024))).ok()?;
    let mut buf = vec![];
    f.read_to_end(&mut buf).ok()?;
    String::from_utf8_lossy(&buf).lines().rev().find_map(crate::parser::parse_timestamp)
}

impl Engine {
    pub fn new(data_dir: &Path, items: &[CatalogueItem], options: EngineOptions, clock: Clock) -> rusqlite::Result<Self> {
        let storage = Storage::open(&data_dir.join("gorn.db"))?;
        let mut map = IndexMap::new();
        let mut map_keys = HashSet::new();
        for it in items {
            // Справочник предметов главнее справочника снаряжения.
            if it.gear && map.contains_key(&it.id) {
                continue;
            }
            let untracked = (it.gear && !it.legendary) || is_untracked_loot_type(it.type_cn.as_deref());
            map.insert(it.id, ItemInfo { name: it.name.clone(), name_ru: it.name_ru.clone(), icon: it.icon.clone(), gear: it.gear, untracked });
            if is_map_key_type(it.type_cn.as_deref()) {
                map_keys.insert(it.id);
            }
        }
        // Связанность — по названию из справочника, а не арифметикой «id + 1000».
        let bound_ids: Vec<ConfigBaseId> =
            items.iter().filter(|it| is_bound_name(Some(&it.name)) || is_bound_name(it.name_ru.as_deref())).map(|it| it.id).collect();
        let gear_bases = items.iter().filter_map(|it| Some((it.id, it.base?))).collect();
        let mut prices = PriceStore::new(None);
        prices.register_bound_ids(bound_ids.iter().copied());
        let mut community = CommunityPriceFeed::new(data_dir, "community-prices.json").with_clock(clock.clone());
        if let Some(u) = &options.community_url {
            community = community.with_base_url(u);
        }
        let community_dirty = Arc::new(AtomicBool::new(false));
        let flag = community_dirty.clone();
        community.on_update(move || flag.store(true, Ordering::SeqCst));
        Ok(Engine {
            storage,
            prices,
            items: map,
            diagnostics: Diagnostics::default(),
            map_keys,
            parser: EventParser::new(),
            inventory: InventoryTracker::new(),
            segmenter: RunSegmenter::new(),
            trade: TradeLedger::default(),
            persisted_runs: HashMap::new(),
            persisted_trade: (0, 0, 0),
            price_memo: RefCell::new(None),
            version: Cell::new(0),
            closed_cache: RefCell::new(None),
            aggregates_cache: RefCell::new(None),
            log_dir: None,
            season_id: None,
            history: vec![],
            current_player: None,
            known_players: IndexMap::new(),
            player_filter: options.player_filter,
            all_seasons: options.all_seasons,
            bound_ids,
            session_started_at: 0,
            previous_session: None,
            sessions: vec![],
            excluded: options.excluded_items.into_iter().collect(),
            min_drop_fe: options.min_drop_fe,
            last_activity_at: 0,
            last_event_ts: 0,
            last_line_at: 0,
            community,
            community_dirty,
            community_enabled: options.community_prices,
            data_dir: data_dir.to_path_buf(),
            past_seasons: RefCell::new(HashMap::new()),
            language: options.language,
            ui: options.ui,
            maps: options.maps.map(MapNames::new).unwrap_or_default(),
            booted: false,
            stopped: false,
            clock,
            sync_community: options.sync_community,
            community_url: options.community_url,
            emit: Cell::new(false),
            last_idle_hint: None,
            icon_base: options.icon_base.unwrap_or_else(|| "gorn-icon://item/".into()),
            use_instance_prices: options.instance_prices,
            instance_prices: HashMap::new(),
            gear_bases,
        })
    }

    fn now(&self) -> i64 {
        (self.clock)()
    }

    /// Пора отправить срез: сбрасывает признак.
    pub fn take_emit(&self) -> bool {
        self.emit.replace(false)
    }

    fn invalidate(&self) {
        self.version.set(self.version.get() + 1);
    }

    /// Изменение, после которого срез собирается заново целиком.
    fn rebuild(&self) {
        self.invalidate();
        self.emit.set(true);
    }

    /// Обновление цен сообщества (из потока ленты) — как rebuild эталона.
    fn sync_feed_updates(&self) {
        if self.community_dirty.swap(false, Ordering::SeqCst) {
            self.rebuild();
        }
    }

    /// Название карты: игровое из справочника на языке названий, иначе — очищенный ассет.
    fn map_name(&self, asset: Option<&str>, area_id: i64) -> String {
        match self.maps.lookup(asset, area_id) {
            None => asset_name(asset, area_id, self.ui),
            Some(k) => {
                if self.language == NameLanguage::En {
                    k.en
                } else {
                    k.ru.unwrap_or(k.en)
                }
            }
        }
    }

    pub fn set_language(&mut self, language: NameLanguage) {
        self.language = language;
        self.rebuild();
    }

    /// Язык интерфейса: подписи в срезе тоже.
    pub fn set_ui_language(&mut self, ui: Lang) {
        self.ui = ui;
        self.rebuild();
    }

    pub fn refresh(&self) {
        self.rebuild();
    }

    fn display_name(&self, id: ConfigBaseId) -> String {
        let Some(info) = self.items.get(&id) else { return format!("id {id}") };
        match (&info.name_ru, self.language) {
            (None, _) | (_, NameLanguage::En) => info.name.clone(),
            (Some(ru), NameLanguage::Both) => format!("{ru} · {}", info.name),
            (Some(ru), NameLanguage::Ru) => ru.clone(),
        }
    }

    fn untracked(&self, id: ConfigBaseId) -> bool {
        UNTRACKED_LOOT_IDS.contains(&id) || self.items.get(&id).is_some_and(|i| i.untracked)
    }

    fn icon(&self, id: ConfigBaseId) -> Option<String> {
        self.items.get(&id).and_then(|i| i.icon.as_ref()).map(|_| format!("{}{id}.webp", self.icon_base))
    }

    pub fn is_booted(&self) -> bool {
        self.booted
    }

    pub fn log_directory(&self) -> Option<&Path> {
        self.log_dir.as_deref()
    }

    /// Полный старт: разбор накопленных логов. Пока игра не найдена, можно звать повторно.
    /// Возвращает путь текущего лога и позицию, с которой его дочитывать.
    pub fn start(
        &mut self,
        log_dir_override: Option<&Path>,
        on_progress: Option<&mut dyn FnMut(&RetroScanProgress)>,
    ) -> Result<Option<TailFrom>, (Box<Snapshot>, String)> {
        if self.booted {
            return Ok(None);
        }
        // Вручную указанная папка могла исчезнуть — тогда ищем установку сами.
        let ov = log_dir_override.filter(|p| p.exists()).map(Path::to_path_buf);
        self.log_dir = ov.or_else(find_log_directory);
        let Some(dir) = self.log_dir.clone() else {
            let s = self.snapshot(Status::Searching, tr(self.ui, "Не нашёл установленную игру", "Game installation not found"));
            return Err((Box::new(s), "game not found".into()));
        };
        self.booted = true;
        match self.boot(&dir, on_progress) {
            Ok(tail) => Ok(Some(tail)),
            Err(e) => {
                self.booted = false;
                let msg = e.to_string();
                let hint = tr(
                    self.ui,
                    &format!("Не удалось прочитать папку логов: {msg}. Укажите папку игры в настройках."),
                    &format!("Could not read the log folder: {msg}. Pick the game folder in settings."),
                );
                Err((Box::new(self.snapshot(Status::Searching, hint)), msg))
            }
        }
    }

    fn boot(&mut self, log_dir: &Path, on_progress: Option<&mut dyn FnMut(&RetroScanProgress)>) -> Result<TailFrom, Box<dyn std::error::Error>> {
        self.session_started_at = self.storage.active_session_start()?;
        // Игру запустили после начала сессии — значит, это уже новая сессия.
        // Перезапуск самого Горна (обновление) счёт не сбрасывает.
        let current_log = log_dir.join("UE_game.log");
        if let Some(t) = log_started_at(&current_log) {
            if t > self.session_started_at {
                self.start_session_at(t);
            }
        }
        self.last_line_at = log_last_line_at(&current_log).unwrap_or(0);
        self.load_sessions();

        // Сначала сохранённое: логи ротируются, база — единственный источник старой истории.
        let stored = self.storage.load_runs()?;
        let stored_trade = self.storage.load_trade()?;
        let keys = self.map_keys.clone();
        let is_key = |id: ConfigBaseId| keys.contains(&id);
        let bases = &self.gear_bases;
        let base_of = |id: ConfigBaseId| bases.get(&id).copied();
        let gear_base: Option<crate::gear::GearBaseLookup> = self.use_instance_prices.then_some(&base_of);
        let scan = retro_scan(log_dir, on_progress, crate::pipeline::AnalyzeOptions { is_map_key: Some(&is_key), current: false, gear_base })?;
        // Сезон — из ПОСЛЕДНЕГО файла.
        self.season_id = scan.latest_season_id.or_else(|| scan.season_ids.last().copied());
        // Разбор логов точнее сохранённого: при совпадении — свежий.
        self.history = merge_runs(&[&stored, &scan.runs]);

        self.trade.listings.extend(merge_by_key(&[stored_trade.listings, scan.trade.listings.clone()], |l| {
            format!("{}:{}:{}", l.ts, l.config_base_id, l.qty)
        }));
        self.trade.receipts.extend(merge_by_key(&[stored_trade.receipts, scan.trade.receipts.clone()], |r| format!("{}:{}", r.ts, r.fe)));
        self.trade.recalls.extend(merge_by_key(&[stored_trade.recalls, scan.trade.recalls.clone()], recall_key));
        for (proto, n) in &scan.unknown_protos {
            self.diagnostics.unknown_protos.insert(proto.clone(), *n);
        }

        self.storage.save_runs(&scan.runs, self.season_id)?;
        self.storage.save_prices(scan.prices.observations())?;
        // Свои пробивки — только текущего сезона.
        self.load_season_prices()?;
        self.storage.save_trade(&scan.trade.listings, &scan.trade.receipts, &scan.trade.recalls)?;
        self.mark_trade_persisted();
        self.storage.save_players(&scan.players)?;
        if self.use_instance_prices {
            for p in self.storage.load_instance_prices()? {
                self.instance_prices.insert((p.run_started_at, p.area_id, p.config_base_id), p.fe);
            }
        }
        for p in self.storage.load_players()? {
            self.known_players.insert(p.player_id.clone(), p);
        }

        // Нарезка текущего лога продолжается с того места, где её оставил разбор.
        if let Some(st) = &scan.segmenter {
            self.segmenter.import_state(st);
        }
        if let Some(pid) = &scan.last_player_id {
            self.current_player = self.known_players.get(pid).cloned();
            self.segmenter.set_player(Some(pid.clone()));
            let season = self.current_player.as_ref().and_then(|p| p.season_id).or(self.season_id.map(|s| s as f64));
            self.segmenter.set_season(season);
        }
        // Состояние сумки из разбора: иначе расход на первую карту уйдёт на установку базы.
        self.inventory.import_slots(&scan.slots);

        if self.community_enabled {
            if let Some(s) = self.season_id {
                self.start_community(s);
            }
        }
        self.rebuild();
        // Дочитывается всегда UE_game.log по фиксированному пути — с байта, где закончил разбор.
        Ok(match scan.current_log {
            Some((p, pos)) => (p, Some(pos)),
            None => (log_dir.join("UE_game.log"), None),
        })
    }

    fn start_community(&self, season: i64) {
        if self.sync_community {
            self.community.start_sync(season);
        } else {
            self.community.start(season);
        }
    }

    /// Пульс раз в 5 с: в карте — всегда срез (таймер идёт), вне её — когда меняется подсказка.
    pub fn heartbeat(&mut self) -> bool {
        self.sync_feed_updates();
        let open = self.segmenter.current();
        if open.is_some_and(|r| !r.is_hub) {
            self.emit.set(true);
            return true;
        }
        let hint = self.hint_for(Status::Idle);
        if hint != self.last_idle_hint {
            self.last_idle_hint = hint;
            self.emit.set(true);
            return true;
        }
        self.emit.get()
    }

    /// Игра перезапущена: забеги сессии переносятся в историю, состояние — заново.
    pub fn on_log_rotate(&mut self) {
        let last = if self.last_event_ts != 0 { self.last_event_ts } else { self.now() };
        let finished = self.segmenter.finish(last);
        if !finished.is_empty() {
            let _ = self.storage.save_runs(&finished, self.season_id);
            self.history = merge_runs(&[&self.history, &finished]);
        }
        self.parser = EventParser::new();
        self.inventory = InventoryTracker::new();
        self.segmenter = RunSegmenter::new();
        self.current_player = None;
        // Новый лог — игра запущена заново: новая сессия.
        let now = self.now();
        self.start_session_at(now);
        self.load_sessions();
        self.rebuild();
    }

    /// Строки, дописанные игрой.
    pub fn consume(&mut self, lines: &[String]) {
        self.sync_feed_updates();
        if let Some(t) = lines.iter().rev().find_map(|l| crate::parser::parse_timestamp(l)) {
            self.last_line_at = t;
        }
        let mut changed = false;
        for raw in lines {
            self.diagnostics.lines_total += 1;
            if let Some(m) = patterns().season_id.captures(raw) {
                let season = digits(&m[1]);
                if Some(season) != self.season_id {
                    // Смена сезона на лету: цены прошлого сезона неприменимы.
                    self.season_id = Some(season);
                    let _ = self.load_season_prices();
                    if self.community_enabled {
                        self.start_community(season);
                    }
                }
            }
            let Some(ev) = self.parser.parse(raw) else { continue };
            // Улика легендарки опознаёт выпавшую базу — раньше, чем её цена ляжет на экземпляр.
            self.identify_gear(&ev);
            if matches!(ev, LogEvent::GearIdentity { .. }) {
                continue;
            }
            // Окна механик — только для начала их забега, не событие для счётчиков.
            if let LogEvent::Page { ts, page, open, .. } = &ev {
                self.segmenter.page(*ts, page, *open);
                continue;
            }
            // Пробивка с фильтрами — не событие для счётчиков (как у эталона),
            // но цена экземпляра, если она включена.
            if let LogEvent::FilteredPrice(p) = &ev {
                self.on_filtered_price(p);
                continue;
            }
            self.diagnostics.lines_matched += 1;
            changed = true;
            self.last_activity_at = self.now();
            if ev.ts() > 0 {
                self.last_event_ts = ev.ts();
            }
            let keys = &self.map_keys;
            let km = |id: ConfigBaseId| keys.contains(&id);
            match ev {
                LogEvent::ItemChange { proto, start, .. } => {
                    if start {
                        self.inventory.begin_transaction(&proto, &mut self.diagnostics);
                    } else {
                        self.inventory.end_transaction();
                    }
                }
                LogEvent::BagInit { page_id, slot_id, config_base_id, num, .. } => self.inventory.apply_init(page_id, slot_id, config_base_id, num),
                LogEvent::BagReset { page_id, .. } => self.inventory.apply_reset(page_id),
                LogEvent::BagAdd { page_id, slot_id, config_base_id, .. } => self.inventory.apply_add(page_id, slot_id, config_base_id),
                LogEvent::BagRemove { ts, page_id, slot_id, .. } => {
                    if let Some(d) = self.inventory.apply_remove(ts, page_id, slot_id, Some(&km)) {
                        self.segmenter.add_delta(&d);
                        self.trade.add(&d);
                    }
                }
                LogEvent::BagModify { ts, page_id, slot_id, config_base_id, num, .. } => {
                    if let Some(d) = self.inventory.apply_modify(ts, page_id, slot_id, config_base_id, num, &mut self.diagnostics, Some(&km)) {
                        self.segmenter.add_delta(&d);
                        self.trade.add(&d);
                    }
                }
                LogEvent::LevelPath { asset, .. } => self.segmenter.note_asset(&asset),
                LogEvent::LevelIds { level_type, .. } => self.segmenter.note_level_type(level_type),
                LogEvent::EnterLevel { ts, area_id, .. } => {
                    // Прошлая зона закрылась — или закрытый забег снова открыт порталом.
                    self.segmenter.enter_level(ts, area_id);
                    self.invalidate();
                    self.persist_finished();
                }
                LogEvent::FilteredPrice(_) | LogEvent::GearIdentity { .. } | LogEvent::Page { .. } => {}
                LogEvent::PriceObservation(p) => {
                    let season = self.prices.season();
                    if let Some(record) = self.prices.observe(&p, season) {
                        let _ = self.storage.save_prices(&[record]);
                        self.invalidate();
                    }
                }
                LogEvent::PlayerInfo(p) => {
                    let known = self.known_players.get(&p.player_id).cloned();
                    let cur = merge_player_info(known.as_ref(), &p);
                    // По сезону персонажа атрибутируются старые забеги и работает фильтр.
                    if known.as_ref().is_none_or(|k| k.season_id != cur.season_id) {
                        self.invalidate();
                    }
                    self.known_players.insert(p.player_id.clone(), cur.clone());
                    self.current_player = Some(cur.clone());
                    self.segmenter.set_player(Some(p.player_id.clone()));
                    if p.season_id.is_some() {
                        self.segmenter.set_season(p.season_id);
                    }
                    let _ = self.storage.save_players(&[cur]);
                }
            }
        }
        if changed {
            self.emit.set(true);
        }
    }

    /// Свои пробивки текущего сезона; цены прошлого сезона не подмешиваются.
    fn load_season_prices(&mut self) -> rusqlite::Result<()> {
        self.prices = PriceStore::new(self.season_id);
        self.prices.register_bound_ids(self.bound_ids.iter().copied());
        for r in self.storage.load_prices(self.season_id)? {
            self.prices.set(r.config_base_id, r.fe, r.observed_at);
        }
        // Сезон сменился: прошлый поднимется из базы заново.
        for past in self.past_seasons.borrow().values() {
            if let Some(c) = &past.community {
                c.stop();
            }
        }
        self.past_seasons.borrow_mut().clear();
        self.invalidate();
        Ok(())
    }

    /// Цены прошлого сезона: свои пробивки того сезона и база сообщества за тот же сезон.
    fn ensure_past_season(&self, season: i64) {
        let mut map = self.past_seasons.borrow_mut();
        let past = map.entry(season).or_insert_with(|| {
            let mut own = PriceStore::new(Some(season));
            own.register_bound_ids(self.bound_ids.iter().copied());
            for r in self.storage.load_prices(Some(season)).unwrap_or_default() {
                own.set(r.config_base_id, r.fe, r.observed_at);
            }
            PastSeason { own, community: None }
        });
        if self.community_enabled && past.community.is_none() {
            let mut feed =
                CommunityPriceFeed::new(&self.data_dir, &format!("community-prices-{season}.json")).with_clock(self.clock.clone());
            if let Some(u) = &self.community_url {
                feed = feed.with_base_url(u);
            }
            let flag = self.community_dirty.clone();
            feed.on_update(move || flag.store(true, Ordering::SeqCst));
            past.community = Some(feed.clone());
            drop(map);
            if self.sync_community {
                feed.load_once_sync(season);
            } else {
                feed.load_once(season);
            }
        }
    }

    /// Цена предмета: своя пробивка, пока свежая, иначе база сообщества, если новее.
    /// У прошлого сезона «свежей» пробивки не бывает: берётся та, что новее.
    fn pick_price(&self, id: ConfigBaseId, season: Option<i64>) -> Option<Picked> {
        if let Some(memo) = self.price_memo.borrow().as_ref() {
            if let Some(p) = memo.get(&(season, id)) {
                return *p;
            }
        }
        let picked = self.pick_price_now(id, season);
        if let Some(memo) = self.price_memo.borrow_mut().as_mut() {
            memo.insert((season, id), picked);
        }
        picked
    }

    fn pick_price_now(&self, id: ConfigBaseId, season: Option<i64>) -> Option<Picked> {
        if self.untracked(id) {
            return Some(Picked { fe: 0.0, source: ViewPriceSource::Excluded });
        }
        if self.prices.is_fixed(id) {
            return Some(Picked { fe: self.prices.price_of(id).unwrap_or(0.0), source: ViewPriceSource::Own });
        }
        if self.excluded.contains(&id) {
            return Some(Picked { fe: 0.0, source: ViewPriceSource::Excluded });
        }
        let past = season.filter(|s| Some(*s) != self.season_id);
        let (own, feed) = match past {
            Some(s) => {
                self.ensure_past_season(s);
                let map = self.past_seasons.borrow();
                let p = &map[&s];
                (p.own.get(id).map(|r| (r.fe, r.observed_at)), p.community.clone())
            }
            None => (self.prices.get(id).map(|r| (r.fe, r.observed_at)), Some(self.community.clone())),
        };
        let community = if self.community_enabled { feed.and_then(|f| f.price_of(id)) } else { None };
        let updated = community.as_ref().map(|c| c.updated_at.clone());
        let updated_ms = move || updated.as_deref().and_then(parse_iso_ms).map(|t| t as f64).unwrap_or(f64::NAN);
        let community_ref = community.as_ref().map(|c| community_reference(c.fe, c.p10, self.items.get(&id).is_some_and(|i| !i.gear)));
        let now = if past.is_some() { i64::MAX } else { self.now() };
        choose_price(own, community_ref.map(|fe| (fe, &updated_ms as &dyn Fn() -> f64)), now, OWN_PRICE_TTL_MS)
            .map(|(fe, s)| Picked { fe, source: s.into() })
    }

    fn price_of(&self, id: ConfigBaseId) -> Option<f64> {
        self.pick_price(id, self.season_id).map(|p| p.fe)
    }

    fn price_source_of(&self, id: ConfigBaseId, season: Option<i64>) -> Option<ViewPriceSource> {
        self.pick_price(id, season).map(|p| p.source)
    }

    /// Цена этого экземпляра в этом забеге (пробивка с фильтрами), если есть.
    /// Исключённый из учёта предмет стоит ноль и здесь.
    fn instance_price(&self, run: &Run, id: ConfigBaseId) -> Option<f64> {
        if self.instance_prices.is_empty() || self.excluded.contains(&id) || self.untracked(id) {
            return None;
        }
        self.instance_prices.get(&(run.started_at, run.area_id, id)).copied()
    }

    /// Пробивка снаряжения с фильтрами по аффиксам: цена ролла — последнему
    /// выпавшему экземпляру этой базы (до пробивки), а не всей базе.
    fn on_filtered_price(&mut self, obs: &PriceObservation) {
        if !self.use_instance_prices || obs.currency != FE_ID {
            return;
        }
        let fe = crate::prices::reference_price(&obs.unit_prices);
        if fe <= 0.0 {
            return;
        }
        let id = obs.config_base_id;
        let mut best: Option<(i64, i64, i64)> = None; // (время дропа, начало забега, зона)
        for run in self.segmenter.current().into_iter().chain(self.segmenter.closed()).chain(self.history.iter()) {
            for d in &run.drops {
                if d.config_base_id == id && d.ts <= obs.ts && best.is_none_or(|b| d.ts > b.0) {
                    best = Some((d.ts, run.started_at, run.area_id));
                }
            }
        }
        let Some((_, started, area)) = best else { return };
        self.instance_prices.insert((started, area, id), fe);
        let _ = self.storage.save_instance_price(&crate::storage::InstancePrice {
            run_started_at: started,
            area_id: area,
            config_base_id: id,
            fe,
            observed_at: obs.ts,
        });
        self.rebuild();
    }

    /// Пробивка или сравнение легендарки: последний выпавший экземпляр её базы
    /// становится ею — с её названием и ценой. Изменённые забеги истории
    /// перезаписываются сразу, закрытые нарезкой — при следующей записи.
    fn identify_gear(&mut self, ev: &LogEvent) {
        if !self.use_instance_prices {
            return;
        }
        let bases = &self.gear_bases;
        let base_of = |id: ConfigBaseId| bases.get(&id).copied();
        let Some((base, unique, ts)) = crate::gear::evidence(ev, &base_of) else { return };
        let runs = self.segmenter.runs_mut().chain(self.history.iter_mut());
        let changed = crate::gear::identify(runs, base, unique, ts, &base_of);
        if changed.is_empty() {
            return;
        }
        let stored: Vec<Run> = self.history.iter().filter(|r| changed.contains(&(r.started_at, r.area_id))).cloned().collect();
        if !stored.is_empty() {
            let _ = self.storage.save_runs(&stored, self.season_id);
        }
        self.persist_finished();
        self.rebuild();
    }

    /// Таблица цен сезона; забеги без сезона — текущими ценами.
    fn season_lookup(&self, season: Option<i64>) -> Box<dyn Fn(ConfigBaseId) -> Option<f64> + '_> {
        match season {
            Some(s) if Some(s) != self.season_id => {
                self.ensure_past_season(s);
                Box::new(move |id| self.pick_price(id, Some(s)).map(|p| p.fe))
            }
            _ => Box::new(|id| self.price_of(id)),
        }
    }

    /// Цены для видимых забегов: в обычном виде — одна таблица текущего сезона.
    fn run_pricing(&self) -> Box<dyn Fn(&Run, ConfigBaseId) -> Option<f64> + '_> {
        if self.player_filter.is_none() && !self.all_seasons {
            return Box::new(|run, id| self.instance_price(run, id).or_else(|| self.price_of(id)));
        }
        Box::new(|run, id| self.instance_price(run, id).or_else(|| self.season_lookup(self.season_of(run))(id)))
    }

    /// Закрытые забеги сразу уходят в базу: лог может ротироваться в любой момент.
    fn persist_finished(&mut self) {
        // Сумма номеров — опознанная легендарка меняет состав, а не число строк.
        let stamp = |r: &Run| {
            let ids: i64 = r.drops.iter().map(|d| d.config_base_id).sum();
            format!("{:?}:{}:{}:{ids}", r.duration_ms, r.drops.len(), r.costs.len())
        };
        let finished: Vec<Run> = self
            .segmenter
            .closed()
            .iter()
            .filter(|r| r.ended_at.is_some() && self.persisted_runs.get(&run_key(r)) != Some(&stamp(r)))
            .cloned()
            .collect();
        if !finished.is_empty() {
            let _ = self.storage.save_runs(&finished, self.season_id);
            for r in &finished {
                self.persisted_runs.insert(run_key(r), stamp(r));
            }
        }
        let (lp, rp, cp) = self.persisted_trade;
        let listings = &self.trade.listings[lp.min(self.trade.listings.len())..];
        // Выплаты одной миллисекунды нумеруются по порядку — пишем их вместе с записанными соседями.
        let fresh_ts: HashSet<i64> = self.trade.receipts[rp.min(self.trade.receipts.len())..].iter().map(|r| r.ts).collect();
        let receipts: Vec<_> = self.trade.receipts.iter().filter(|r| fresh_ts.contains(&r.ts)).cloned().collect();
        let fresh_recalls: HashSet<i64> = self.trade.recalls[cp.min(self.trade.recalls.len())..].iter().map(|r| r.ts).collect();
        let recalls: Vec<_> = self.trade.recalls.iter().filter(|r| fresh_recalls.contains(&r.ts)).cloned().collect();
        if listings.is_empty() && receipts.is_empty() && recalls.is_empty() {
            return;
        }
        let _ = self.storage.save_trade(listings, &receipts, &recalls);
        self.mark_trade_persisted();
    }

    fn mark_trade_persisted(&mut self) {
        self.persisted_trade = (self.trade.listings.len(), self.trade.receipts.len(), self.trade.recalls.len());
    }

    /// Предметы, которые игроку уже падали, которые он выставлял или пробивал.
    pub fn seen_item_ids(&self) -> Vec<ConfigBaseId> {
        let mut ids = IndexSet::new();
        for run in self.history.iter().chain(self.segmenter.closed()).chain(self.segmenter.current()) {
            for d in run.drops.iter().chain(&run.costs) {
                ids.insert(d.config_base_id);
            }
        }
        for l in &self.trade.listings {
            ids.insert(l.config_base_id);
        }
        for p in self.prices.all() {
            ids.insert(p.config_base_id);
        }
        ids.into_iter().collect()
    }

    /// Имя забега: план с меткой из маяка, которым открыли карту, и сама карта.
    fn run_name(&self, run: &Run) -> String {
        let map = self.map_name(run.asset.as_deref(), run.area_id);
        for cost in &run.costs {
            let Some(info) = self.items.get(&cost.config_base_id) else { continue };
            let plane = plane_name(info.name_ru.as_deref().unwrap_or(&info.name), self.ui).or_else(|| plane_name(&info.name, self.ui));
            let Some(plane) = plane else { continue };
            return if run.asset.is_some() { format!("{plane} · {map}") } else { plane };
        }
        map
    }

    /// Имя стратегии: чем открывалась карта; маяк сокращается до плана и метки.
    fn strategy_title(&self, cost_items: &[ConfigBaseId]) -> String {
        if cost_items.is_empty() {
            return tr(self.ui, "Без вложений", "No map costs");
        }
        cost_items
            .iter()
            .map(|&id| match self.items.get(&id) {
                None => format!("id {id}"),
                Some(info) => plane_name(info.name_ru.as_deref().unwrap_or(&info.name), self.ui).unwrap_or_else(|| self.display_name(id)),
            })
            .collect::<Vec<_>>()
            .join(" + ")
    }

    /// Что показывать: персонажа, текущий сезон или все сезоны — одним действием.
    pub fn set_run_scope(&mut self, player_id: Option<String>, all_seasons: bool) {
        self.player_filter = player_id;
        self.all_seasons = all_seasons;
        self.rebuild();
    }

    /// Сезон забега: свой, а у старых записей — сезон их персонажа.
    fn season_of(&self, run: &Run) -> Option<i64> {
        if let Some(s) = run.season_id {
            return Some(s as i64);
        }
        let pid = run.player_id.as_ref()?;
        self.known_players.get(pid).and_then(|p| p.season_id).map(|s| s as i64)
    }

    /// Сезон, в границах которого считаются цифры; None — все сезоны.
    fn scope_season(&self) -> Option<i64> {
        if let Some(pid) = &self.player_filter {
            return self.known_players.get(pid).and_then(|p| p.season_id).map(|s| s as i64);
        }
        if self.all_seasons {
            None
        } else {
            self.season_id
        }
    }

    fn is_current_season(&self, run: &Run) -> bool {
        let s = self.season_of(run);
        s.is_none() || s == self.season_id
    }

    fn in_scope(&self, run: &Run) -> bool {
        if let Some(pid) = &self.player_filter {
            return run.player_id.as_ref() == Some(pid);
        }
        if self.all_seasons || self.season_id.is_none() {
            return true;
        }
        self.season_of(run) == self.season_id
    }

    /// Новая сессия: счётчики с этого момента, история не трогается.
    pub fn reset_session(&mut self) {
        let now = self.now();
        self.start_session_at(now);
        self.load_sessions();
        self.rebuild();
    }

    fn start_session_at(&mut self, t: i64) {
        if let Ok(t) = self.storage.reset_session(t) {
            self.session_started_at = t;
        }
    }

    fn load_sessions(&mut self) {
        self.previous_session = self.storage.previous_session().ok().flatten();
        self.sessions = self
            .storage
            .list_sessions()
            .unwrap_or_default()
            .into_iter()
            .map(|s| SessionBounds { started_at: s.started_at, ended_at: s.ended_at })
            .collect();
    }

    /// Поиск по всей истории под текущим фильтром: итоги — по всем совпадениям.
    pub fn query_runs(&self, query: &str) -> RunQueryResult {
        self.sync_feed_updates();
        let needle = query.trim().to_lowercase();
        let (closed, _, _) = self.visible_runs();
        let matched: Vec<&Run> = closed.farming.iter().filter(|r| needle.is_empty() || self.search_text(r).contains(&needle)).collect();
        let (mut net_fe, mut in_map_ms) = (0.0, 0i64);
        let (mut rates, mut nets, mut rated_net) = (vec![], vec![], 0.0);
        let pricing = self.run_pricing();
        for run in &matched {
            let net = run_net(run, &*pricing);
            net_fe += net;
            // Тюрьма — в итоге, но не в темпе: её время лог не видит.
            if is_prison_run(run) {
                continue;
            }
            let active = active_duration(run, IDLE_CAP_MS, None);
            in_map_ms += active;
            rates.push(per_hour(net, active as f64));
            nets.push(net);
            rated_net += net;
        }
        let start = matched.len().saturating_sub(RUN_QUERY_LIMIT);
        RunQueryResult {
            total: matched.len(),
            net_fe,
            in_map_ms,
            rate: summarize_timed(&rates, Some(&nets), rated_net, in_map_ms),
            runs: matched[start..].iter().rev().map(|r| self.to_view(r)).collect(),
        }
    }

    /// Всё, по чему забег находится поиском, одной строкой в нижнем регистре.
    fn search_text(&self, run: &Run) -> String {
        let mut parts = vec![self.run_name(run), asset_name(run.asset.as_deref(), run.area_id, self.ui)];
        if let Some(m) = self.maps.lookup(run.asset.as_deref(), run.area_id) {
            parts.push(m.en);
            parts.push(m.ru.unwrap_or_default());
        }
        for d in run.drops.iter().chain(&run.costs) {
            let Some(info) = self.items.get(&d.config_base_id) else { continue };
            parts.push(info.name.clone());
            if let Some(ru) = &info.name_ru {
                if !ru.is_empty() {
                    parts.push(ru.clone());
                }
            }
        }
        if run.costs.is_empty() {
            parts.push("без вложений".into());
            parts.push("no map costs".into());
        }
        parts.join("\n").to_lowercase()
    }

    /// Видимые забеги в CSV для Excel: хронологически, строка на забег.
    pub fn export_runs_csv(&self) -> String {
        self.sync_feed_updates();
        let (closed, _, _) = self.visible_runs();
        let pricing = self.run_pricing();
        let stamp = |ts: i64| {
            use chrono::TimeZone;
            chrono::Local.timestamp_millis_opt(ts).single().map(|d| d.format("%Y-%m-%d %H:%M").to_string()).unwrap_or_default()
        };
        let items = |deltas: &[ItemDelta], season: Option<i64>| {
            self.item_views(deltas, season, None).iter().map(|v| format!("{} ×{}", v.name, v.qty)).collect::<Vec<_>>().join(", ")
        };
        let ui = self.ui;
        let header: Vec<String> = [
            ("Начало", "Started"),
            ("Персонаж", "Character"),
            ("Карта", "Map"),
            ("Стратегия", "Strategy"),
            ("В карте, мин", "In map, min"),
            ("Добыча, ФЕ", "Loot, FE"),
            ("Вложено, ФЕ", "Invested, FE"),
            ("Чистыми, ФЕ", "Net, FE"),
            ("Чистыми в час, ФЕ", "Net per hour, FE"),
            ("Валюта ФЕ", "FE currency"),
            ("Выпало", "Dropped"),
            ("Потрачено", "Spent"),
        ]
        .iter()
        .map(|(ru, en)| tr(ui, ru, en))
        .collect();
        let rows: Vec<Vec<CsvCell>> = closed
            .farming
            .iter()
            .map(|r| {
                let active = active_duration(r, IDLE_CAP_MS, None);
                let net = run_net(r, &*pricing);
                let player = r.player_id.as_ref().and_then(|p| self.known_players.get(p));
                let who = player.map(|p| {
                    let v = to_player_view(p, ui);
                    let mut parts = vec![];
                    if let Some(n) = v.hero_name.or(v.name).filter(|s| !s.is_empty()) {
                        parts.push(n);
                    }
                    if let Some(l) = v.level {
                        let l = js_num_str(l);
                        parts.push(tr(ui, &format!("{l} ур."), &format!("lvl {l}")));
                    }
                    parts.join(" ")
                });
                let ids: Vec<ConfigBaseId> = r.costs.iter().map(|c| c.config_base_id).collect::<std::collections::BTreeSet<_>>().into_iter().collect();
                vec![
                    CsvCell::Text(stamp(r.started_at)),
                    who.map(CsvCell::Text).unwrap_or(CsvCell::Null),
                    CsvCell::Text(self.run_name(r)),
                    CsvCell::Text(self.strategy_title(&ids)),
                    CsvCell::Num(active as f64 / 60_000.0),
                    CsvCell::Num(run_value(r, &*pricing)),
                    CsvCell::Num(run_cost(r, &*pricing)),
                    CsvCell::Num(net),
                    CsvCell::Num(per_hour(net, active as f64)),
                    CsvCell::Num(run_fe(r) as f64),
                    CsvCell::Text(items(&r.drops, self.season_of(r))),
                    CsvCell::Text(items(&r.costs, self.season_of(r))),
                ]
            })
            .collect();
        to_csv(&header, &rows, ui)
    }

    /// Исключение предмета из учёта: его добыча стоит ноль во всех цифрах.
    pub fn set_excluded(&mut self, id: ConfigBaseId, excluded: bool) {
        if excluded {
            self.excluded.insert(id);
        } else {
            self.excluded.shift_remove(&id);
        }
        self.rebuild();
    }

    /// Порог «мелочи» в списках добычи — только отображение.
    pub fn set_min_drop_fe(&mut self, value: f64) {
        self.min_drop_fe = value.max(0.0);
        self.emit.set(true);
    }

    fn bag_view(&self) -> Option<BagView> {
        let contents = self.inventory.snapshot();
        if contents.is_empty() {
            return None;
        }
        let bag = bag_value(&contents, &|id| self.price_of(id), 6);
        Some(BagView {
            value_fe: bag.value_fe,
            fe_cash: bag.fe_cash,
            unpriced: bag.unpriced,
            top: bag
                .top
                .into_iter()
                .map(|i| BagItemView { name: self.display_name(i.config_base_id), icon: self.icon(i.config_base_id), config_base_id: i.config_base_id, qty: i.qty, total_fe: i.total_fe })
                .collect(),
        })
    }

    /// Сумка по слотам с ценой каждого: за штуку и за стопку.
    pub fn query_bag(&self) -> BagGrid {
        self.sync_feed_updates();
        let mut pages: Vec<BagPageView> = vec![];
        for (page_id, slot_id, id, qty) in self.inventory.occupied() {
            if pages.last().is_none_or(|p| p.page_id != page_id) {
                pages.push(BagPageView { page_id, value_fe: 0.0, unpriced: 0, slots: vec![] });
            }
            let page = pages.last_mut().unwrap();
            let picked = self.pick_price(id, self.season_id);
            let unit = picked.map(|p| p.fe);
            let known = self.items.contains_key(&id);
            match unit {
                Some(u) => page.value_fe += u * qty as f64,
                // Вне справочника — непродаваемое (ресурсы сезона, связанное): оценку не занижает.
                None if !known => {}
                None => page.unpriced += 1,
            }
            page.slots.push(BagSlotView {
                slot_id,
                known,
                item: DropView {
                    config_base_id: id,
                    name: self.display_name(id),
                    icon: self.icon(id),
                    qty,
                    unit_fe: unit,
                    total_fe: unit.map(|u| u * qty as f64),
                    price_source: picked.map(|p| p.source),
                },
            });
        }
        BagGrid { pages }
    }

    /// Цены с историей своих пробивок: пустой запрос — что падает, по влиянию, плюс исключённое.
    pub fn query_prices(&self, query: &str) -> Vec<PriceRow> {
        self.sync_feed_updates();
        const LIMIT: usize = 40;
        let needle = query.trim().to_lowercase();
        let mut seen: IndexMap<ConfigBaseId, i64> = IndexMap::new();
        let (closed, _, _) = self.visible_runs();
        for run in &closed.farming {
            for d in run.drops.iter().chain(&run.costs).filter(|d| !self.untracked(d.config_base_id)) {
                *seen.entry(d.config_base_id).or_insert(0) += d.qty;
            }
        }
        let ids: Vec<ConfigBaseId> = if needle.is_empty() {
            let impact = |id: ConfigBaseId| self.price_of(id).unwrap_or(0.0) * seen.get(&id).copied().unwrap_or(0) as f64;
            let mut rest: Vec<ConfigBaseId> = seen.keys().copied().filter(|&id| id != FE_ID && !self.excluded.contains(&id)).collect();
            rest.sort_by(|&a, &b| {
                let d = impact(b) - impact(a);
                let d = if d == 0.0 { (seen.get(&b).copied().unwrap_or(0) - seen.get(&a).copied().unwrap_or(0)) as f64 } else { d };
                d.partial_cmp(&0.0).unwrap_or(std::cmp::Ordering::Equal)
            });
            self.excluded.iter().copied().chain(rest).collect()
        } else {
            let mut matches: Vec<ConfigBaseId> = self
                .items
                .iter()
                .filter(|(id, info)| **id != FE_ID && !self.untracked(**id) && format!("{}\n{}", info.name, info.name_ru.as_deref().unwrap_or("")).to_lowercase().contains(&needle))
                .map(|(id, _)| *id)
                .collect();
            let rank = |id: ConfigBaseId| (if seen.contains_key(&id) { 2 } else { 0 }) + (if self.price_of(id).is_some() { 1 } else { 0 });
            matches.sort_by_key(|&id| std::cmp::Reverse(rank(id)));
            matches
        };
        ids.into_iter()
            .take(LIMIT)
            .map(|id| {
                let picked = self.pick_price(id, self.season_id);
                PriceRow {
                    config_base_id: id,
                    name: self.display_name(id),
                    icon: self.icon(id),
                    fe: picked.map(|p| p.fe),
                    source: picked.map(|p| p.source),
                    own_observed_at: self.prices.get(id).map(|r| r.observed_at),
                    trend: price_trend(&self.storage.price_history(id, 60, self.season_id).unwrap_or_default()),
                    seen_qty: seen.get(&id).copied().unwrap_or(0),
                }
            })
            .collect()
    }

    /// Включение и отключение цен сообщества на лету.
    pub fn set_community_prices(&mut self, enabled: bool) {
        self.community_enabled = enabled;
        match self.season_id {
            Some(s) if enabled => self.start_community(s),
            _ => self.community.stop(),
        }
        self.rebuild();
    }

    fn to_view(&self, run: &Run) -> RunView {
        let fe_gained = run_fe(run);
        // Забег прошлого сезона — по ценам того сезона, и строки добычи тоже.
        let season = self.season_of(run);
        let price_of = self.season_lookup(season);
        let pricing = |r: &Run, id: ConfigBaseId| self.instance_price(r, id).or_else(|| price_of(id));
        let value_fe = run_value(run, &pricing);
        let cost_fe = run_cost(run, &pricing);
        let net_fe = value_fe - cost_fe;
        let active = active_duration(run, IDLE_CAP_MS, if run.ended_at.is_none() { Some(self.now()) } else { None });
        RunView {
            key: run_key(run),
            area_id: run.area_id,
            name: self.run_name(run),
            started_at: run.started_at,
            duration_ms: run.duration_ms,
            active_ms: active,
            fe_gained,
            value_fe,
            cost_fe,
            net_fe,
            per_hour: per_hour(net_fe, active as f64),
            drops: self.item_views(&run.drops, season, Some(run)),
            costs: self.item_views(&run.costs, season, Some(run)),
            prison: is_prison_run(run).then_some(true),
        }
    }

    /// Строки предметов: одинаковые сложены, дорогие сверху; вне учёта добычи — без строк.
    fn item_views(&self, deltas: &[ItemDelta], season: Option<i64>, run: Option<&Run>) -> Vec<DropView> {
        let price_of = self.season_lookup(season);
        let instance = |id: ConfigBaseId| run.and_then(|r| self.instance_price(r, id));
        let mut by_item: IndexMap<ConfigBaseId, i64> = IndexMap::new();
        for d in deltas.iter().filter(|d| !self.untracked(d.config_base_id)) {
            *by_item.entry(d.config_base_id).or_insert(0) += d.qty;
        }
        let mut v: Vec<DropView> = by_item
            .into_iter()
            .map(|(id, qty)| {
                let own = instance(id);
                let unit = own.or_else(|| price_of(id));
                DropView {
                    config_base_id: id,
                    name: self.display_name(id),
                    icon: self.icon(id),
                    qty,
                    unit_fe: unit,
                    total_fe: unit.map(|u| u * qty as f64),
                    price_source: if own.is_some() { Some(ViewPriceSource::Own) } else { self.price_source_of(id, season) },
                }
            })
            .collect();
        v.sort_by(|a, b| b.total_fe.unwrap_or(-1.0).partial_cmp(&a.total_fe.unwrap_or(-1.0)).unwrap_or(std::cmp::Ordering::Equal));
        v
    }

    /// Видимые забеги: закрытые под фильтром, открытый живой и он же — если виден.
    fn visible_runs(&self) -> (Rc<ClosedRuns>, Option<&Run>, Option<&Run>) {
        let live_open = self.segmenter.current();
        let open = live_open.filter(|r| self.in_scope(r));
        (self.closed_runs(), live_open, open)
    }

    /// Закрытые забеги под фильтром — пересобираются только при новой версии.
    fn closed_runs(&self) -> Rc<ClosedRuns> {
        if let Some((v, c)) = self.closed_cache.borrow().as_ref() {
            if *v == self.version.get() {
                return c.clone();
            }
        }
        // Забег, вновь открытый порталом, лежит в истории закрытой копией — пока открыт, не считается.
        let open_key = self.segmenter.current().map(run_key);
        let all_runs: Vec<Run> = merge_runs(&[&self.history, self.segmenter.closed()])
            .into_iter()
            .filter(|r| Some(run_key(r)) != open_key)
            .collect();
        let farming = all_runs.iter().filter(|r| self.in_scope(r) && is_farming_run(r)).cloned().collect();
        let c = Rc::new(ClosedRuns { all_runs, farming });
        *self.closed_cache.borrow_mut() = Some((self.version.get(), c.clone()));
        c
    }

    /// Срез для интерфейса. Выбор цен запоминается на время среза.
    pub fn build_snapshot(&self) -> Snapshot {
        self.sync_feed_updates();
        if self.price_memo.borrow().is_some() {
            return self.build_snapshot_now();
        }
        *self.price_memo.borrow_mut() = Some(HashMap::new());
        let s = self.build_snapshot_now();
        *self.price_memo.borrow_mut() = None;
        s
    }

    fn build_snapshot_now(&self) -> Snapshot {
        let (closed, live_open, open) = self.visible_runs();
        let pricing = self.run_pricing();
        let agg = self.history_aggregates(&closed, &*pricing);

        // У выплат биржи нет пометки сезона — границы сезона по его забегам.
        let (from, to) = self.season_bounds(agg.season_span, live_open);
        let lookup = self.season_lookup(self.scope_season());
        let realized = self.trade.summarize(&*lookup, from, to);

        // Текущий забег обязан попадать в итоги: иначе экран кажется замершим.
        let open_farming = open.filter(|r| !r.is_hub);
        let open_prison = open_farming.is_some_and(is_prison_run);
        let open_rated = open_farming.filter(|_| !open_prison);
        let now = self.now();
        let open_elapsed = open_rated.map(|r| active_duration(r, IDLE_CAP_MS, Some(now))).unwrap_or(0);

        let mut unpriced_drops = agg.unpriced_drops;
        if let Some(r) = open_farming {
            unpriced_drops += r.drops.iter().filter(|d| pricing(r, d.config_base_id).is_none()).count();
        }

        // Счётчики сессии: закрытые из кэша плюс открытый — в том же порядке сложения.
        let s = &agg.session;
        let prison_fe = if open_prison { s.prison_fe + run_net(open_farming.unwrap(), &*pricing) } else { s.prison_fe };
        let in_map_ms = s.in_map_ms + open_elapsed;
        let prison_ms = s.prison_ms + if open_prison { active_duration(open_farming.unwrap(), IDLE_CAP_MS, Some(now)) } else { 0 };
        // Время сессии идёт и в городе: открытый отрезок любой, не только карта.
        let session_ms = session_clock(self.session_started_at, self.last_line_at, now);
        let value_fe = open_farming.map(|r| s.value_fe + run_value(r, &*pricing)).unwrap_or(s.value_fe);
        let cost_fe = open_farming.map(|r| s.cost_fe + run_cost(r, &*pricing)).unwrap_or(s.cost_fe);
        let fe_gained = open_farming.map(|r| s.fe_gained + run_fe(r)).unwrap_or(s.fe_gained);
        let session_top = match open_rated {
            Some(r) => top_drops(std::slice::from_ref(r), &*pricing, 3, &s.top),
            None => s.top.clone(),
        };

        // Статус — про игру, а не про фильтр.
        let status = if live_open.is_some_and(|r| !r.is_hub) { Status::Running } else { Status::Idle };
        let price_now = |_: &Run, id: ConfigBaseId| self.price_of(id);
        let open_current: Vec<Run> = open_farming.filter(|r| self.is_current_season(r)).cloned().into_iter().collect();

        Snapshot {
            status,
            hint: self.hint_for(status),
            last_activity_at: self.last_activity_at,
            log_dir: self.log_dir.as_ref().map(|p| p.to_string_lossy().to_string()),
            season_id: self.season_id,
            season_name: season_name(self.season_id, self.ui),
            all_seasons: self.all_seasons,
            other_season_runs: agg.other_season_runs,
            player: self.current_player.as_ref().map(|p| to_player_view(p, self.ui)),
            players: self.player_options(&agg.player_runs),
            player_filter: self.player_filter.clone(),
            current: open_farming.map(|r| self.to_view(r)),
            recent: agg.recent.clone(),
            rate: agg.rate.clone(),
            fe_rate: agg.fe_rate.clone(),
            // В час — по зонам, как у других трекеров: итог сессии (карты, бои
            // Vorax и тюрьма) ÷ активное время в них. Город не в счёт.
            live_rate: per_hour(value_fe - cost_fe, (in_map_ms + prison_ms) as f64),
            norm: agg.norm,
            jackpot_auto_fe: self.jackpot_auto_fe(),
            strategies: agg.strategies.clone(),
            maps: agg.maps.clone(),
            days: agg.days.clone(),
            sessions: agg.sessions.clone(),
            bag: self.bag_view(),
            min_drop_fe: self.min_drop_fe,
            // «Пробить цену» — только про текущий сезон.
            missing: missing_prices(&open_current, &price_now, Some(&agg.missing))
                .into_iter()
                .take(8)
                .map(|m| MissingPriceView {
                    name: self.display_name(m.config_base_id),
                    icon: self.icon(m.config_base_id),
                    config_base_id: m.config_base_id,
                    qty: m.qty,
                    runs: m.runs,
                    is_cost: m.is_cost,
                })
                .collect(),
            session_top: session_top
                .iter()
                .map(|t| SessionDropView {
                    config_base_id: t.config_base_id,
                    name: self.display_name(t.config_base_id),
                    icon: self.icon(t.config_base_id),
                    qty: t.qty,
                    total_fe: t.total_fe,
                    at: t.at,
                    run_name: self.run_name(&t.run),
                })
                .collect(),
            prison: PrisonInfo { net_fe: prison_fe, runs: s.prison_runs + open_prison as usize },
            session: SessionInfo { started_at: self.session_started_at, previous: agg.previous.clone() },
            totals: Totals {
                runs: s.rated_runs + open_rated.is_some() as usize,
                in_map_ms,
                prison_ms,
                session_ms,
                fe_gained,
                value_fe,
                cost_fe,
                realized_fe: realized.realized_fe,
                realization_ratio: realized.ratio,
                listed_paper_fe: realized.paper_fe,
            },
            diagnostics: SnapshotDiagnostics {
                unknown_protos: self.diagnostics.unknown_protos.iter().map(|(k, v)| (k.clone(), *v)).collect(),
                priced_items: self.prices.size(),
                unpriced_drops,
                community_prices: if self.community_enabled { self.community.size() } else { 0 },
                community_enabled: self.community_enabled,
                community_fetched_at: self.community.fetched_at_ms(),
                community_stale: self.community.stale(),
            },
        }
    }

    /// Агрегаты по закрытым забегам — при новой версии данных или когда устарели по времени.
    fn history_aggregates(&self, closed: &ClosedRuns, pricing: RunPricing) -> Rc<HistoryAggregates> {
        let now = self.now();
        if let Some(c) = self.aggregates_cache.borrow().as_ref() {
            if c.version == self.version.get() && now < c.valid_until {
                return c.clone();
            }
        }
        let all_runs = &closed.all_runs;
        let farming = &closed.farming;
        let rated: Vec<Run> = farming.iter().filter(|r| !is_prison_run(r)).cloned().collect();
        let nets: Vec<f64> = rated.iter().map(|r| run_net(r, pricing)).collect();
        let actives: Vec<i64> = rated.iter().map(|r| active_duration(r, IDLE_CAP_MS, None)).collect();
        let active_total: i64 = actives.iter().sum();
        let net_rates: Vec<f64> = nets.iter().zip(&actives).map(|(n, a)| per_hour(*n, *a as f64)).collect();
        let fe_rates: Vec<f64> = rated.iter().zip(&actives).map(|(r, a)| per_hour(run_fe(r) as f64, *a as f64)).collect();
        let fe_amounts: Vec<f64> = rated.iter().map(|r| run_fe(r) as f64).collect();

        let unpriced_drops = farming.iter().map(|r| r.drops.iter().filter(|d| pricing(r, d.config_base_id).is_none()).count()).sum();

        // Счётчики — от начала сессии; забег принадлежит сессии, в которой закончился.
        let session_farming: Vec<Run> = farming.iter().filter(|r| run_end(r) >= self.session_started_at).cloned().collect();
        let session_rated: Vec<Run> = session_farming.iter().filter(|r| !is_prison_run(r)).cloned().collect();
        let session_prison: Vec<&Run> = session_farming.iter().filter(|r| is_prison_run(r)).collect();

        let mut player_runs: IndexMap<String, usize> = IndexMap::new();
        for r in all_runs {
            if !is_farming_run(r) {
                continue;
            }
            if let Some(p) = &r.player_id {
                *player_runs.entry(p.clone()).or_insert(0) += 1;
            }
        }
        let current: Vec<Run> = farming.iter().filter(|r| self.is_current_season(r)).cloned().collect();
        let mut missing = MissingTally::new();
        tally_missing(&current, &|_: &Run, id| self.price_of(id), &mut missing);

        let start = rated.len().saturating_sub(25);
        let agg = HistoryAggregates {
            version: self.version.get(),
            valid_until: self.aggregates_expire_at(now),
            rate: summarize_timed(&net_rates, Some(&nets), nets.iter().sum(), active_total),
            fe_rate: summarize_timed(&fe_rates, Some(&fe_amounts), fe_amounts.iter().sum(), active_total),
            norm: rolling_per_hour(&nets, &actives, NORM_WINDOW),
            recent: rated[start..].iter().rev().map(|r| self.to_view(r)).collect(),
            strategies: group_by_strategy(&rated, pricing, IDLE_CAP_MS)
                .into_iter()
                .filter(|s| s.runs >= 2)
                .take(6)
                .map(|s| StrategyView {
                    title: self.strategy_title(&s.cost_items),
                    cost_items: s
                        .cost_items
                        .iter()
                        .map(|&id| CostItemView { config_base_id: id, name: self.display_name(id), icon: self.icon(id) })
                        .collect(),
                    key: s.key,
                    runs: s.runs,
                    rate: s.rate,
                    cost_per_run: s.cost_per_run,
                    net_per_run: s.net_per_run,
                    total_active_ms: s.total_active_ms,
                })
                .collect(),
            maps: group_by_map(&rated, &|r: &Run| self.map_name(r.asset.as_deref(), r.area_id), pricing).into_iter().take(8).collect(),
            days: daily_totals(farming, pricing, 14, now),
            sessions: session_totals(&self.sessions, farming, pricing, Some(10)),
            previous: self.previous_summary(farming, pricing),
            other_season_runs: if self.season_id.is_none() {
                0
            } else {
                all_runs.iter().filter(|r| is_farming_run(r) && self.season_of(r) != self.season_id).count()
            },
            player_runs,
            missing,
            unpriced_drops,
            season_span: self.season_span(all_runs),
            session: SessionAgg {
                rated_runs: session_rated.len(),
                prison_runs: session_prison.len(),
                prison_fe: session_prison.iter().fold(0.0, |a, r| a + run_net(r, pricing)),
                prison_ms: session_prison.iter().fold(0, |a, r| a + active_duration(r, IDLE_CAP_MS, None)),
                in_map_ms: session_rated.iter().fold(0, |a, r| a + active_duration(r, IDLE_CAP_MS, None)),
                value_fe: session_farming.iter().fold(0.0, |a, r| a + run_value(r, pricing)),
                cost_fe: session_farming.iter().fold(0.0, |a, r| a + run_cost(r, pricing)),
                fe_gained: session_farming.iter().fold(0, |a, r| a + run_fe(r)),
                top: top_drops(&session_rated, pricing, 3, &[]),
            },
        };
        let agg = Rc::new(agg);
        *self.aggregates_cache.borrow_mut() = Some(agg.clone());
        agg
    }

    /// Агрегаты верны до полуночи или до того, как своя пробивка устареет.
    fn aggregates_expire_at(&self, now: i64) -> i64 {
        let mut until = next_day_start(now);
        for r in self.prices.all() {
            let stale = r.observed_at + OWN_PRICE_TTL_MS;
            if stale > now && stale < until {
                until = stale;
            }
        }
        until
    }

    /// Первый старт и последний конец закрытых забегов выбранного сезона.
    fn season_span(&self, all_runs: &[Run]) -> (f64, f64) {
        let (mut from, mut to) = (f64::INFINITY, f64::NEG_INFINITY);
        let Some(season) = self.scope_season() else { return (from, to) };
        for r in all_runs {
            if self.season_of(r) != Some(season) {
                continue;
            }
            from = from.min(r.started_at as f64);
            to = to.max(run_end(r) as f64);
        }
        (from, to)
    }

    /// Границы выбранного сезона во времени: текущий открыт справа, все сезоны — без границ.
    fn season_bounds(&self, closed: (f64, f64), live_open: Option<&Run>) -> (f64, f64) {
        let Some(season) = self.scope_season() else { return (f64::NEG_INFINITY, f64::INFINITY) };
        let (mut from, mut to) = closed;
        if let Some(r) = live_open {
            if self.season_of(r) == Some(season) {
                from = from.min(r.started_at as f64);
                to = to.max(run_end(r) as f64);
            }
        }
        // У сезона ещё нет забегов — и торговли в его границах не видно.
        if from == f64::INFINITY {
            return (f64::INFINITY, f64::INFINITY);
        }
        (from, if Some(season) == self.season_id { f64::INFINITY } else { to })
    }

    /// Итог последней закрытой сессии.
    fn previous_summary(&self, farming: &[Run], pricing: RunPricing) -> Option<PeriodSummary> {
        let (start, end) = self.previous_session?;
        let runs: Vec<&Run> = farming.iter().filter(|r| run_end(r) >= start && run_end(r) < end).collect();
        if runs.is_empty() {
            return None;
        }
        Some(PeriodSummary {
            net_fe: runs.iter().fold(0.0, |a, r| a + run_net(r, pricing)),
            in_map_ms: runs.iter().fold(0, |a, r| a + if is_prison_run(r) { 0 } else { active_duration(r, IDLE_CAP_MS, None) }),
            runs: runs.iter().filter(|r| !is_prison_run(r)).count(),
        })
    }

    /// Персонажи для фильтра: текущего сезона выше, свежие сверху.
    fn player_options(&self, counts: &IndexMap<String, usize>) -> Vec<PlayerOption> {
        let current = |p: &PlayerProfile| (p.season_id.map(|s| s as i64) == self.season_id && p.season_id.is_some() == self.season_id.is_some()) as i32;
        let mut v: Vec<&PlayerProfile> = self.known_players.values().collect();
        v.sort_by(|a, b| (current(b) - current(a)).cmp(&0).then(b.last_seen_at.cmp(&a.last_seen_at)));
        v.into_iter().map(|p| PlayerOption { view: to_player_view(p, self.ui), runs: counts.get(&p.player_id).copied().unwrap_or(0) }).collect()
    }

    /// Порог «дорогой находки» для режима «авто»: P95 известных цен сезона.
    fn jackpot_auto_fe(&self) -> f64 {
        let mut values: Vec<f64> = self.prices.all().iter().map(|r| r.fe).collect();
        if self.community_enabled {
            values.extend(self.community.all().iter().map(|p| p.fe));
        }
        sort_num(&mut values);
        if values.is_empty() {
            50.0
        } else {
            js_round(percentile(&values, 0.95)).max(10.0)
        }
    }

    fn hint_for(&self, status: Status) -> Option<String> {
        if status == Status::Running {
            return None;
        }
        let ui = self.ui;
        // Про нечитаемый лог — только если из него действительно ничего не удалось достать.
        if self.history.is_empty() && self.diagnostics.lines_matched == 0 {
            return Some(tr(
                ui,
                "Лог не читается. В игре: Настройки → Другое → Включить лог, затем перезайти персонажем.",
                "The log is not readable. In game: Settings → Other → Enable log, then re-enter with your character.",
            ));
        }
        if self.last_activity_at == 0 {
            return Some(tr(ui, "Игра не запущена.", "The game is not running."));
        }
        if self.now() - self.last_activity_at > 120_000 {
            return Some(tr(
                ui,
                "Игра давно ничего не пишет в лог — похоже, закрыта.",
                "The game has not written to the log for a while — it seems to be closed.",
            ));
        }
        Some(tr(ui, "Ждём вход на карту.", "Waiting for you to enter a map."))
    }

    pub fn snapshot(&self, status: Status, hint: String) -> Snapshot {
        Snapshot { status, hint: Some(hint), ..self.build_snapshot() }
    }

    /// Остановка: дописать закрытое в базу. Повторный вызов — без действия.
    pub fn stop(&mut self) {
        if self.stopped {
            return;
        }
        self.stopped = true;
        self.community.stop();
        for past in self.past_seasons.borrow().values() {
            if let Some(c) = &past.community {
                c.stop();
            }
        }
        self.persist_finished();
    }
}

/// Профиль → вид для интерфейса.
fn to_player_view(p: &PlayerProfile, ui: Lang) -> PlayerView {
    static SUFFIX: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let re = SUFFIX.get_or_init(|| Regex::new(r"#[0-9]+$").unwrap());
    PlayerView {
        player_id: p.player_id.clone(),
        // Решёточный суффикс — дискриминатор аккаунта, на экране он шум.
        name: p.name.as_ref().filter(|n| !n.is_empty()).map(|n| re.replace(n, "").into_owned()),
        hero_name: hero_name(p.hero_id.map(|h| h as i64), ui),
        level: p.level,
        season_id: p.season_id,
        season_name: season_name(p.season_id.map(|s| s as i64), ui),
    }
}

/// Планы Инобытия в именительном падеже: в словаре они в родительном («Маячок Ледяной бездны»).
const PLANES: [(&str, &str, &str); 6] = [
    ("Ледяной бездны|Glacial Abyss", "Ледяная бездна", "Glacial Abyss"),
    ("Кипящей лавы|Blistering Lava", "Кипящая лава", "Blistering Lava Sea"),
    ("Стальной кузницы|Steel Forge", "Стальная кузница", "Steel Forge"),
    ("Громовых пустошей|Thunder Waste", "Громовые пустоши", "Thunder Wastes"),
    ("Пустотных земель|Voidlands", "Пустотные земли", "Voidlands"),
    ("Дальний космос|Deep Space", "Дальний космос", "Deep Space"),
];

struct PlaneRx {
    beacon: Regex,
    planes: Vec<Regex>,
    mark: Regex,
}

fn plane_rx() -> &'static PlaneRx {
    static RX: std::sync::OnceLock<PlaneRx> = std::sync::OnceLock::new();
    RX.get_or_init(|| PlaneRx {
        beacon: Regex::new(r"(?i)Маячок|Beacon").unwrap(),
        planes: PLANES.iter().map(|(p, _, _)| Regex::new(&format!("(?i){p}")).unwrap()).collect(),
        mark: Regex::new(r"(?i)метка времени\s*([0-9]+)|Timemark\s*([0-9]+)").unwrap(),
    })
}

/// План и метка времени из названия маячка; None — предмет не карта.
fn plane_name(item_name: &str, ui: Lang) -> Option<String> {
    let rx = plane_rx();
    if !rx.beacon.is_match(item_name) {
        return None;
    }
    let i = rx.planes.iter().position(|re| re.is_match(item_name))?;
    let (_, ru, en) = PLANES[i];
    let number = rx.mark.captures(item_name).and_then(|c| c.get(1).or(c.get(2))).map(|m| m.as_str().to_string());
    let name = tr(ui, ru, en);
    Some(match number {
        Some(n) => tr(ui, &format!("{name} · метка {n}"), &format!("{name} · Timemark {n}")),
        None => name,
    })
}

/// Имя карты, которой нет в справочнике: очищенный ассет, а не «зона 5348».
fn asset_name(asset: Option<&str>, area_id: i64, ui: Lang) -> String {
    static RX: std::sync::OnceLock<(Regex, Regex, Regex)> = std::sync::OnceLock::new();
    let (prefix, num, tail) = RX.get_or_init(|| {
        (Regex::new(r"^[A-Z]{2,4}_").unwrap(), Regex::new(r"[0-9]+$").unwrap(), Regex::new(r"[_\s-]+$").unwrap())
    });
    let zone = || tr(ui, &format!("Зона {area_id}"), &format!("Zone {area_id}"));
    let Some(asset) = asset.filter(|a| !a.is_empty()) else { return zone() };
    let a = prefix.replace(asset, "");
    let a = num.replace(&a, "");
    let a = tail.replace(&a, "");
    if a.is_empty() {
        zone()
    } else {
        a.into_owned()
    }
}
