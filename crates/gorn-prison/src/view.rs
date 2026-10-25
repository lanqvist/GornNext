//! Что распознано на экране тюрьмы — договор между распознаванием и
//! остальным советником. Перенос `view.ts`. Камеры 0–15: 0 — допросная.
//!
//! `to_json` даёт тот же JSON, что объект эталона (необязательные поля —
//! только когда заданы): по нему сверка сравнивает виды экрана.

use serde_json::{json, Map, Value};

use crate::recognize::sample::Box4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellKind {
    Office,
    Locked,
    Empty,
    Inmate,
}

impl CellKind {
    pub fn name(self) -> &'static str {
        match self {
            CellKind::Office => "office",
            CellKind::Locked => "locked",
            CellKind::Empty => "empty",
            CellKind::Inmate => "inmate",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CellView {
    pub kind: CellKind,
    /// Зверь 0…3 (mammoth, lion, wolf, rat).
    pub beast: Option<u8>,
    pub tier: Option<u8>,
    /// «!» — событие в камере.
    pub event: Option<bool>,
    pub nest: Option<bool>,
    /// Клыки: на экране цепочки — цель пожирания; иначе — клыки над портретом.
    pub jaws: Option<bool>,
    /// Редкость видна на запертой двери.
    pub shown_tier: Option<u8>,
    /// Редкость не видна (тёмная рамка).
    pub tier_unknown: Option<bool>,
    /// Метка «Добыча».
    pub prey: Option<bool>,
    /// Метка «Бивень» (Щипцы); None — эталона нет.
    pub tusk: Option<bool>,
}

impl CellView {
    pub fn new(kind: CellKind) -> Self {
        CellView {
            kind,
            beast: None,
            tier: None,
            event: None,
            nest: None,
            jaws: None,
            shown_tier: None,
            tier_unknown: None,
            prey: None,
            tusk: None,
        }
    }

    pub fn to_json(&self) -> Value {
        let mut m = Map::new();
        m.insert("kind".into(), json!(self.kind.name()));
        if let Some(b) = self.beast {
            m.insert("beast".into(), json!(crate::model::BEASTS[b as usize]));
        }
        if let Some(t) = self.tier {
            m.insert("tier".into(), json!(t));
        }
        if let Some(v) = self.tier_unknown {
            m.insert("tierUnknown".into(), json!(v));
        }
        if let Some(v) = self.jaws {
            m.insert("jaws".into(), json!(v));
        }
        if let Some(v) = self.shown_tier {
            m.insert("shownTier".into(), json!(v));
        }
        if let Some(v) = self.event {
            m.insert("event".into(), json!(v));
        }
        if let Some(v) = self.nest {
            m.insert("nest".into(), json!(v));
        }
        if let Some(v) = self.prey {
            m.insert("prey".into(), json!(v));
        }
        if let Some(v) = self.tusk {
            m.insert("tusk".into(), json!(v));
        }
        Value::Object(m)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Decision {
    Manage { consume: bool, replace: bool, end: bool, hunt: bool },
    Office,
    /// «Выберите цель»: крысиная тропа или добыча «Выслеживания».
    Ratway,
    /// Окно события.
    Event,
}

impl Decision {
    pub fn kind(&self) -> &'static str {
        match self {
            Decision::Manage { .. } => "manage",
            Decision::Office => "office",
            Decision::Ratway => "ratway",
            Decision::Event => "event",
        }
    }

    pub fn to_json(&self) -> Value {
        match self {
            Decision::Manage { consume, replace, end, hunt } => {
                let mut m = Map::new();
                m.insert("kind".into(), json!("manage"));
                m.insert("consume".into(), json!(consume));
                m.insert("replace".into(), json!(replace));
                m.insert("end".into(), json!(end));
                if *hunt {
                    m.insert("hunt".into(), json!(true));
                }
                Value::Object(m)
            }
            d => json!({ "kind": d.kind() }),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CellAt {
    pub x: f64,
    pub y: f64,
    pub r: f64,
}

/// Где рисовать метки (пиксели кадра).
#[derive(Clone, Debug, PartialEq)]
pub struct BoardAt {
    pub cells: Vec<CellAt>,
    pub door_cards: Vec<Box4>,
    pub decision_cards: Vec<Box4>,
    /// Вид каждой карточки ряда решений, в порядке decision_cards.
    pub decision_kinds: Vec<String>,
    pub cancel: Option<Box4>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BoardView {
    pub cells: Vec<CellView>,
    pub warden: Option<usize>,
    /// None — не видно (экран цепочки) или не прочитано.
    pub days_left: Option<i32>,
    /// Предложенные камеры, в порядке карточек; допросная — 0.
    pub doors: Vec<usize>,
    pub decision: Option<Decision>,
    /// Экран цепочки: цели.
    pub chain: Option<Vec<usize>>,
    pub tools: Vec<String>,
    /// Места панели орудий (None внутри — иконка без эталона); None — панели не видно.
    pub tool_slots: Option<Vec<Option<String>>>,
    /// Призматических в хранилище; −1 — панель не видна.
    pub chromatic: i32,
    pub objective: Option<String>,
    pub at: BoardAt,
}

fn box_json(b: &Box4) -> Value {
    json!(b.to_vec())
}

impl BoardView {
    pub fn to_json(&self) -> Value {
        let mut m = Map::new();
        m.insert("cells".into(), Value::Array(self.cells.iter().map(CellView::to_json).collect()));
        m.insert("warden".into(), json!(self.warden));
        m.insert("daysLeft".into(), json!(self.days_left));
        m.insert("doors".into(), json!(self.doors));
        m.insert("decision".into(), self.decision.as_ref().map(Decision::to_json).unwrap_or(Value::Null));
        m.insert("chain".into(), self.chain.as_ref().map(|t| json!({ "targets": t })).unwrap_or(Value::Null));
        m.insert("tools".into(), json!(self.tools));
        if let Some(s) = &self.tool_slots {
            m.insert("toolSlots".into(), json!(s));
        }
        m.insert("chromatic".into(), json!(self.chromatic));
        m.insert("objective".into(), json!(self.objective));
        let at = &self.at;
        m.insert(
            "at".into(),
            json!({
                "cells": at.cells.iter().map(|c| json!({ "x": c.x, "y": c.y, "r": c.r })).collect::<Vec<_>>(),
                "doorCards": at.door_cards.iter().map(box_json).collect::<Vec<_>>(),
                "decisionCards": at.decision_cards.iter().map(box_json).collect::<Vec<_>>(),
                "decisionKinds": at.decision_kinds,
                "cancel": at.cancel.as_ref().map(box_json).unwrap_or(Value::Null),
            }),
        );
        Value::Object(m)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PickView {
    pub cards: Vec<Option<String>>,
    pub boxes: Vec<Box4>,
    /// Есть кнопка «Обновить».
    pub reroll: bool,
    /// Выбор в награду за цель (а не в начале забега).
    pub reward: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ScreenView {
    Board(BoardView),
    Pick(PickView),
    Treasure(Vec<Box4>),
    Other,
}

impl ScreenView {
    pub fn to_json(&self) -> Value {
        match self {
            ScreenView::Board(b) => json!({ "screen": "board", "board": b.to_json() }),
            ScreenView::Pick(p) => json!({
                "screen": "pick",
                "pick": {
                    "cards": p.cards,
                    "boxes": p.boxes.iter().map(box_json).collect::<Vec<_>>(),
                    "reroll": p.reroll,
                    "reward": p.reward,
                }
            }),
            ScreenView::Treasure(g) => json!({ "screen": "treasure", "gifts": g.iter().map(box_json).collect::<Vec<_>>() }),
            ScreenView::Other => json!({ "screen": "other" }),
        }
    }
}
