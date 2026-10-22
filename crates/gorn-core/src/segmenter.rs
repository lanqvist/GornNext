//! Нарезка потока событий на забеги. Перенос `core/runSegmenter.ts`.
//!
//! Забег — отрезок между входом в зону и входом в следующую. Хабы (город,
//! укрытие) помечаются is_hub. Портал в город и обратно — один забег.
//! Вложения, сделанные в городе перед входом, принадлежат следующей карте.

use serde::Serialize;

use crate::patterns::{is_challenge_level, is_hub_asset};
use crate::types::*;

/// Окно, в котором возврат в ту же карту считается продолжением забега.
const RESUME_TTL_MS: i64 = 480_000;
/// Вложения ждут своей карты не дольше этого.
const PENDING_TTL_MS: i64 = 300_000;

/// Незакрытое состояние нарезки — передаётся живому дочитыванию.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SegmenterState {
    pub current: Option<Run>,
    pub resumable: Vec<Run>,
    pub pending_costs: Vec<ItemDelta>,
    pub pending_asset: Option<String>,
    pub pending_level_type: Option<i64>,
    pub player_id: Option<String>,
    pub season_id: Option<f64>,
}

#[derive(Default)]
pub struct RunSegmenter {
    runs: Vec<Run>,
    current: Option<Run>,
    pending_asset: Option<String>,
    pending_level_type: Option<i64>,
    player_id: Option<String>,
    season_id: Option<f64>,
    pending_costs: Vec<ItemDelta>,
}

impl RunSegmenter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn note_asset(&mut self, asset: &str) {
        self.pending_asset = Some(asset.to_string());
    }

    pub fn note_level_type(&mut self, t: i64) {
        self.pending_level_type = Some(t);
    }

    pub fn set_player(&mut self, id: Option<String>) {
        self.player_id = id;
    }

    pub fn set_season(&mut self, s: Option<f64>) {
        self.season_id = s;
    }

    pub fn enter_level(&mut self, ts: i64, area_id: i64) {
        self.close_current(ts);
        let is_hub = is_hub_asset(self.pending_asset.as_deref());
        if !is_hub && self.try_resume(ts, area_id) {
            return;
        }
        let costs = if is_hub { vec![] } else { self.take_pending_costs(ts) };
        self.current = Some(Run {
            area_id,
            asset: self.pending_asset.take(),
            level_type: self.pending_level_type.take(),
            started_at: ts,
            ended_at: None,
            duration_ms: None,
            is_hub,
            player_id: self.player_id.clone(),
            season_id: self.season_id,
            drops: vec![],
            costs,
        });
    }

    /// Продолжение — только если между выходом и возвратом были одни хабы,
    /// не открывалась новая карта, зона не испытание и прошло меньше окна.
    fn try_resume(&mut self, ts: i64, area_id: i64) -> bool {
        if !self.pending_costs.is_empty() {
            return false;
        }
        if is_challenge_level(self.pending_level_type) {
            return false;
        }
        for i in (0..self.runs.len()).rev() {
            let run = &self.runs[i];
            if run.is_hub {
                continue;
            }
            if run.area_id != area_id || run.asset != self.pending_asset {
                return false;
            }
            match run.ended_at {
                Some(e) if ts - e <= RESUME_TTL_MS => {}
                _ => return false,
            }
            let mut run = self.runs.remove(i);
            run.ended_at = None;
            run.duration_ms = None;
            self.current = Some(run);
            self.pending_asset = None;
            self.pending_level_type = None;
            return true;
        }
        false
    }

    fn take_pending_costs(&mut self, ts: i64) -> Vec<ItemDelta> {
        let fresh = self.pending_costs.iter().filter(|c| ts - c.ts <= PENDING_TTL_MS).cloned().collect();
        self.pending_costs.clear();
        fresh
    }

    /// Приросты — текущей зоне, расходы на открытие — той карте, ради которой они сделаны.
    pub fn add_delta(&mut self, delta: &ItemDelta) {
        if delta.kind == ProtoKind::MapCost && delta.qty < 0 {
            let cost = ItemDelta { qty: -delta.qty, ..delta.clone() };
            match self.current.as_mut() {
                Some(c) if !c.is_hub => c.costs.push(cost),
                _ => self.pending_costs.push(cost),
            }
            return;
        }
        let Some(c) = self.current.as_mut() else { return };
        if delta.kind == ProtoKind::Loot && delta.qty > 0 {
            c.drops.push(delta.clone());
        }
    }

    fn close_current(&mut self, ts: i64) {
        if let Some(mut c) = self.current.take() {
            c.ended_at = Some(ts);
            c.duration_ms = Some(ts - c.started_at);
            self.runs.push(c);
        }
    }

    /// Состояние на конец разобранного файла.
    pub fn export_state(&self, last_ts: i64) -> SegmenterState {
        SegmenterState {
            current: self.current.clone(),
            resumable: self.runs.iter().filter(|r| r.ended_at.is_some_and(|e| last_ts - e <= RESUME_TTL_MS)).cloned().collect(),
            pending_costs: self.pending_costs.clone(),
            pending_asset: self.pending_asset.clone(),
            pending_level_type: self.pending_level_type,
            player_id: self.player_id.clone(),
            season_id: self.season_id,
        }
    }

    /// Продолжить нарезку с того места, где остановился разбор файла.
    pub fn import_state(&mut self, st: &SegmenterState) {
        self.runs = st.resumable.clone();
        self.current = st.current.clone();
        self.pending_costs = st.pending_costs.clone();
        self.pending_asset = st.pending_asset.clone();
        self.pending_level_type = st.pending_level_type;
        self.player_id = st.player_id.clone();
        self.season_id = st.season_id;
    }

    /// Закрытые забеги — без открытого.
    pub fn closed(&self) -> &[Run] {
        &self.runs
    }

    pub fn current(&self) -> Option<&Run> {
        self.current.as_ref()
    }

    /// Открытый и закрытые забеги — для опознания выпавшего снаряжения.
    pub fn runs_mut(&mut self) -> impl Iterator<Item = &mut Run> {
        self.current.iter_mut().chain(self.runs.iter_mut())
    }

    /// Завершает открытый забег. Вызывать в конце файла.
    pub fn finish(&mut self, last_ts: i64) -> Vec<Run> {
        self.close_current(last_ts);
        std::mem::take(&mut self.runs)
    }
}
