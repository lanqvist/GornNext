//! Разбор всех накопленных логов при первом запуске. Перенос `core/retroScan.ts`.
//!
//! Каждый файл — отдельная сессия игры, поэтому состояние сумки между файлами
//! не переносится: после перезахода игра присылает InitBagData заново.

use std::path::{Path, PathBuf};

use indexmap::{IndexMap, IndexSet};

use crate::analysis::RunPricing;
use crate::inventory::SlotState;
use crate::locate::list_log_files;
use crate::pipeline::{analyze_log_file, AnalyzeOptions};
use crate::prices::PriceStore;
use crate::segmenter::SegmenterState;
use crate::trade::{merge_by_key, recall_key, TradeLedger};
use crate::types::*;

#[derive(Clone, Debug)]
pub struct RetroScanResult {
    pub runs: Vec<Run>,
    pub prices: PriceStore,
    pub trade: TradeLedger,
    pub files_scanned: usize,
    pub lines_scanned: i64,
    /// Порядок появления: первый — самый старый.
    pub season_ids: IndexSet<i64>,
    pub elapsed_ms: u128,
    /// Незнакомые протоколы по всем файлам — для самодиагностики.
    pub unknown_protos: IndexMap<String, i64>,
    /// Файлы, которые не удалось прочитать (обычно ротация во время скана).
    pub failed: Vec<(PathBuf, String)>,
    /// Слоты сумки после последнего файла — для живого дочитывания.
    pub slots: Vec<(String, SlotState)>,
    /// Сезон последнего файла — текущий.
    pub latest_season_id: Option<i64>,
    /// Все персонажи из всех файлов, у каждого — самое свежее состояние.
    pub players: Vec<PlayerProfile>,
    /// Кем зашли в последнем файле.
    pub last_player_id: Option<String>,
    /// Текущий лог и конец его последней полной строки: с него продолжит дочитывание.
    pub current_log: Option<(PathBuf, u64)>,
    /// Незакрытая нарезка текущего лога.
    pub segmenter: Option<SegmenterState>,
}

#[derive(Clone, Debug)]
pub struct RetroScanProgress<'a> {
    pub file: &'a Path,
    pub index: usize,
    pub total: usize,
    /// Забегов найдено к началу этого файла.
    pub runs_found: usize,
}

pub fn retro_scan(
    log_dir: &Path,
    mut on_progress: Option<&mut dyn FnMut(&RetroScanProgress)>,
    options: AnalyzeOptions,
) -> std::io::Result<RetroScanResult> {
    let started = std::time::Instant::now();
    let files = list_log_files(log_dir)?;

    let mut runs: Vec<Run> = vec![];
    let mut slots = vec![];
    let mut unknown_protos: IndexMap<String, i64> = IndexMap::new();
    let mut season_ids = IndexSet::new();
    let mut lines_scanned = 0;
    let mut season_id = None;
    let mut current_log = None;
    let mut segmenter = None;
    let mut players: IndexMap<String, PlayerProfile> = IndexMap::new();
    let mut last_player_id = None;

    // Файлы могут перекрываться после ротации при краше: торговые события
    // каждого файла копятся отдельно и сливаются с дедупликацией.
    let (mut listings, mut receipts, mut recalls, mut purchases) = (vec![], vec![], vec![], vec![]);

    // Цены — в одном хранилище; сезон у каждой пробивки — сезон её файла.
    let mut prices = PriceStore::new(None);
    let mut failed = vec![];

    for (index, file) in files.iter().enumerate() {
        if let Some(cb) = on_progress.as_mut() {
            cb(&RetroScanProgress { file: &file.path, index, total: files.len(), runs_found: runs.len() });
        }
        // Один недочитанный файл не должен ронять разбор остальных.
        let result = match analyze_log_file(&file.path, AnalyzeOptions { current: file.is_current, ..options }) {
            Ok(r) => r,
            Err(err) => {
                failed.push((file.path.clone(), err.to_string()));
                continue;
            }
        };

        runs.extend(result.runs);
        slots = result.slots;
        lines_scanned += result.diagnostics.lines_total;
        if let Some(s) = result.season_id {
            season_ids.insert(s);
            season_id = Some(s);
        }
        for observation in &result.prices {
            prices.observe(observation, result.season_id);
        }
        listings.push(result.trade.listings);
        receipts.push(result.trade.receipts);
        recalls.push(result.trade.recalls);
        purchases.push(result.trade.purchases);

        for (proto, n) in result.diagnostics.unknown_protos {
            *unknown_protos.entry(proto).or_insert(0) += n;
        }
        // Свежие значения побеждают, но пустые не затирают известные.
        for profile in result.players {
            let merged = merge_player_profile(players.get(&profile.player_id), profile);
            players.insert(merged.player_id.clone(), merged);
        }
        if result.last_player_id.is_some() {
            last_player_id = result.last_player_id;
        }
        if file.is_current {
            current_log = Some((file.path.clone(), result.position));
            segmenter = result.segmenter;
        }
    }

    let mut trade = TradeLedger::default();
    trade.listings = merge_by_key(&listings, |l| format!("{}:{}:{}", l.ts, l.config_base_id, l.qty));
    trade.receipts = merge_by_key(&receipts, |r| format!("{}:{}", r.ts, r.fe));
    trade.recalls = merge_by_key(&recalls, recall_key);
    trade.purchases = merge_by_key(&purchases, |p| format!("{}:{}:{}", p.ts, p.config_base_id, p.qty));

    // Из двух копий одного забега остаётся более полная: копия из оборванного файла может быть усечённой.
    runs.sort_by_key(|r| r.started_at);
    let mut deduped: Vec<Run> = vec![];
    for run in runs {
        if let Some(prev) = deduped.last_mut() {
            if prev.started_at == run.started_at && prev.area_id == run.area_id {
                let score = |r: &Run| r.duration_ms.unwrap_or(0) + (r.drops.len() + r.costs.len()) as i64;
                if score(&run) > score(prev) {
                    *prev = run;
                }
                continue;
            }
        }
        deduped.push(run);
    }

    Ok(RetroScanResult {
        runs: deduped,
        prices,
        trade,
        files_scanned: files.len(),
        lines_scanned,
        season_ids,
        elapsed_ms: started.elapsed().as_millis(),
        unknown_protos,
        failed,
        slots,
        latest_season_id: season_id,
        players: players.into_values().collect(),
        last_player_id,
        current_log,
        segmenter,
    })
}

/// Суммарная бумажная оценка дропа забега.
pub fn run_value(run: &Run, pricing: RunPricing) -> f64 {
    run.drops.iter().filter_map(|d| pricing(run, d.config_base_id).map(|p| p * d.qty as f64)).fold(0.0, |a, b| a + b)
}

/// Суммарная стоимость вложенного в карту.
pub fn run_cost(run: &Run, pricing: RunPricing) -> f64 {
    run.costs.iter().filter_map(|c| pricing(run, c.config_base_id).map(|p| p * c.qty as f64)).fold(0.0, |a, b| a + b)
}
