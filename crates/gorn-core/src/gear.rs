//! Опознание выпавшего снаряжения.
//!
//! Подбор пишет в лог только базу (`ConfigBaseId = 4647` — «Кольцо духа
//! урожая»), а легендарка на этой базе — отдельный предмет со своим номером и
//! своей ценой («Horn Ring», 113107). Цена базы у сообщества — цена редких
//! роллов (сотни ФЕ), легендарка же стоит пару ФЕ. Номер легендарки всплывает
//! позже: в пробивке (`+filters+1+refer [113107]`) и в сравнении при наведении
//! (`AttributeMgr:SetPlayerEquipment(BaseId=4647, GoldId=113107, 6)`). Тогда
//! последний выпавший экземпляр этой базы становится легендаркой — с её
//! названием и ценой.

use crate::types::*;

/// Улика старше этого окна дроп не опознаёт: пробивают и сравнивают только
/// что подобранное, а давний дроп той же базы не трогаем.
pub const IDENTIFY_WINDOW_MS: i64 = 3_600_000;

/// База легендарки по справочнику снаряжения; None — не легендарка или база неизвестна.
pub type GearBaseLookup<'a> = &'a dyn Fn(ConfigBaseId) -> Option<ConfigBaseId>;

/// Улика в момент `ts`: вещь на базе `base` — легендарка `unique`. Последний
/// выпавший до `ts` (в пределах окна) экземпляр базы становится легендаркой,
/// если он ещё не опознан: повторное наведение на ту же вещь не перекидывает
/// легендарку на более старый дроп. Копии забега (вновь открытый порталом
/// лежит и в истории) меняются вместе. Возвращает изменённые забеги:
/// (начало, зона).
pub fn identify<'r>(
    runs: impl IntoIterator<Item = &'r mut Run>,
    base: ConfigBaseId,
    unique: ConfigBaseId,
    ts: i64,
    base_of: GearBaseLookup,
) -> Vec<(i64, i64)> {
    if base == unique {
        return vec![];
    }
    let mut runs: Vec<&'r mut Run> = runs.into_iter().collect();
    let of_base = |id: ConfigBaseId| id == base || id == unique || base_of(id) == Some(base);
    let last = runs
        .iter()
        .flat_map(|r| r.drops.iter())
        .filter(|d| of_base(d.config_base_id) && d.ts <= ts && ts - d.ts <= IDENTIFY_WINDOW_MS)
        .max_by_key(|d| d.ts);
    let Some(last) = last else { return vec![] };
    if last.config_base_id != base {
        return vec![];
    }
    let (at, page, slot) = (last.ts, last.page_id, last.slot_id);
    let mut changed = vec![];
    for r in runs.iter_mut() {
        let mut hit = false;
        for d in r.drops.iter_mut().filter(|d| d.config_base_id == base && d.ts == at && d.page_id == page && d.slot_id == slot) {
            d.config_base_id = unique;
            hit = true;
        }
        if hit {
            changed.push((r.started_at, r.area_id));
        }
    }
    changed
}

/// Улика из события лога: пробивка легендарки (база — из справочника) или
/// сравнение при наведении (база — из лога). None — события-улики нет.
pub fn evidence(ev: &LogEvent, base_of: GearBaseLookup) -> Option<(ConfigBaseId, ConfigBaseId, i64)> {
    match ev {
        LogEvent::PriceObservation(p) | LogEvent::FilteredPrice(p) => base_of(p.config_base_id).map(|b| (b, p.config_base_id, p.ts)),
        LogEvent::GearIdentity { ts, base_id, unique_id, .. } => Some((*base_id, *unique_id, *ts)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drop_at(ts: i64, id: ConfigBaseId, slot: i64) -> ItemDelta {
        ItemDelta { ts, config_base_id: id, qty: 1, proto: "PickItems".into(), kind: ProtoKind::Loot, page_id: 100, slot_id: slot }
    }

    fn run(started_at: i64, drops: Vec<ItemDelta>) -> Run {
        Run {
            area_id: 1,
            asset: None,
            level_type: None,
            started_at,
            entered_at: None,
            ended_at: None,
            duration_ms: None,
            is_hub: false,
            player_id: None,
            season_id: None,
            drops,
            costs: vec![],
        }
    }

    const HORN: ConfigBaseId = 113107;
    const HARVEST: ConfigBaseId = 4647;

    fn base_of(id: ConfigBaseId) -> Option<ConfigBaseId> {
        (id == HORN).then_some(HARVEST)
    }

    #[test]
    fn last_drop_of_the_base_becomes_the_legendary_once() {
        let mut runs = vec![run(0, vec![drop_at(10, HARVEST, 1)]), run(100, vec![drop_at(110, HARVEST, 2), drop_at(120, 5000, 3)])];
        assert_eq!(identify(runs.iter_mut(), HARVEST, HORN, 200, &base_of), vec![(100, 1)]);
        assert_eq!(runs[1].drops[0].config_base_id, HORN);
        // Повторная улика — тот же экземпляр: старый дроп базы остаётся базой.
        assert!(identify(runs.iter_mut(), HARVEST, HORN, 300, &base_of).is_empty());
        assert_eq!(runs[0].drops[0].config_base_id, HARVEST);
    }

    #[test]
    fn evidence_before_the_drop_or_outside_the_window_is_ignored() {
        let mut runs = vec![run(0, vec![drop_at(1000, HARVEST, 1)])];
        assert!(identify(runs.iter_mut(), HARVEST, HORN, 999, &base_of).is_empty());
        assert!(identify(runs.iter_mut(), HARVEST, HORN, 1000 + IDENTIFY_WINDOW_MS + 1, &base_of).is_empty());
        assert_eq!(runs[0].drops[0].config_base_id, HARVEST);
    }

    #[test]
    fn copies_of_a_reopened_run_change_together() {
        let mut runs = vec![run(0, vec![drop_at(10, HARVEST, 1)]), run(0, vec![drop_at(10, HARVEST, 1)])];
        assert_eq!(identify(runs.iter_mut(), HARVEST, HORN, 20, &base_of).len(), 2);
        assert!(runs.iter().all(|r| r.drops[0].config_base_id == HORN));
    }
}
