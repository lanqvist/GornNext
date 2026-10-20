//! Срезы накопленной истории: по картам, по дням и по сессиям.
//! Перенос `core/history.ts`.

use chrono::{Duration, Local, LocalResult, NaiveDate, NaiveDateTime, TimeZone};
use indexmap::IndexMap;
use serde::Serialize;

use crate::analysis::{active_duration, is_prison_run, run_end, run_net, RunPricing, IDLE_CAP_MS};
use crate::stats::{per_hour, summarize, RateSummary};
use crate::types::Run;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MapStats {
    /// Имя карты — то же, что в таблице забегов.
    pub name: String,
    pub runs: usize,
    pub rate: RateSummary,
    pub net_per_run: f64,
    /// Среднее активное время карты.
    pub avg_active_ms: f64,
    pub total_active_ms: i64,
}

/// Какая карта сколько приносит: «куда идти», а не «чем открывать».
pub fn group_by_map(runs: &[Run], name_of: &dyn Fn(&Run) -> String, pricing: RunPricing) -> Vec<MapStats> {
    let mut groups: IndexMap<String, Vec<&Run>> = IndexMap::new();
    for run in runs {
        groups.entry(name_of(run)).or_default().push(run);
    }
    let mut out: Vec<MapStats> = groups
        .into_iter()
        .map(|(name, list)| {
            let (mut net, mut active_ms) = (0.0, 0i64);
            let mut rates = vec![];
            for run in &list {
                let active = active_duration(run, IDLE_CAP_MS, None);
                let value = run_net(run, pricing);
                net += value;
                active_ms += active;
                // Тот же отсев, что у стратегий: на пяти секундах темп — шум.
                if active > 5000 {
                    rates.push(per_hour(value, active as f64));
                }
            }
            MapStats {
                name,
                runs: list.len(),
                rate: summarize(&rates, None),
                net_per_run: net / list.len() as f64,
                avg_active_ms: active_ms as f64 / list.len() as f64,
                total_active_ms: active_ms,
            }
        })
        .collect();
    // Сначала то, где проведено больше времени: там и выборка надёжнее.
    out.sort_by_key(|s| std::cmp::Reverse(s.total_active_ms));
    out
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PeriodTotals {
    /// Забеги в темп — без тюрьмы.
    pub runs: usize,
    /// Итог, вместе с тюрьмой.
    pub net_fe: f64,
    /// Время в картах — без тюрьмы: её время лог не видит.
    pub in_map_ms: i64,
    /// Тюрьма отдельно: в net_fe она есть, в темп — нет.
    pub prison_fe: f64,
    pub prison_runs: usize,
}

fn totals_of<'r>(runs: impl IntoIterator<Item = &'r Run>, pricing: RunPricing) -> PeriodTotals {
    let mut t = PeriodTotals::default();
    for run in runs {
        let net = run_net(run, pricing);
        t.net_fe += net;
        if is_prison_run(run) {
            t.prison_fe += net;
            t.prison_runs += 1;
        } else {
            t.in_map_ms += active_duration(run, IDLE_CAP_MS, None);
            t.runs += 1;
        }
    }
    t
}

/// Местное время в мс, как у `Date` в JS: из двух одинаковых часов при
/// переводе назад — первые, несуществующий час при переводе вперёд — сдвиг на час.
fn local_ms(naive: NaiveDateTime) -> i64 {
    match Local.from_local_datetime(&naive) {
        LocalResult::Single(t) | LocalResult::Ambiguous(t, _) => t.timestamp_millis(),
        LocalResult::None => local_ms(naive + Duration::hours(1)) - 3_600_000,
    }
}

fn local_date(ts: i64) -> NaiveDate {
    Local.timestamp_millis_opt(ts).single().map(|t| t.date_naive()).unwrap_or_default()
}

fn midnight(d: NaiveDate) -> i64 {
    local_ms(d.and_hms_opt(0, 0, 0).unwrap())
}

/// Полночь местного времени для метки.
pub fn day_start(ts: i64) -> i64 {
    midnight(local_date(ts))
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DayTotals {
    /// Полночь местного времени.
    pub day: i64,
    #[serde(flatten)]
    pub totals: PeriodTotals,
}

/// Итоги за последние `days` дней, включая сегодняшний; пустые дни остаются в ряду.
/// День забега — по его окончанию. Шаг — по календарю, а не сутками:
/// при переводе часов в сутках бывает 23 и 25 часов.
pub fn daily_totals(runs: &[Run], pricing: RunPricing, days: usize, now: i64) -> Vec<DayTotals> {
    let first = local_date(now) - Duration::days(days as i64 - 1);
    let from = midnight(first);
    let mut by_day: std::collections::HashMap<i64, Vec<&Run>> = Default::default();
    for run in runs {
        let end = run_end(run);
        if end < from {
            continue;
        }
        by_day.entry(day_start(end)).or_default().push(run);
    }
    (0..days)
        .map(|i| {
            let day = midnight(first + Duration::days(i as i64));
            let list = by_day.get(&day).map(|v| v.as_slice()).unwrap_or(&[]);
            DayTotals { day, totals: totals_of(list.iter().copied(), pricing) }
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionBounds {
    pub started_at: i64,
    /// None — активная сессия.
    pub ended_at: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionTotals {
    #[serde(flatten)]
    pub bounds: SessionBounds,
    #[serde(flatten)]
    pub totals: PeriodTotals,
}

/// Итоги каждой сессии: забег — в сессии, где закончился; пустые отбрасываются;
/// свежие сверху, не больше `limit`.
pub fn session_totals(sessions: &[SessionBounds], runs: &[Run], pricing: RunPricing, limit: Option<usize>) -> Vec<SessionTotals> {
    let mut sorted = sessions.to_vec();
    sorted.sort_by_key(|s| std::cmp::Reverse(s.started_at));
    let mut out = vec![];
    for s in sorted {
        if limit.is_some_and(|l| out.len() >= l) {
            break;
        }
        let inside = runs.iter().filter(|r| {
            let end = run_end(r);
            end >= s.started_at && s.ended_at.is_none_or(|e| end < e)
        });
        let totals = totals_of(inside, pricing);
        if totals.runs + totals.prison_runs > 0 {
            out.push(SessionTotals { bounds: s, totals });
        }
    }
    out
}
