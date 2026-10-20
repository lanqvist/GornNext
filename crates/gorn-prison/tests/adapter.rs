//! Память забега: JSON того же вида, что у эталона, читается обратно без потерь;
//! испорченное сохранение — чистый забег.

use gorn_prison::adapter::{blank_memory, load_memory, PrisonAdapter, Step};
use serde_json::json;

#[test]
fn memory_roundtrip() {
    let mut a = PrisonAdapter::new(None);
    a.mem.day = 7;
    a.mem.tools = vec![1, 17];
    a.mem.step = Step::Manage;
    a.mem.storage = 3;
    a.mem.objective = Some(2);
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
