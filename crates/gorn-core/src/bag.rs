//! Стоимость сумки по ценам. Перенос `core/bag.ts`.

use indexmap::IndexMap;
use serde::Serialize;

use crate::types::*;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BagItemValue {
    pub config_base_id: ConfigBaseId,
    pub qty: i64,
    pub total_fe: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BagValue {
    /// Оценка всего оценённого, включая сами ФЕ.
    pub value_fe: f64,
    /// ФЕ на руках — отдельно: это деньги, а не оценка.
    pub fe_cash: i64,
    /// Разных предметов без цены: на них оценка занижена.
    pub unpriced: usize,
    /// Самое ценное, кроме ФЕ, по убыванию.
    pub top: Vec<BagItemValue>,
}

/// Неоценённое не прячется, а считается.
pub fn bag_value(contents: &IndexMap<ConfigBaseId, i64>, price_of: &dyn Fn(ConfigBaseId) -> Option<f64>, limit: usize) -> BagValue {
    let (mut value_fe, mut unpriced) = (0.0, 0);
    let mut items = vec![];
    for (&id, &qty) in contents {
        if qty <= 0 {
            continue;
        }
        let Some(price) = price_of(id) else {
            unpriced += 1;
            continue;
        };
        let total_fe = price * qty as f64;
        value_fe += total_fe;
        if id != FE_ID && total_fe > 0.0 {
            items.push(BagItemValue { config_base_id: id, qty, total_fe });
        }
    }
    items.sort_by(|a, b| b.total_fe.partial_cmp(&a.total_fe).unwrap_or(std::cmp::Ordering::Equal));
    items.truncate(limit);
    BagValue { value_fe, fe_cash: contents.get(&FE_ID).copied().unwrap_or(0), unpriced, top: items }
}
