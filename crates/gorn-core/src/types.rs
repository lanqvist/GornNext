//! Типы трекера. Перенос `core/types.ts` (gorn-tli). Поля и имена в JSON —
//! те же, что у объектов эталона: по ним сверка сравнивает разбор логов.

use serde::Serialize;

/// Идентификатор предмета в игре (ConfigBaseId в логе).
pub type ConfigBaseId = i64;

/// Flame Elementium — основная торговая валюта (PageId 102, SlotId 0).
pub const FE_ID: ConfigBaseId = 100300;

/// Смысл транзакции по ItemChange@ ProtoName.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ProtoKind {
    /// доход: подобрано с земли
    Loot,
    /// расход: открытие карты
    MapCost,
    /// операции с биржей
    Trade,
    /// перекладывание, надевание, оценка — не доход и не расход
    Neutral,
    /// незнакомый протокол
    Unknown,
}

/// Событие лога.
#[derive(Clone, Debug, PartialEq)]
pub enum LogEvent {
    /// BagMgr@:Modfy — слот получил новое СОВОКУПНОЕ количество.
    BagModify { ts: i64, line: i64, page_id: i64, slot_id: i64, config_base_id: ConfigBaseId, num: i64 },
    /// BagMgr@:InitBagData — снимок слота при полной синхронизации страницы.
    BagInit { ts: i64, line: i64, page_id: i64, slot_id: i64, config_base_id: ConfigBaseId, num: i64 },
    /// BagMgr@:RemoveBagItem — слот очищен.
    BagRemove { ts: i64, line: i64, page_id: i64, slot_id: i64 },
    /// ItemChange@ Add — новый экземпляр предмета лёг в слот.
    BagAdd { ts: i64, line: i64, page_id: i64, slot_id: i64, config_base_id: ConfigBaseId, num: i64 },
    /// ItemChange@ ProtoName=X start|end.
    ItemChange { ts: i64, line: i64, proto: String, start: bool },
    /// LevelMgr:_EnterLevelSuccess(N).
    EnterLevel { ts: i64, line: i64, area_id: i64 },
    /// LevelMgr@:LevelPath — ассет карты.
    LevelPath { ts: i64, line: i64, asset: String },
    /// LevelMgr@ LevelUid, LevelType, LevelId.
    LevelIds { ts: i64, line: i64, level_uid: i64, level_type: i64, level_id: i64 },
    /// Ответ биржи на пробивку цены.
    PriceObservation(PriceObservation),
    /// Пробивка с фильтрами по аффиксам — цена конкретного ролла, а не базы.
    /// Эталон её отбрасывает; движок кладёт её на последний выпавший экземпляр
    /// базы (цена экземпляра), разбор лога и аналитика — не учитывают.
    FilteredPrice(PriceObservation),
    /// Вход персонажем и его профиль.
    PlayerInfo(PlayerInfo),
    /// Вещь на базе `base_id` — легендарка `unique_id` (сравнение при наведении).
    GearIdentity { ts: i64, line: i64, base_id: ConfigBaseId, unique_id: ConfigBaseId },
}

impl LogEvent {
    pub fn ts(&self) -> i64 {
        match self {
            LogEvent::BagModify { ts, .. }
            | LogEvent::BagInit { ts, .. }
            | LogEvent::BagRemove { ts, .. }
            | LogEvent::BagAdd { ts, .. }
            | LogEvent::ItemChange { ts, .. }
            | LogEvent::EnterLevel { ts, .. }
            | LogEvent::LevelPath { ts, .. }
            | LogEvent::LevelIds { ts, .. }
            | LogEvent::GearIdentity { ts, .. } => *ts,
            LogEvent::PriceObservation(p) | LogEvent::FilteredPrice(p) => p.ts,
            LogEvent::PlayerInfo(p) => p.ts,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PriceObservation {
    pub kind: &'static str,
    pub ts: i64,
    pub line: i64,
    /// Предмет, который пробивали.
    pub config_base_id: ConfigBaseId,
    /// Валюта лотов, обычно FE_ID.
    pub currency: ConfigBaseId,
    /// Цены за единицу по активным лотам.
    pub unit_prices: Vec<f64>,
    pub syn_id: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerInfo {
    pub ts: i64,
    pub line: i64,
    /// Идентификатор персонажа — строка: в double не помещается.
    pub player_id: String,
    pub hero_id: Option<f64>,
    pub level: Option<f64>,
    pub name: Option<String>,
    pub season_id: Option<f64>,
}

/// Сводный профиль персонажа.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerProfile {
    pub player_id: String,
    pub name: Option<String>,
    pub hero_id: Option<f64>,
    pub level: Option<f64>,
    pub season_id: Option<f64>,
    pub last_seen_at: i64,
}

/// Дополняет профиль более свежими данными: известные значения затирают старые, отсутствующие — нет.
pub fn merge_player_profile(base: Option<&PlayerProfile>, next: PlayerProfile) -> PlayerProfile {
    PlayerProfile {
        player_id: next.player_id,
        name: next.name.or_else(|| base.and_then(|b| b.name.clone())),
        hero_id: next.hero_id.or(base.and_then(|b| b.hero_id)),
        level: next.level.or(base.and_then(|b| b.level)),
        season_id: next.season_id.or(base.and_then(|b| b.season_id)),
        last_seen_at: next.last_seen_at.max(base.map(|b| b.last_seen_at).unwrap_or(0)),
    }
}

pub fn merge_player_info(base: Option<&PlayerProfile>, ev: &PlayerInfo) -> PlayerProfile {
    merge_player_profile(
        base,
        PlayerProfile {
            player_id: ev.player_id.clone(),
            name: ev.name.clone(),
            hero_id: ev.hero_id,
            level: ev.level,
            season_id: ev.season_id,
            last_seen_at: ev.ts,
        },
    )
}

/// Прирост или расход конкретного предмета.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemDelta {
    pub ts: i64,
    pub config_base_id: ConfigBaseId,
    /// Положительное — получено, отрицательное — израсходовано.
    pub qty: i64,
    pub proto: String,
    pub kind: ProtoKind,
    pub page_id: i64,
    pub slot_id: i64,
}

/// Забег: отрезок между входом в зону и выходом из неё.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub area_id: i64,
    pub asset: Option<String>,
    pub level_type: Option<i64>,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub duration_ms: Option<i64>,
    pub is_hub: bool,
    pub player_id: Option<String>,
    pub season_id: Option<f64>,
    pub drops: Vec<ItemDelta>,
    pub costs: Vec<ItemDelta>,
}

/// Счётчики самодиагностики.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Diagnostics {
    pub lines_total: i64,
    pub lines_matched: i64,
    pub unknown_protos: indexmap::IndexMap<String, i64>,
    /// Дельты, отброшенные из-за смены предмета в слоте.
    pub slot_rebases: i64,
}

/// `Number(s)` из JS для строк из лога: пробелы по краям, пустая строка — 0,
/// десятичная, экспоненциальная и 0x-запись, Infinity; иначе NaN.
pub fn js_number(s: &str) -> f64 {
    let t = s.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
    if t.is_empty() {
        return 0.0;
    }
    match t {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    for (p, radix) in [("0x", 16), ("0X", 16), ("0o", 8), ("0O", 8), ("0b", 2), ("0B", 2)] {
        if let Some(h) = t.strip_prefix(p) {
            return u64::from_str_radix(h, radix).map(|v| v as f64).unwrap_or(f64::NAN);
        }
    }
    // Rust принимает «inf», «nan» и т. п. — JS нет.
    if t.chars().any(|c| c.is_ascii_alphabetic() && c != 'e' && c != 'E') {
        return f64::NAN;
    }
    t.parse::<f64>().unwrap_or(f64::NAN)
}

/// Целое из группы цифр регулярки — как `Number(m[i])`.
pub fn digits(s: &str) -> i64 {
    let v = js_number(s);
    v as i64
}
