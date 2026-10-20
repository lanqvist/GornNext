//! Учёт операций с биржей. Перенос `core/trade.ts`.
//!
//! Лог даёт выставления (XchgForSale), поступления ФЕ (XchgReceive), покупки
//! (XchgBuy) и отзывы (XchgRecall), но не говорит, за какой предмет пришли
//! деньги. Поэтому считается коэффициент реализации за период: реально
//! пришедшие ФЕ к бумажной оценке выставленного.

use indexmap::IndexMap;
use serde::Serialize;

use crate::types::*;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Listing {
    pub ts: i64,
    pub config_base_id: ConfigBaseId,
    pub qty: i64,
}

/// Отзыв лота: предмет вернулся с витрины, продажи не было.
pub type Recall = Listing;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Receipt {
    pub ts: i64,
    /// Сколько ФЕ пришло — уже за вычетом комиссии.
    pub fe: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Purchase {
    pub ts: i64,
    pub config_base_id: ConfigBaseId,
    pub qty: i64,
    pub fe_spent: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RealizationSummary {
    pub realized_fe: f64,
    pub paper_fe: f64,
    pub ratio: Option<f64>,
    pub listings: usize,
    pub receipts: usize,
    pub unpriced_items: Vec<ConfigBaseId>,
}

/// Слияние списков с дедупликацией по ключу, но без схлопывания законных
/// повторов: по каждому ключу берётся МАКСИМУМ вхождений среди источников.
pub fn merge_by_key<T: Clone>(sources: &[Vec<T>], key_of: impl Fn(&T) -> String) -> Vec<T> {
    let mut best: IndexMap<String, Vec<T>> = IndexMap::new();
    for source in sources {
        let mut grouped: IndexMap<String, Vec<T>> = IndexMap::new();
        for item in source {
            grouped.entry(key_of(item)).or_default().push(item.clone());
        }
        for (k, list) in grouped {
            match best.get_mut(&k) {
                Some(e) if list.len() > e.len() => *e = list,
                Some(_) => {}
                None => {
                    best.insert(k, list);
                }
            }
        }
    }
    best.into_values().flatten().collect()
}

pub fn recall_key(r: &Recall) -> String {
    format!("{}:{}:{}", r.ts, r.config_base_id, r.qty)
}

/// Действующие лоты: выставления минус отзывы, по порядку времени.
pub fn apply_recalls(listings: &[Listing], recalls: &[Recall]) -> Vec<Listing> {
    if recalls.is_empty() {
        return listings.to_vec();
    }
    let mut active = listings.to_vec();
    active.sort_by_key(|l| l.ts);
    let mut ordered = recalls.to_vec();
    ordered.sort_by_key(|r| r.ts);
    for recall in &ordered {
        // Отзывать можно только то, что уже выставлено.
        let mut upto = active.len();
        while upto > 0 && active[upto - 1].ts > recall.ts {
            upto -= 1;
        }
        remove_listing(&mut active, upto, recall.config_base_id, recall.qty);
    }
    active
}

fn remove_listing(listings: &mut Vec<Listing>, upto: usize, id: ConfigBaseId, qty: i64) {
    for i in (0..upto).rev() {
        if listings[i].config_base_id == id && listings[i].qty == qty {
            listings.remove(i);
            return;
        }
    }
    for i in (0..upto).rev() {
        if listings[i].config_base_id != id {
            continue;
        }
        if listings[i].qty > qty {
            listings[i].qty -= qty;
        } else {
            listings.remove(i);
        }
        return;
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TradeLedger {
    /// Все выставления как были — без учёта отзывов.
    pub listings: Vec<Listing>,
    pub recalls: Vec<Recall>,
    pub receipts: Vec<Receipt>,
    pub purchases: Vec<Purchase>,
    #[serde(skip)]
    pending_spend: i64,
}

impl TradeLedger {
    pub fn active_listings(&self) -> Vec<Listing> {
        apply_recalls(&self.listings, &self.recalls)
    }

    pub fn add(&mut self, d: &ItemDelta) {
        if d.kind != ProtoKind::Trade {
            return;
        }
        match d.proto.as_str() {
            // Предмет ушёл из сумки на витрину.
            "XchgForSale" => {
                if d.qty < 0 && d.config_base_id != FE_ID {
                    self.listings.push(Listing { ts: d.ts, config_base_id: d.config_base_id, qty: -d.qty });
                }
            }
            // Пришли деньги за проданное.
            "XchgReceive" => {
                if d.config_base_id == FE_ID && d.qty > 0 {
                    self.receipts.push(Receipt { ts: d.ts, fe: d.qty });
                }
            }
            "XchgBuy" => {
                if d.config_base_id == FE_ID && d.qty < 0 {
                    self.pending_spend += -d.qty;
                } else if d.qty > 0 {
                    self.purchases.push(Purchase {
                        ts: d.ts,
                        config_base_id: d.config_base_id,
                        qty: d.qty,
                        fe_spent: self.pending_spend,
                    });
                    self.pending_spend = 0;
                }
            }
            // Лот отозван — выставление снимается с бумажного счёта.
            "XchgRecall" => {
                if d.qty > 0 && d.config_base_id != FE_ID {
                    self.recalls.push(Recall { ts: d.ts, config_base_id: d.config_base_id, qty: d.qty });
                }
            }
            _ => {}
        }
    }

    /// Сводка за период; price_of — референсная цена или None.
    pub fn summarize(&self, price_of: impl Fn(ConfigBaseId) -> Option<f64>, from: f64, to: f64) -> RealizationSummary {
        let in_range = |ts: i64| ts as f64 >= from && ts as f64 <= to;
        let listings: Vec<Listing> = self.active_listings().into_iter().filter(|l| in_range(l.ts)).collect();
        let receipts: Vec<&Receipt> = self.receipts.iter().filter(|r| in_range(r.ts)).collect();
        let mut paper = 0.0;
        let mut unpriced: IndexMap<ConfigBaseId, ()> = IndexMap::new();
        for l in &listings {
            match price_of(l.config_base_id) {
                None => {
                    unpriced.insert(l.config_base_id, ());
                }
                Some(p) => paper += p * l.qty as f64,
            }
        }
        let realized: f64 = receipts.iter().fold(0.0, |a, r| a + r.fe as f64);
        RealizationSummary {
            realized_fe: realized,
            paper_fe: paper,
            ratio: (paper > 0.0).then(|| realized / paper),
            listings: listings.len(),
            receipts: receipts.len(),
            unpriced_items: unpriced.into_keys().collect(),
        }
    }
}
