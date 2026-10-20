//! Изменения слотов сумки → приросты и расходы предметов. Перенос `core/inventory.ts`.
//!
//! `Num` в BagMgr@:Modfy — НОВЫЙ РАЗМЕР СТАКА, поэтому доход = текущее минус
//! предыдущее по ключу (PageId, SlotId), с четырьмя краевыми случаями:
//! первое наблюдение — база; сменился предмет — перебазирование; удаление —
//! слот пуст; InitBagData — перезапись базы без начисления.

use indexmap::IndexMap;
use serde::Serialize;

use crate::patterns::{delta_kind, proto_kind, MapKeyLookup};
use crate::types::*;

/// Состояние слота: config_base_id None — слот известен и пуст.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SlotState {
    pub config_base_id: Option<ConfigBaseId>,
    pub qty: i64,
}

fn slot_key(page: i64, slot: i64) -> String {
    format!("{page}:{slot}")
}

#[derive(Default)]
pub struct InventoryTracker {
    slots: IndexMap<String, SlotState>,
    /// Текущий ProtoName между start и end.
    current_proto: Option<String>,
}

impl InventoryTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn begin_transaction(&mut self, proto: &str, diag: &mut Diagnostics) {
        self.current_proto = Some(proto.to_string());
        if proto_kind(proto) == ProtoKind::Unknown {
            *diag.unknown_protos.entry(proto.to_string()).or_insert(0) += 1;
        }
    }

    pub fn end_transaction(&mut self) {
        self.current_proto = None;
    }

    /// InitBagData — снимок: перезаписывает базу и никогда не даёт дохода.
    pub fn apply_init(&mut self, page: i64, slot: i64, id: ConfigBaseId, num: i64) {
        self.slots.insert(slot_key(page, slot), SlotState { config_base_id: Some(id), qty: num });
    }

    /// Слот очищен: что там лежало, знаем только из своего состояния.
    pub fn apply_remove(&mut self, ts: i64, page: i64, slot: i64, is_map_key: Option<MapKeyLookup>) -> Option<ItemDelta> {
        let key = slot_key(page, slot);
        let prev = self.slots.insert(key, SlotState { config_base_id: None, qty: 0 });
        let prev = prev?;
        let id = prev.config_base_id?;
        if prev.qty <= 0 {
            return None;
        }
        let proto = self.current_proto.clone().unwrap_or_else(|| "unknown".into());
        let kind = delta_kind(&proto, id, -prev.qty, is_map_key);
        Some(ItemDelta { ts, config_base_id: id, qty: -prev.qty, proto, kind, page_id: page, slot_id: slot })
    }

    /// ItemChange@ Add — новый экземпляр лёг в слот: «предмет есть, количество 0».
    pub fn apply_add(&mut self, page: i64, slot: i64, id: ConfigBaseId) {
        let key = slot_key(page, slot);
        if self.slots.get(&key).is_some_and(|p| p.config_base_id == Some(id)) {
            return;
        }
        self.slots.insert(key, SlotState { config_base_id: Some(id), qty: 0 });
    }

    /// Дельта, если изменение действительно является приростом или расходом.
    #[allow(clippy::too_many_arguments)]
    pub fn apply_modify(
        &mut self,
        ts: i64,
        page: i64,
        slot: i64,
        id: ConfigBaseId,
        num: i64,
        diag: &mut Diagnostics,
        is_map_key: Option<MapKeyLookup>,
    ) -> Option<ItemDelta> {
        let prev = self.slots.insert(slot_key(page, slot), SlotState { config_base_id: Some(id), qty: num });
        // Случай 1: слот раньше не видели — установка базы.
        let prev = prev?;
        let qty = match prev.config_base_id {
            // Случай 2: слот был пуст — получено ровно столько, сколько появилось.
            None => Some(num),
            Some(p) if p == id => Some(num - prev.qty),
            // Случай 3: в слоте лежал другой предмет — перебазируемся.
            Some(_) => None,
        };
        let Some(qty) = qty else {
            diag.slot_rebases += 1;
            return None;
        };
        if qty == 0 {
            return None;
        }
        let proto = self.current_proto.clone().unwrap_or_else(|| "unknown".into());
        let kind = delta_kind(&proto, id, qty, is_map_key);
        Some(ItemDelta { ts, config_base_id: id, qty, proto, kind, page_id: page, slot_id: slot })
    }

    /// Состояние слотов целиком — база для живого дочитывания.
    pub fn export_slots(&self) -> Vec<(String, SlotState)> {
        self.slots.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }

    pub fn import_slots(&mut self, entries: &[(String, SlotState)]) {
        for (k, v) in entries {
            self.slots.insert(k.clone(), v.clone());
        }
    }

    /// Текущее содержимое сумки — для оценки нетворса.
    pub fn snapshot(&self) -> IndexMap<ConfigBaseId, i64> {
        let mut out = IndexMap::new();
        for s in self.slots.values() {
            if let Some(id) = s.config_base_id {
                *out.entry(id).or_insert(0) += s.qty;
            }
        }
        out
    }
}
