//! Нарезка потока событий на забеги. Перенос `core/runSegmenter.ts`.
//!
//! Забег — отрезок между входом в зону и входом в следующую. Хабы (город,
//! укрытие) помечаются is_hub. Портал в город и обратно — один забег.
//! Вложения, сделанные в городе перед входом, принадлежат следующей карте.
//!
//! Механики вне карт идут в окне убежища и только потом ведут в зону: доска
//! «Тюрьмы Талоса» (S16BoardMain, минуты ходов) → зона сбора на минуту,
//! Vorax (S13GamePlay) → операционная. Окно, из которого сразу вошли в зону, —
//! начало её забега (`entered_at`) [лог 27.10: доска 2,5–3 мин, зона ~1 мин].

use serde::Serialize;

use crate::patterns::{is_challenge_level, is_hub_asset};
use crate::types::*;

/// Окно, в котором возврат в ту же карту считается продолжением забега.
const RESUME_TTL_MS: i64 = 480_000;
/// Вложения ждут своей карты не дольше этого.
const PENDING_TTL_MS: i64 = 300_000;
/// Окно механики закрылось не раньше этого до входа в зону — значит, вело в неё
/// (доска → тюрьма и Vorax → операционная: ~1 с; окно наёмников за 11 с до
/// Разлома в него не вело).
const PAGE_TO_ZONE_MS: i64 = 5_000;

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
    pub mechanic: Mechanic,
}

/// Окна сезонной механики в убежище: какие открыты, с какого момента, когда закрылись все.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Mechanic {
    pub open: Vec<String>,
    pub since: Option<i64>,
    pub closed_at: Option<i64>,
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
    mechanic: Mechanic,
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

    /// Окно сезонной механики (`S16BoardMain`, `S13GamePlay`…) открылось или закрылось.
    /// Считаются только окна в убежище: на карте доску лишь смотрят.
    pub fn page(&mut self, ts: i64, page: &str, open: bool) {
        if self.current.as_ref().is_some_and(|c| !c.is_hub) {
            return;
        }
        let m = &mut self.mechanic;
        if open {
            if m.open.is_empty() && (m.since.is_none() || m.closed_at.is_some()) {
                m.since = Some(ts);
                m.closed_at = None;
            }
            if !m.open.iter().any(|p| p == page) {
                m.open.push(page.to_string());
            }
        } else if let Some(i) = m.open.iter().position(|p| p == page) {
            m.open.remove(i);
            if m.open.is_empty() {
                m.closed_at = Some(ts);
            }
        }
    }

    /// Начало забега механики: её окно открыто или только что закрылось входом в зону.
    fn take_entered(&mut self, ts: i64, is_hub: bool) -> Option<i64> {
        let m = std::mem::take(&mut self.mechanic);
        let led_in = !m.open.is_empty() || m.closed_at.is_some_and(|c| ts - c <= PAGE_TO_ZONE_MS);
        m.since.filter(|&s| !is_hub && led_in && s < ts)
    }

    pub fn enter_level(&mut self, ts: i64, area_id: i64) {
        let is_hub = is_hub_asset(self.pending_asset.as_deref());
        let entered = self.take_entered(ts, is_hub);
        self.close_current(ts);
        // Время в окне механики — уже её забег, не убежища.
        if let Some(e) = entered {
            if let Some(hub) = self.runs.last_mut().filter(|r| r.is_hub && r.ended_at == Some(ts)) {
                let end = e.max(hub.started_at);
                hub.ended_at = Some(end);
                hub.duration_ms = Some(end - hub.started_at);
            }
        }
        if !is_hub && self.try_resume(ts, area_id) {
            return;
        }
        let costs = if is_hub { vec![] } else { self.take_pending_costs(ts) };
        self.current = Some(Run {
            area_id,
            asset: self.pending_asset.take(),
            level_type: self.pending_level_type.take(),
            started_at: ts,
            entered_at: entered,
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
            mechanic: self.mechanic.clone(),
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
        self.mechanic = st.mechanic.clone();
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

#[cfg(test)]
mod tests {
    use super::RunSegmenter;

    fn zone(s: &mut RunSegmenter, ts: i64, asset: &str) {
        s.note_asset(asset);
        s.enter_level(ts, 1);
    }

    #[test]
    fn prison_board_starts_its_run() {
        // Лог 27.10 17:49–17:54: вход в механику, доска 2,5 мин, зона сбора.
        let mut s = RunSegmenter::new();
        zone(&mut s, 0, "XZ_YuJinZhiXiBiNanSuo200");
        s.page(100_000, "S16GamePlayEntry", true);
        s.page(105_000, "S16BoardMain", true);
        s.page(106_000, "S16BoardRelicPop", true);
        s.page(107_000, "S16BoardRelicPop", false);
        s.page(250_000, "S16BoardMain", false);
        s.page(250_002, "S16GamePlayEntry", false);
        zone(&mut s, 251_000, "ZhongZuiDiYu");
        zone(&mut s, 390_000, "XZ_YuJinZhiXiBiNanSuo200");
        let runs = s.finish(400_000);
        assert_eq!(runs[0].ended_at, Some(100_000), "убежище — до входа в окно");
        assert_eq!((runs[1].started_at, runs[1].entered_at), (251_000, Some(100_000)));
        assert_eq!(runs[2].entered_at, None);
    }

    #[test]
    fn board_glance_is_not_an_entry() {
        let mut s = RunSegmenter::new();
        zone(&mut s, 0, "XZ_YuJinZhiXiBiNanSuo200");
        // Посмотрел доску и закрыл; на карту — через полторы минуты.
        s.page(10_000, "S16BoardMain", true);
        s.page(190_000, "S16BoardMain", false);
        zone(&mut s, 272_000, "DD_ChaoBaiZhiLu200");
        // Доска, открытая на карте, не в счёт.
        s.page(280_000, "S16BoardMain", true);
        s.page(281_000, "S16BoardMain", false);
        zone(&mut s, 290_000, "XZ_YuJinZhiXiBiNanSuo200");
        // Окно открыто, закрыто и открыто снова — начало с последнего открытия.
        s.page(300_000, "S13GamePlay", true);
        s.page(301_000, "S13GamePlay", false);
        s.page(500_000, "S13GamePlay", true);
        s.page(600_000, "S13GamePlay", false);
        zone(&mut s, 601_000, "DiXiaZhenSuo");
        let runs = s.finish(700_000);
        assert_eq!(runs[0].ended_at, Some(272_000));
        assert_eq!(runs[1].entered_at, None);
        assert_eq!(runs[3].entered_at, Some(500_000));
    }
}
