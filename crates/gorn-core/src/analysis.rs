//! Анализ забегов: чистая прибыль, активное время, стратегии, недостающие
//! цены, крупные находки. Перенос `core/analysis.ts`.

use std::collections::{BTreeSet, HashMap};

use indexmap::IndexMap;
use serde::Serialize;

use crate::stats::{per_hour, percentile, sort_num, summarize_timed, RateSummary};
use crate::types::*;

/// Цена предмета для забега: своя таблица у каждого сезона.
pub type RunPricing<'a> = &'a dyn Fn(&Run, ConfigBaseId) -> Option<f64>;

/// Забег, который идёт в статистику фарма: не город и не мгновенный проход.
pub fn is_farming_run(run: &Run) -> bool {
    !run.is_hub && run.duration_ms.unwrap_or(0) > 5000
}

/// Сезонная «Тюрьма Талоса»: отдельный источник дохода, не в темп.
pub fn is_prison_run(run: &Run) -> bool {
    run.asset.as_deref().is_some_and(|a| a.starts_with("ZhongZuiDiYu"))
}

pub fn is_rated_run(run: &Run) -> bool {
    is_farming_run(run) && !is_prison_run(run)
}

/// Сколько самой валюты ФЕ упало за забег.
pub fn run_fe(run: &Run) -> i64 {
    run.drops.iter().filter(|d| d.config_base_id == FE_ID).map(|d| d.qty).sum()
}

pub fn run_end(run: &Run) -> i64 {
    run.ended_at.unwrap_or(run.started_at)
}

pub fn run_key(run: &Run) -> String {
    format!("{}:{}", run.started_at, run.area_id)
}

/// Слияние списков забегов по ключу: при совпадении побеждает более поздний список.
pub fn merge_runs(lists: &[&[Run]]) -> Vec<Run> {
    let mut by_key: IndexMap<String, Run> = IndexMap::new();
    for list in lists {
        for run in list.iter() {
            by_key.insert(run_key(run), run.clone());
        }
    }
    let mut v: Vec<Run> = by_key.into_values().collect();
    v.sort_by_key(|r| r.started_at);
    drop_nested(v)
}

/// Убирает забеги, целиком лежащие внутри другого забега той же зоны.
fn drop_nested(sorted: Vec<Run>) -> Vec<Run> {
    let mut reach: HashMap<i64, i64> = HashMap::new();
    sorted
        .into_iter()
        .filter(|run| {
            let Some(end) = run.ended_at else { return true };
            if run.is_hub {
                return true;
            }
            if reach.get(&run.area_id).is_some_and(|&r| end <= r) {
                return false;
            }
            reach.insert(run.area_id, end);
            true
        })
        .collect()
}

/// Пауза дольше этого в активное время не идёт.
pub const IDLE_CAP_MS: i64 = 180_000;

/// Лог игры молчит дольше этого — игра закрыта, часы сессии стоят.
pub const GAME_SILENT_MS: i64 = 120_000;

/// Время сессии по часам, без вычетов: от начала сессии до «сейчас», пока
/// игра пишет лог (она пишет его и в городе, и в простое), а закрыта — до
/// последней строки её лога. Город, тюрьма, Vorax и перерывы — всё время.
pub fn session_clock(started_at: i64, last_line_at: i64, now: i64) -> i64 {
    if last_line_at <= 0 {
        return 0;
    }
    let end = if now - last_line_at <= GAME_SILENT_MS { now } else { last_line_at };
    (end - started_at).max(0)
}

/// Сколько времени карта была «живой»: промежутки между событиями обрезаются порогом.
/// Механика вне карт (доска тюрьмы, Vorax) — от входа в её окно до выхода
/// целиком: ходы на доске добычи не дают, и порог съел бы их время.
pub fn active_duration(run: &Run, idle_cap: i64, until: Option<i64>) -> i64 {
    let end = run.ended_at.or(until).unwrap_or(run.started_at);
    if let Some(entered) = run.entered_at {
        return (end - entered).max(0);
    }
    let total = (end - run.started_at).max(0);
    if total <= idle_cap {
        return total;
    }
    let mut events: Vec<i64> =
        run.drops.iter().chain(run.costs.iter()).map(|e| e.ts).filter(|&ts| ts >= run.started_at && ts <= end).collect();
    events.sort();
    if events.is_empty() {
        return total.min(idle_cap);
    }
    let mut active = 0;
    let mut previous = run.started_at;
    for ts in events.into_iter().chain(std::iter::once(end)) {
        active += (ts - previous).min(idle_cap);
        previous = ts;
    }
    active
}

/// Чистая стоимость забега: добыча минус вложения.
pub fn run_net(run: &Run, pricing: RunPricing) -> f64 {
    let mut value = 0.0;
    for d in &run.drops {
        if let Some(p) = pricing(run, d.config_base_id) {
            value += p * d.qty as f64;
        }
    }
    let mut cost = 0.0;
    for c in &run.costs {
        if let Some(p) = pricing(run, c.config_base_id) {
            cost += p * c.qty as f64;
        }
    }
    value - cost
}

/// Подпись стратегии: из чего карта была открыта.
pub fn strategy_key(run: &Run) -> String {
    let ids: BTreeSet<ConfigBaseId> = run.costs.iter().map(|c| c.config_base_id).collect();
    if ids.is_empty() {
        "none".into()
    } else {
        ids.iter().map(|i| i.to_string()).collect::<Vec<_>>().join("+")
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StrategyStats {
    pub key: String,
    pub cost_items: Vec<ConfigBaseId>,
    pub runs: usize,
    pub rate: RateSummary,
    pub cost_per_run: f64,
    pub net_per_run: f64,
    pub total_active_ms: i64,
}

/// Группировка забегов по стратегии с честной статистикой внутри каждой.
pub fn group_by_strategy(runs: &[Run], pricing: RunPricing, idle_cap: i64) -> Vec<StrategyStats> {
    let mut groups: IndexMap<String, Vec<&Run>> = IndexMap::new();
    for r in runs {
        groups.entry(strategy_key(r)).or_default().push(r);
    }
    let mut out: Vec<StrategyStats> = groups
        .into_iter()
        .map(|(key, list)| {
            let mut rates = vec![];
            let (mut cost, mut net, mut active_ms) = (0.0, 0.0, 0i64);
            for run in &list {
                let active = active_duration(run, idle_cap, None);
                let value = run_net(run, pricing);
                active_ms += active;
                net += value;
                for c in &run.costs {
                    if let Some(p) = pricing(run, c.config_base_id) {
                        cost += p * c.qty as f64;
                    }
                }
                if active > 5000 {
                    rates.push(per_hour(value, active as f64));
                }
            }
            StrategyStats {
                cost_items: if key == "none" { vec![] } else { key.split('+').map(|s| s.parse().unwrap()).collect() },
                key,
                runs: list.len(),
                rate: summarize_timed(&rates, None, net, active_ms),
                cost_per_run: cost / list.len() as f64,
                net_per_run: net / list.len() as f64,
                total_active_ms: active_ms,
            }
        })
        .collect();
    // Сначала то, во что вложено больше всего времени.
    out.sort_by_key(|s| std::cmp::Reverse(s.total_active_ms));
    out
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MissingEntry {
    pub qty: i64,
    pub runs: usize,
    pub is_cost: bool,
}

pub type MissingTally = IndexMap<ConfigBaseId, MissingEntry>;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MissingPrice {
    pub config_base_id: ConfigBaseId,
    pub qty: i64,
    pub runs: usize,
    pub is_cost: bool,
}

/// Недостающие цены, ещё не упорядоченные.
pub fn tally_missing(runs: &[Run], pricing: RunPricing, into: &mut MissingTally) {
    for run in runs {
        let mut in_run: std::collections::HashSet<ConfigBaseId> = Default::default();
        let mut note = |d: &ItemDelta, is_cost: bool| {
            if pricing(run, d.config_base_id).is_some() {
                return;
            }
            let e = into.entry(d.config_base_id).or_insert(MissingEntry { qty: 0, runs: 0, is_cost: false });
            e.qty += d.qty;
            if in_run.insert(d.config_base_id) {
                e.runs += 1;
            }
            e.is_cost |= is_cost;
        };
        for d in &run.drops {
            note(d, false);
        }
        for c in &run.costs {
            note(c, true);
        }
    }
}

/// Чего не хватает для точной оценки — по влиянию на цифру.
pub fn missing_prices(runs: &[Run], pricing: RunPricing, base: Option<&MissingTally>) -> Vec<MissingPrice> {
    let mut t = base.cloned().unwrap_or_default();
    tally_missing(runs, pricing, &mut t);
    let mut v: Vec<MissingPrice> =
        t.into_iter().map(|(id, e)| MissingPrice { config_base_id: id, qty: e.qty, runs: e.runs, is_cost: e.is_cost }).collect();
    v.sort_by(|a, b| {
        if a.is_cost != b.is_cost {
            return if a.is_cost { std::cmp::Ordering::Less } else { std::cmp::Ordering::Greater };
        }
        if a.runs != b.runs {
            return b.runs.cmp(&a.runs);
        }
        b.qty.cmp(&a.qty)
    });
    v
}

/// Норма — темп последних N забегов.
pub const NORM_WINDOW: usize = 20;

/// Норма в час: последние N забегов вместе — их добыча ÷ их время.
pub fn rolling_per_hour(nets: &[f64], active_ms: &[i64], window: usize) -> f64 {
    let from = nets.len().saturating_sub(window);
    let total: f64 = nets[from..].iter().sum();
    let ms: i64 = active_ms[from.min(active_ms.len())..].iter().sum();
    per_hour(total, ms as f64)
}

pub fn rolling_norm(rates: &[f64], window: usize) -> f64 {
    let recent = &rates[rates.len().saturating_sub(window)..];
    if recent.is_empty() {
        return 0.0;
    }
    let mut v = recent.to_vec();
    sort_num(&mut v);
    percentile(&v, 0.5)
}

/// Дорогая находка сессии: предмет в одном забеге, все его штуки вместе.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TopDrop {
    pub config_base_id: ConfigBaseId,
    pub qty: i64,
    pub total_fe: f64,
    pub at: i64,
    /// Начало забега (ключ забега — started_at:area_id).
    pub run_started_at: i64,
    pub run_area_id: i64,
    #[serde(skip)]
    pub run: Run,
}

/// Самые дорогие находки по забегам; ФЕ не в счёт.
pub fn top_drops(runs: &[Run], pricing: RunPricing, limit: usize, seed: &[TopDrop]) -> Vec<TopDrop> {
    let mut top = seed.to_vec();
    for run in runs {
        let mut by_item: IndexMap<ConfigBaseId, (i64, i64)> = IndexMap::new();
        for d in &run.drops {
            if d.config_base_id == FE_ID {
                continue;
            }
            let e = by_item.entry(d.config_base_id).or_insert((0, 0));
            e.0 += d.qty;
            e.1 = e.1.max(d.ts);
        }
        for (id, (qty, at)) in by_item {
            let Some(unit) = pricing(run, id) else { continue };
            if unit <= 0.0 || qty <= 0 {
                continue;
            }
            let total = unit * qty as f64;
            if top.len() == limit && total <= top[limit - 1].total_fe {
                continue;
            }
            top.push(TopDrop { config_base_id: id, qty, total_fe: total, at, run_started_at: run.started_at, run_area_id: run.area_id, run: run.clone() });
            top.sort_by(|a, b| b.total_fe.partial_cmp(&a.total_fe).unwrap_or(std::cmp::Ordering::Equal));
            if top.len() > limit {
                top.pop();
            }
        }
    }
    top
}
