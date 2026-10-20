//! Честная статистика: медиана, а не среднее, и рядом — насколько ей верить.
//! Перенос `core/stats.ts`.

use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Confidence {
    Insufficient,
    Emerging,
    Solid,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RateSummary {
    pub median: f64,
    pub mean: f64,
    pub p25: f64,
    pub p75: f64,
    pub sample_size: usize,
    pub confidence: Confidence,
    /// Доля всей добычи, принесённая самым удачным забегом, в [0, 1].
    pub top_share: f64,
}

pub fn confidence_of(n: usize) -> Confidence {
    if n < 15 {
        Confidence::Insufficient
    } else if n < 50 {
        Confidence::Emerging
    } else {
        Confidence::Solid
    }
}

/// Сортировка чисел как `sort((a, b) => a - b)` в JS (NaN не ожидается).
pub fn sort_num(v: &mut [f64]) {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
}

/// Линейно интерполированный перцентиль.
pub fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    if sorted.len() == 1 {
        return sorted[0];
    }
    let idx = (sorted.len() - 1) as f64 * p;
    let lo = idx.floor() as usize;
    let hi = idx.ceil() as usize;
    if lo == hi {
        return sorted[lo];
    }
    sorted[lo] + (sorted[hi] - sorted[lo]) * (idx - lo as f64)
}

pub fn median(values: &[f64]) -> f64 {
    let mut v = values.to_vec();
    sort_num(&mut v);
    percentile(&v, 0.5)
}

/// Доля лучшего в сумме положительных значений.
pub fn top_share_of(amounts: &[f64]) -> f64 {
    let (mut total, mut max) = (0.0, 0.0);
    for &v in amounts {
        if v.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
            continue;
        }
        total += v;
        if v > max {
            max = v;
        }
    }
    if total > 0.0 {
        (max / total).min(1.0)
    } else {
        0.0
    }
}

pub fn summarize(values: &[f64], amounts: Option<&[f64]>) -> RateSummary {
    let n = values.len();
    if n == 0 {
        return RateSummary {
            median: 0.0,
            mean: 0.0,
            p25: 0.0,
            p75: 0.0,
            sample_size: 0,
            confidence: Confidence::Insufficient,
            top_share: 0.0,
        };
    }
    let mut sorted = values.to_vec();
    sort_num(&mut sorted);
    let total = values.iter().fold(0.0, |a, b| a + b);
    RateSummary {
        median: percentile(&sorted, 0.5),
        mean: total / n as f64,
        p25: percentile(&sorted, 0.25),
        p75: percentile(&sorted, 0.75),
        sample_size: n,
        confidence: confidence_of(n),
        top_share: top_share_of(amounts.unwrap_or(values)),
    }
}

/// Доход в час из количества и длительности.
pub fn per_hour(amount: f64, duration_ms: f64) -> f64 {
    if duration_ms <= 0.0 {
        return 0.0;
    }
    (amount / duration_ms) * 3_600_000.0
}
