//! Цены из ответов биржи на пробивку. Перенос `core/priceStore.ts`.
//!
//! Референсная цена — 10-й перцентиль цен активных лотов: минимум — чей-то
//! демпинг, среднее задирают завышенные лоты. Своя пробивка свежая 3 дня;
//! устаревшая уступает цене сообщества, если та новее.

use std::collections::HashSet;

use indexmap::IndexMap;
use serde::Serialize;

use crate::stats::{percentile, sort_num};
use crate::types::*;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PriceRecord {
    pub config_base_id: ConfigBaseId,
    /// Референсная цена в ФЕ.
    pub fe: f64,
    pub median: f64,
    pub min: f64,
    pub sample_size: usize,
    pub observed_at: i64,
    pub season_id: Option<i64>,
}

/// Доля лотов, отсекаемая снизу при выборе референсной цены.
pub const REFERENCE_PERCENTILE: f64 = 0.1;
/// Сколько своя пробивка считается свежей.
pub const OWN_PRICE_TTL_MS: i64 = 3 * 24 * 3_600_000;

/// Связанный (непередаваемый) предмет — по названию: «Bound …», «绑定…», «Связанн…».
pub fn is_bound_name(name: Option<&str>) -> bool {
    let Some(n) = name else { return false };
    if n.is_empty() {
        return false;
    }
    let lower = n.to_lowercase();
    (lower.starts_with("bound") && n.chars().nth(5).is_some_and(|c| c.is_whitespace()))
        || n.starts_with("绑定")
        || lower.starts_with("связанн")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PriceSource {
    Own,
    Community,
}

/// Какую цену брать: своя побеждает, пока свежая; устаревшая уступает более новой цене сообщества.
pub fn choose_price(
    own: Option<(f64, i64)>,
    community: Option<(f64, &dyn Fn() -> f64)>,
    now: i64,
    ttl_ms: i64,
) -> Option<(f64, PriceSource)> {
    if let Some((fe, at)) = own {
        if community.is_none() || now - at < ttl_ms {
            return Some((fe, PriceSource::Own));
        }
        if let Some((_, updated)) = community {
            let u = updated();
            if !u.is_finite() || u <= at as f64 {
                return Some((fe, PriceSource::Own));
            }
        }
    }
    community.map(|(fe, _)| (fe, PriceSource::Community))
}

/// Цена сообщества: для справочника — медиана, для экипировки вне него — нижняя (P10).
pub fn community_reference(fe: f64, p10: f64, in_catalogue: bool) -> f64 {
    if in_catalogue {
        return fe;
    }
    if p10.is_finite() && p10 > 0.0 && p10 < fe {
        p10
    } else {
        fe
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct PricePoint {
    pub t: i64,
    pub fe: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PriceTrend {
    pub points: Vec<PricePoint>,
    pub change: Option<f64>,
}

const WEEK_MS: i64 = 7 * 24 * 3_600_000;
const SPARK_MS: i64 = 30 * 24 * 3_600_000;

/// Тренд по своим пробивкам: изменение — внутри недели до свежей.
pub fn price_trend(history: &[PricePoint]) -> PriceTrend {
    let mut sorted = history.to_vec();
    sorted.sort_by_key(|p| p.t);
    let Some(latest) = sorted.last().copied() else { return PriceTrend { points: vec![], change: None } };
    let recent: Vec<PricePoint> = sorted.iter().filter(|p| p.t >= latest.t - SPARK_MS).copied().collect();
    let points = recent[recent.len().saturating_sub(24)..].to_vec();
    let last_idx = sorted.len() - 1;
    let reference = sorted.iter().enumerate().find(|(i, p)| p.t >= latest.t - WEEK_MS && *i != last_idx).map(|(_, p)| *p);
    let change = match reference {
        Some(r) if r.t < latest.t && r.fe > 0.0 => Some(latest.fe / r.fe - 1.0),
        _ => None,
    };
    PriceTrend { points, change }
}

pub fn reference_price(unit_prices: &[f64]) -> f64 {
    let mut sorted: Vec<f64> = unit_prices.iter().copied().filter(|p| *p > 0.0).collect();
    sort_num(&mut sorted);
    if sorted.is_empty() {
        return 0.0;
    }
    percentile(&sorted, REFERENCE_PERCENTILE)
}

#[derive(Clone, Debug, Default)]
pub struct PriceStore {
    season_id: Option<i64>,
    prices: IndexMap<ConfigBaseId, PriceRecord>,
    history: Vec<PriceRecord>,
    bound: HashSet<ConfigBaseId>,
}

impl PriceStore {
    pub fn new(season_id: Option<i64>) -> Self {
        PriceStore { season_id, ..Default::default() }
    }

    pub fn season(&self) -> Option<i64> {
        self.season_id
    }

    /// Наблюдение из лога; более свежее вытесняет более старое.
    pub fn observe(&mut self, ev: &PriceObservation, season_id: Option<i64>) -> Option<PriceRecord> {
        // Лоты за другую валюту к оценке в ФЕ не приводятся.
        if ev.currency != FE_ID {
            return None;
        }
        let mut sorted: Vec<f64> = ev.unit_prices.iter().copied().filter(|p| *p > 0.0).collect();
        sort_num(&mut sorted);
        if sorted.is_empty() {
            return None;
        }
        let rec = PriceRecord {
            config_base_id: ev.config_base_id,
            fe: reference_price(&sorted),
            median: percentile(&sorted, 0.5),
            min: sorted[0],
            sample_size: sorted.len(),
            observed_at: ev.ts,
            season_id,
        };
        let replace = self.prices.get(&ev.config_base_id).is_none_or(|e| e.observed_at <= rec.observed_at);
        if replace {
            self.prices.insert(ev.config_base_id, rec.clone());
        }
        self.history.push(rec.clone());
        Some(rec)
    }

    /// Ручная установка цены.
    pub fn set(&mut self, id: ConfigBaseId, fe: f64, observed_at: i64) {
        self.prices.insert(
            id,
            PriceRecord { config_base_id: id, fe, median: fe, min: fe, sample_size: 0, observed_at, season_id: self.season_id },
        );
    }

    /// Цена предмета в ФЕ: ФЕ стоит сам себя, связанное — ноль.
    pub fn price_of(&self, id: ConfigBaseId) -> Option<f64> {
        if id == FE_ID {
            return Some(1.0);
        }
        if self.bound.contains(&id) {
            return Some(0.0);
        }
        self.prices.get(&id).map(|r| r.fe)
    }

    /// Цена задана правилом, а не рынком.
    pub fn is_fixed(&self, id: ConfigBaseId) -> bool {
        id == FE_ID || self.bound.contains(&id)
    }

    pub fn register_bound_ids(&mut self, ids: impl IntoIterator<Item = ConfigBaseId>) {
        self.bound.extend(ids);
    }

    pub fn get(&self, id: ConfigBaseId) -> Option<&PriceRecord> {
        self.prices.get(&id)
    }

    pub fn all(&self) -> Vec<PriceRecord> {
        self.prices.values().cloned().collect()
    }

    /// Все наблюдения, а не только последние по предмету.
    pub fn observations(&self) -> &[PriceRecord] {
        &self.history
    }

    pub fn size(&self) -> usize {
        self.prices.len()
    }
}
