//! Файл лога целиком → забеги, дельты, наблюдения цен. Перенос `core/pipeline.ts`.
//!
//! Строки режутся как `readline` в Node: `\n`, `\r\n` и одиночный `\r`, UTF-8
//! с заменой битых последовательностей. В режиме текущего лога (игра его ещё
//! дописывает) разбирается только до последнего перевода строки, а открытый
//! забег не закрывается — его продолжит живое дочитывание.

use std::path::Path;

use indexmap::IndexMap;
use serde::Serialize;

use crate::gear::GearBaseLookup;
use crate::inventory::{InventoryTracker, SlotState};
use crate::parser::EventParser;
use crate::patterns::{patterns, MapKeyLookup};
use crate::segmenter::{RunSegmenter, SegmenterState};
use crate::trade::TradeLedger;
use crate::types::*;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsView {
    pub lines_total: i64,
    pub lines_matched: i64,
    pub unknown_protos: Vec<(String, i64)>,
    pub slot_rebases: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisResult {
    pub runs: Vec<Run>,
    pub prices: Vec<PriceObservation>,
    pub trade: TradeLedger,
    pub inventory: Vec<(ConfigBaseId, i64)>,
    /// Состояние слотов на конец файла — база для живого дочитывания.
    pub slots: Vec<(String, SlotState)>,
    pub diagnostics: DiagnosticsView,
    pub season_id: Option<i64>,
    pub players: Vec<PlayerProfile>,
    pub last_player_id: Option<String>,
    pub first_ts: Option<i64>,
    pub last_ts: Option<i64>,
    /// Сколько байт разобрано (в режиме текущего лога — до конца последней полной строки).
    pub position: u64,
    /// Незакрытая нарезка текущего лога.
    pub segmenter: Option<SegmenterState>,
}

#[derive(Default, Clone, Copy)]
pub struct AnalyzeOptions<'a> {
    /// Ключ ли предмет от карты — из справочника.
    pub is_map_key: Option<MapKeyLookup<'a>>,
    /// Файл — текущий лог, который игра ещё дописывает.
    pub current: bool,
    /// База легендарки — из справочника снаряжения. Задана — выпавшая база
    /// опознаётся легендаркой по пробивке и сравнению (`gear::identify`);
    /// у эталона опознания нет — для сверки с ним None.
    pub gear_base: Option<GearBaseLookup<'a>>,
}

/// Строки как у `readline` (crlfDelay: Infinity).
pub fn split_lines(text: &str) -> Vec<&str> {
    let mut out = vec![];
    let b = text.as_bytes();
    let mut start = 0;
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\n' => {
                out.push(&text[start..i]);
                start = i + 1;
            }
            b'\r' => {
                out.push(&text[start..i]);
                if i + 1 < b.len() && b[i + 1] == b'\n' {
                    i += 1;
                }
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    if start < b.len() {
        out.push(&text[start..]);
    }
    out
}

/// Разобрать уже прочитанные байты лога.
pub fn analyze_bytes(bytes: &[u8], opts: AnalyzeOptions) -> AnalysisResult {
    let (body, position) = if opts.current {
        match bytes.iter().rposition(|&b| b == b'\n') {
            Some(cut) => (&bytes[..=cut], cut as u64 + 1),
            None => (&bytes[..0], 0),
        }
    } else {
        (bytes, bytes.len() as u64)
    };
    let text = String::from_utf8_lossy(body);
    let mut diag = Diagnostics::default();
    let mut parser = EventParser::new();
    let mut inventory = InventoryTracker::new();
    let mut segmenter = RunSegmenter::new();
    let mut trade = TradeLedger::default();
    let mut prices = vec![];
    let mut season_id: Option<i64> = None;
    let (mut first_ts, mut last_ts) = (None, None);
    let mut players: IndexMap<String, PlayerProfile> = IndexMap::new();
    let mut last_player_id: Option<String> = None;
    let km = opts.is_map_key;

    for raw in split_lines(&text) {
        diag.lines_total += 1;
        // SeasonId приходит внутри сокет-дампа профиля игрока.
        if season_id.is_none() {
            if let Some(m) = patterns().season_id.captures(raw) {
                season_id = Some(digits(&m[1]));
            }
        }
        let Some(ev) = parser.parse(raw) else { continue };
        if let Some(base_of) = opts.gear_base {
            if let Some((base, unique, ts)) = crate::gear::evidence(&ev, base_of) {
                crate::gear::identify(segmenter.runs_mut(), base, unique, ts, base_of);
            }
        }
        // Сравнение при наведении — только улика опознания, не событие для счётчиков.
        if matches!(ev, LogEvent::GearIdentity { .. }) {
            continue;
        }
        // Пробивка с фильтрами по аффиксам — цена ролла, а не базы: для разбора
        // файла её нет (как у эталона); цену экземпляра ставит движок.
        if matches!(ev, LogEvent::FilteredPrice(_)) {
            continue;
        }
        // Окна механик — только для начала их забега.
        if let LogEvent::Page { ts, page, open, .. } = &ev {
            segmenter.page(*ts, page, *open);
            continue;
        }
        diag.lines_matched += 1;
        let ts = ev.ts();
        if ts > 0 {
            first_ts.get_or_insert(ts);
            last_ts = Some(ts);
        }
        match ev {
            LogEvent::ItemChange { proto, start, .. } => {
                if start {
                    inventory.begin_transaction(&proto, &mut diag);
                } else {
                    inventory.end_transaction();
                }
            }
            LogEvent::BagInit { page_id, slot_id, config_base_id, num, .. } => inventory.apply_init(page_id, slot_id, config_base_id, num),
            LogEvent::BagReset { page_id, .. } => inventory.apply_reset(page_id),
            LogEvent::BagAdd { page_id, slot_id, config_base_id, .. } => inventory.apply_add(page_id, slot_id, config_base_id),
            LogEvent::BagRemove { ts, page_id, slot_id, .. } => {
                if let Some(d) = inventory.apply_remove(ts, page_id, slot_id, km) {
                    segmenter.add_delta(&d);
                    trade.add(&d);
                }
            }
            LogEvent::BagModify { ts, page_id, slot_id, config_base_id, num, .. } => {
                if let Some(d) = inventory.apply_modify(ts, page_id, slot_id, config_base_id, num, &mut diag, km) {
                    segmenter.add_delta(&d);
                    trade.add(&d);
                }
            }
            LogEvent::LevelPath { asset, .. } => segmenter.note_asset(&asset),
            LogEvent::LevelIds { level_type, .. } => segmenter.note_level_type(level_type),
            LogEvent::EnterLevel { ts, area_id, .. } => segmenter.enter_level(ts, area_id),
            LogEvent::PriceObservation(p) => prices.push(p),
            LogEvent::FilteredPrice(_) | LogEvent::GearIdentity { .. } | LogEvent::Page { .. } => {}
            LogEvent::PlayerInfo(p) => {
                let merged = merge_player_info(players.get(&p.player_id), &p);
                players.insert(p.player_id.clone(), merged);
                last_player_id = Some(p.player_id.clone());
                segmenter.set_player(Some(p.player_id.clone()));
                if p.season_id.is_some() {
                    segmenter.set_season(p.season_id);
                }
            }
        }
    }

    // Открытый забег текущего лога не закрывается: игрок ещё в карте.
    let last = last_ts.unwrap_or(0);
    let (runs, seg_state) = if opts.current {
        (segmenter.closed().to_vec(), Some(segmenter.export_state(last)))
    } else {
        (segmenter.finish(last), None)
    };
    AnalysisResult {
        runs,
        prices,
        trade,
        inventory: inventory.snapshot().into_iter().collect(),
        slots: inventory.export_slots(),
        diagnostics: DiagnosticsView {
            lines_total: diag.lines_total,
            lines_matched: diag.lines_matched,
            unknown_protos: diag.unknown_protos.into_iter().collect(),
            slot_rebases: diag.slot_rebases,
        },
        season_id,
        players: players.into_values().collect(),
        last_player_id,
        first_ts,
        last_ts,
        position,
        segmenter: seg_state,
    }
}

/// Прогнать файл лога целиком.
pub fn analyze_log_file(path: &Path, opts: AnalyzeOptions) -> std::io::Result<AnalysisResult> {
    let bytes = std::fs::read(path)?;
    Ok(analyze_bytes(&bytes, opts))
}
