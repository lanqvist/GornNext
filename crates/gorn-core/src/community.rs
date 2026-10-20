//! Первичные цены из краудсорс-базы TITrack (titrack.ninja). Перенос `main/communityPrices.ts`.
//!
//! Предметы — родной ConfigBaseId игры, сезон — тот же SeasonId, что в логе.
//! Вежливость к чужому бесплатному серверу: первый заход — полная выборка,
//! дальше только дельта по updated_at; опрос раз в 15 минут; при любой ошибке —
//! молча живём на кэше. Своя пробивка всегда перекрывает цену сообщества —
//! это решает движок, здесь только доставка.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::types::ConfigBaseId;

const BASE_URL: &str = "https://qhjulyngunwiculnharg.supabase.co";
/// Публичный (publishable) ключ из открытого кода TITrack — не секрет.
const ANON_KEY: &str = "sb_publishable_YgqYSMUarrM_IKvcNpJlBw_KwTpp7ho";

const PAGE_SIZE: usize = 1000;
pub const REFRESH_INTERVAL: Duration = Duration::from_secs(15 * 60);
/// Рынок прошлого сезона закрыт: его цены сверяются не чаще раза в сутки.
pub const PAST_SEASON_REFRESH_MS: i64 = 24 * 3_600_000;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommunityPrice {
    pub config_base_id: ConfigBaseId,
    /// Медиана референсных цен участников, в ФЕ.
    pub fe: f64,
    /// 10-й перцентиль — нижняя оценка.
    pub p10: f64,
    pub submissions: f64,
    /// Сколько разных устройств прислали цену: 1 — верить осторожно.
    pub devices: f64,
    pub updated_at: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CacheFile {
    season_id: i64,
    fetched_at: String,
    /// Верхняя граница updated_at из полученных строк — курсор дельты.
    cursor: Option<String>,
    prices: Vec<CommunityPrice>,
}

/// Время ISO 8601 в мс, как `Date.parse` (доли секунды — до миллисекунд, отбрасыванием).
pub fn parse_iso_ms(s: &str) -> Option<i64> {
    let t = chrono::DateTime::parse_from_rfc3339(s).ok()?;
    Some(t.timestamp() * 1000 + (t.timestamp_subsec_nanos() / 1_000_000) as i64)
}

/// Часы в мс; подменяются в проверках.
pub type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

pub fn system_clock() -> Clock {
    Arc::new(|| chrono::Utc::now().timestamp_millis())
}

#[derive(Default)]
struct State {
    prices: IndexMap<ConfigBaseId, CommunityPrice>,
    cursor: Option<String>,
    season_id: Option<i64>,
    fetched_at: Option<i64>,
    refresh_failed: bool,
}

type Listener = Arc<dyn Fn() + Send + Sync>;

/// Лента цен сообщества одного сезона. Клон — та же лента.
#[derive(Clone)]
pub struct CommunityPriceFeed {
    state: Arc<Mutex<State>>,
    cache_path: PathBuf,
    listeners: Arc<Mutex<Vec<Listener>>>,
    /// Поколение сезона: обновление, начатое в прошлом сезоне, видит чужое поколение и выходит.
    generation: Arc<AtomicU64>,
    /// Флаг опроса текущего запуска; новый start/stop его гасит.
    polling: Arc<Mutex<Option<Arc<AtomicBool>>>>,
    /// Адрес сервера — подменяется в проверках.
    base_url: String,
    clock: Clock,
}

impl CommunityPriceFeed {
    /// Файл кэша у каждой ленты свой: цены прошлого сезона живут в отдельном файле.
    pub fn new(cache_dir: &Path, cache_file: &str) -> Self {
        CommunityPriceFeed {
            state: Default::default(),
            cache_path: cache_dir.join(cache_file),
            listeners: Default::default(),
            generation: Default::default(),
            polling: Default::default(),
            base_url: BASE_URL.into(),
            clock: system_clock(),
        }
    }

    pub fn with_clock(mut self, clock: Clock) -> Self {
        self.clock = clock;
        self
    }

    fn now_ms(&self) -> i64 {
        (self.clock)()
    }

    pub fn with_base_url(mut self, url: &str) -> Self {
        self.base_url = url.into();
        self
    }

    pub fn on_update(&self, listener: impl Fn() + Send + Sync + 'static) {
        self.listeners.lock().unwrap().push(Arc::new(listener));
    }

    pub fn size(&self) -> usize {
        self.state.lock().unwrap().prices.len()
    }

    /// Когда цены последний раз сверялись с сетью.
    pub fn fetched_at_ms(&self) -> Option<i64> {
        self.state.lock().unwrap().fetched_at
    }

    /// Сеть недоступна — показываем данные из кэша.
    pub fn stale(&self) -> bool {
        self.state.lock().unwrap().refresh_failed
    }

    pub fn price_of(&self, id: ConfigBaseId) -> Option<CommunityPrice> {
        self.state.lock().unwrap().prices.get(&id).cloned()
    }

    pub fn all(&self) -> Vec<CommunityPrice> {
        self.state.lock().unwrap().prices.values().cloned().collect()
    }

    pub fn cursor(&self) -> Option<String> {
        self.state.lock().unwrap().cursor.clone()
    }

    /// Цикл: кэш → полная выборка или дельта → опрос раз в 15 минут.
    pub fn start(&self, season_id: i64) {
        self.stop();
        self.switch_season(season_id);
        self.load_cache(season_id);
        if self.size() > 0 {
            self.notify();
        }
        let flag = Arc::new(AtomicBool::new(true));
        *self.polling.lock().unwrap() = Some(flag.clone());
        let feed = self.clone();
        std::thread::Builder::new()
            .name("gorn-community".into())
            .spawn(move || {
                while flag.load(Ordering::Relaxed) {
                    feed.refresh();
                    // Сон кусками, чтобы остановка не ждала 15 минут.
                    let mut slept = Duration::ZERO;
                    while slept < REFRESH_INTERVAL && flag.load(Ordering::Relaxed) {
                        std::thread::sleep(Duration::from_millis(500));
                        slept += Duration::from_millis(500);
                    }
                }
            })
            .expect("поток цен сообщества");
    }

    /// Разовая загрузка без опроса — для прошлого сезона; свежий кэш в сеть не ходит.
    pub fn load_once(&self, season_id: i64) {
        self.stop();
        self.switch_season(season_id);
        self.load_cache(season_id);
        if self.size() > 0 {
            self.notify();
        }
        let stale = self.fetched_at_ms().is_none_or(|t| self.now_ms() - t > PAST_SEASON_REFRESH_MS);
        if stale {
            let feed = self.clone();
            std::thread::spawn(move || {
                feed.refresh();
            });
        }
    }

    /// Как start, но обновление — сразу в этом потоке и без опроса (проверки).
    pub fn start_sync(&self, season_id: i64) {
        self.stop();
        self.switch_season(season_id);
        self.load_cache(season_id);
        if self.size() > 0 {
            self.notify();
        }
        self.refresh();
    }

    /// Как load_once, но обновление — сразу в этом потоке (проверки).
    pub fn load_once_sync(&self, season_id: i64) {
        self.stop();
        self.switch_season(season_id);
        self.load_cache(season_id);
        if self.size() > 0 {
            self.notify();
        }
        if self.fetched_at_ms().is_none_or(|t| self.now_ms() - t > PAST_SEASON_REFRESH_MS) {
            self.refresh();
        }
    }

    pub fn stop(&self) {
        if let Some(f) = self.polling.lock().unwrap().take() {
            f.store(false, Ordering::Relaxed);
        }
    }

    /// Смена сезона сбрасывает всё полученное.
    pub fn switch_season(&self, season_id: i64) {
        let mut s = self.state.lock().unwrap();
        if s.season_id == Some(season_id) {
            return;
        }
        self.generation.fetch_add(1, Ordering::SeqCst);
        *s = State { season_id: Some(season_id), ..Default::default() };
    }

    fn notify(&self) {
        let listeners: Vec<Listener> = self.listeners.lock().unwrap().clone();
        for l in listeners {
            l();
        }
    }

    /// Кэш своего сезона; кэш другого сезона не поднимаем, битый — молча пропускаем.
    pub fn load_cache(&self, season_id: i64) {
        let Ok(text) = std::fs::read_to_string(&self.cache_path) else { return };
        let mut s = self.state.lock().unwrap();
        let raw: CacheFile = match serde_json::from_str(&text) {
            Ok(r) => r,
            Err(_) => {
                s.cursor = None;
                return;
            }
        };
        if raw.season_id != season_id {
            return;
        }
        for p in raw.prices {
            s.prices.insert(p.config_base_id, p);
        }
        s.cursor = raw.cursor;
        if let Some(t) = parse_iso_ms(&raw.fetched_at) {
            s.fetched_at = Some(t);
        }
    }

    fn save_cache(&self, s: &State) {
        let Some(season_id) = s.season_id else { return };
        let at = chrono::DateTime::from_timestamp_millis(self.now_ms()).unwrap_or_default();
        let data = CacheFile {
            season_id,
            fetched_at: at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            cursor: s.cursor.clone(),
            prices: s.prices.values().cloned().collect(),
        };
        if let Some(dir) = self.cache_path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        // Кэш — оптимизация: без него следующий запуск просто скачает всё заново.
        if let Ok(text) = serde_json::to_string(&data) {
            let _ = std::fs::write(&self.cache_path, text);
        }
    }

    /// Одна страница PostgREST; диапазон — заголовком Range.
    fn fetch_page(&self, season_id: i64, offset: usize, since: Option<&str>) -> Result<Vec<serde_json::Value>, String> {
        // Вторичная сортировка по id: у пачки строк updated_at совпадает.
        let mut filters = vec![format!("season_id=eq.{season_id}"), "order=updated_at.asc,config_base_id.asc".to_string()];
        if let Some(s) = since {
            filters.push(format!("updated_at=gt.{}", encode_uri_component(s)));
        }
        let url = format!(
            "{}/rest/v1/aggregated_prices?select=config_base_id,price_fe_median,price_fe_p10,submission_count,unique_devices,updated_at&{}",
            self.base_url,
            filters.join("&")
        );
        let res = ureq::get(&url)
            .set("apikey", ANON_KEY)
            .set("Authorization", &format!("Bearer {ANON_KEY}"))
            .set("Range", &format!("{}-{}", offset, offset + PAGE_SIZE - 1))
            .timeout(Duration::from_secs(20))
            .call()
            .map_err(|e| format!("community prices: {e}"))?;
        let text = res.into_string().map_err(|e| e.to_string())?;
        serde_json::from_str(&text).map_err(|e| e.to_string())
    }

    /// Дельта-обновление; число новых/изменённых цен. Ошибки глотаются.
    pub fn refresh(&self) -> usize {
        let Some(season_id) = self.state.lock().unwrap().season_id else { return 0 };
        let generation = self.generation.load(Ordering::SeqCst);
        let current = || generation == self.generation.load(Ordering::SeqCst);
        let since = self.cursor();
        let mut offset = 0;
        let mut changed = 0;
        loop {
            let rows = match self.fetch_page(season_id, offset, since.as_deref()) {
                Ok(r) => r,
                Err(_) => {
                    // Ошибка запроса прошлого сезона к новому отношения не имеет.
                    if !current() {
                        return 0;
                    }
                    self.state.lock().unwrap().refresh_failed = true;
                    self.notify();
                    return 0;
                }
            };
            if !current() {
                return 0;
            }
            {
                let mut s = self.state.lock().unwrap();
                for row in &rows {
                    let (Some(id), Some(fe)) = (row["config_base_id"].as_f64(), row["price_fe_median"].as_f64()) else { continue };
                    if fe <= 0.0 {
                        continue;
                    }
                    let updated = row["updated_at"].as_str().unwrap_or_default().to_string();
                    let p = CommunityPrice {
                        config_base_id: id as i64,
                        fe,
                        p10: row["price_fe_p10"].as_f64().unwrap_or(f64::NAN),
                        submissions: row["submission_count"].as_f64().unwrap_or(f64::NAN),
                        devices: row["unique_devices"].as_f64().unwrap_or(f64::NAN),
                        updated_at: updated.clone(),
                    };
                    s.prices.insert(p.config_base_id, p);
                    if s.cursor.as_deref().is_none_or(|c| updated.as_str() > c) {
                        s.cursor = Some(updated);
                    }
                    changed += 1;
                }
            }
            if rows.len() < PAGE_SIZE {
                break;
            }
            offset += PAGE_SIZE;
        }
        {
            let mut s = self.state.lock().unwrap();
            // Успешная сверка — свежесть, даже если новых цен не пришло.
            s.fetched_at = Some(self.now_ms());
            s.refresh_failed = false;
            if changed > 0 {
                self.save_cache(&s);
            }
        }
        self.notify();
        changed
    }
}

/// `encodeURIComponent` из JS.
fn encode_uri_component(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}
