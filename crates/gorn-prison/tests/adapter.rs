//! Память забега: JSON того же вида, что у эталона, читается обратно без потерь;
//! испорченное сохранение — чистый забег.

use gorn_prison::adapter::{blank_memory, load_memory, Cause, DecisionMem, PrisonAdapter, Step};
use gorn_prison::model::start_event;
use serde_json::json;

#[test]
fn memory_roundtrip() {
    let mut a = PrisonAdapter::new(None);
    a.mem.day = 7;
    a.mem.tools = vec![1, 17];
    a.mem.step = Step::Manage;
    a.mem.storage = 3;
    a.mem.objective = Some(2);
    a.mem.decision = Some(DecisionMem { consume: true, replace: false, end: true, hunt: false, after_chain: true });
    let j = a.mem.to_json();
    assert_eq!(load_memory(&j), a.mem);
    assert_eq!(j["tools"], json!(["keys", "maul"]));
    assert_eq!(j["step"], json!("manage"));
}

#[test]
fn broken_memory_is_blank() {
    assert_eq!(load_memory(&json!({ "v": 2 })), blank_memory());
    assert_eq!(load_memory(&json!(null)), blank_memory());
    assert_eq!(load_memory(&json!({ "v": 1, "cells": [] })), blank_memory());
}

/// Журнал фактов 24.10 10:07: Горн открыт посреди нового забега — день по логу
/// ушёл с 19 на 0, память прошлого (хранилище, орудия) сбрасывается.
#[test]
fn day_going_back_starts_new_run() {
    let mut a = PrisonAdapter::new(None);
    a.log_day(Some((20, 20)));
    a.mem.storage = 8;
    a.mem.storage_floor = 8;
    a.mem.tools = vec![11, 20];
    a.mem.objective_done = true;
    a.log_day(Some((1, 20)));
    assert_eq!((a.mem.day, a.mem.storage, a.mem.storage_floor), (0, 0, 0));
    assert!(a.mem.tools.is_empty());
    assert!(!a.mem.objective_done && !a.mem.run_start);
    // Соседний день и тот же — не сброс.
    a.mem.storage = 2;
    a.log_day(Some((5, 20)));
    a.log_day(Some((4, 20)));
    assert_eq!((a.mem.day, a.mem.storage), (3, 2));
}

/// «Расточительность»: за каждого нового Призматического в цепочке — по 3
/// выбора (журнал фактов 24.10 12:24–12:25: хранилище 3 → 7 за окном подарков).
#[test]
fn abundance_gives_gifts_per_prism() {
    let mut a = PrisonAdapter::new(None);
    a.chain_gifts = true;
    a.mem.start = start_event::ABUNDANCE;
    a.log_day(Some((13, 20)));
    a.mem.storage = 3;
    a.treasure(false, Cause::Consume);
    assert_eq!(a.mem.gifts, 3);
    // Призма укуса уже за окном: 3 выбора, один взят.
    a.mem.storage = 4;
    a.treasure(true, Cause::None);
    assert_eq!(a.mem.gifts, 3);
    a.gift_taken();
    assert_eq!(a.mem.gifts, 2);
    // Подарок дал ещё призму — ещё 3 выбора.
    a.mem.storage = 5;
    a.treasure(true, Cause::None);
    assert_eq!(a.mem.gifts, 5);
}
