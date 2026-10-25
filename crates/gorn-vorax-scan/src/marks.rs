//! Что показать поверх игры по совету: обводка карты (или кнопки обновления), обводки баков-целей и плашка с
//! ходом. Перенос `plan_for` из vorax-overlay/src/draw.rs (DinoRaos/vorax-solver-rs, MIT); рисует его окно
//! Горна (ui/public/vorax.js), здесь — только где и что. Надписи по-русски, карты — русскими названиями
//! (data/names-ru.json), если они известны.

use crate::data::ScanData;
use crate::layout::{CardPos, Layout};
use crate::pixels::Rect;
use crate::reading::{Problem, Reading};
use gorn_vorax::advisor::{Advice, Best, ScreenPhase, ScreenState};
use serde::Serialize;

/// Лицо карты в опорных пикселях (layout.rs CARD_FACE).
const CARD_W: f64 = 230.0;
const CARD_H: f64 = 340.0;

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Pill {
    /// ход: карта и её баки
    pub main: String,
    /// чем кончается: «64% до 840k»
    pub sub: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct CardMark {
    pub cx: f64,
    pub cy: f64,
    /// наклон, радианы
    pub angle: f64,
    pub w: f64,
    pub h: f64,
}

/// Отметки в пикселях кадра, по которому читали.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Plan {
    pub pill: Option<Pill>,
    /// карта, которую брать
    pub card: Option<CardMark>,
    /// кнопка обновления
    pub button: Option<Rect>,
    /// стекло каждого бака-цели
    pub tanks: Vec<Rect>,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.pill.is_none() && self.card.is_none() && self.button.is_none() && self.tanks.is_empty()
    }

    /// Сдвинуть на (dx, dy) и умножить на k: из пикселей кадра в пиксели экрана.
    pub fn mapped(&self, dx: f64, dy: f64, k: f64) -> Plan {
        let r = |r: &Rect| Rect::new((r.x + dx) * k, (r.y + dy) * k, r.w * k, r.h * k);
        Plan {
            pill: self.pill.clone(),
            card: self.card.map(|c| CardMark { cx: (c.cx + dx) * k, cy: (c.cy + dy) * k, angle: c.angle, w: c.w * k, h: c.h * k }),
            button: self.button.as_ref().map(r),
            tanks: self.tanks.iter().map(r).collect(),
        }
    }
}

fn pct(x: f64) -> String {
    format!("{:.0}% до {}k", x * 100.0, gorn_vorax::value::TARGET / 1000)
}

/// Название карты для плашки: русское, если известно.
pub fn title(data: &ScanData, phase: ScreenPhase, name: &str) -> String {
    let key = match phase {
        ScreenPhase::Potion => "potion",
        ScreenPhase::Instrument => "instrument",
        ScreenPhase::Protocol => "protocol",
    };
    data.ru.get(key).and_then(|m| m.get(name)).cloned().unwrap_or_else(|| name.to_string())
}

/// Не в эталоне: начальный инструмент один — обвести его; выбора нет, только подтвердить.
pub fn only_plan(l: &Layout, name: Option<&str>) -> Plan {
    let c = l.cards[1];
    Plan {
        pill: Some(Pill { main: name.unwrap_or("Единственный инструмент").to_string(), sub: "выбора нет — подтвердить".into() }),
        card: Some(CardMark { cx: c.cx, cy: c.cy, angle: c.angle, w: CARD_W * l.k, h: CARD_H * l.k }),
        button: None,
        tanks: vec![],
    }
}

/// Места в руке, откуда взяты имена чтения: неузнанных карт в его руке нет.
fn slots(reading: &Reading, size: usize) -> Vec<usize> {
    let unknown: Vec<usize> = reading
        .problems
        .iter()
        .filter_map(|p| match p {
            Problem::UnknownArt { field, .. } => field.strip_prefix("hand").and_then(|j| j.parse().ok()),
            _ => None,
        })
        .collect();
    (0..size).filter(|j| !unknown.contains(j)).collect()
}

/// Отметки этого совета на этой доске. `l`: разметка кадра, по которому читали.
pub fn plan_for(advice: &Advice, screen: &ScreenState, reading: &Reading, l: &Layout, data: &ScanData) -> Plan {
    let mut plan = Plan::default();
    if screen.phase == ScreenPhase::Protocol {
        // планы операции меняют финальный бой, а не конечную активность — советовать нечего
        plan.pill = Some(Pill { main: "План операции — любой".into(), sub: "на счёт не влияет".into() });
        return plan;
    }
    let size = reading.hand_ranks.as_ref().map(|r| r.len()).filter(|&n| n > 0).or(reading.hand_size).unwrap_or(3);
    let at = slots(reading, size);
    let pos: &[CardPos] = l.hand_at(size);
    match &advice.best {
        Best::Nothing => {}
        Best::Refresh => {
            let r = l.refresh;
            plan.button = Some(Rect::new(r.x - 0.35 * r.w, r.y - 0.45 * r.h, r.w * 1.7, r.h * 1.9));
            let sub = advice.refresh_end.map(|e| pct(e.reached)).unwrap_or_default();
            plan.pill = Some(Pill { main: "Обновить карты".into(), sub });
        }
        Best::Card(r) => {
            let name = screen.hand.get(r.index).map(|n| title(data, screen.phase, n)).unwrap_or_else(|| format!("Карта {}", r.index + 1));
            if let Some(c) = at.get(r.index).and_then(|&slot| pos.get(slot)) {
                plan.card = Some(CardMark { cx: c.cx, cy: c.cy, angle: c.angle, w: CARD_W * l.k, h: CARD_H * l.k });
            }
            plan.tanks.extend(r.targets.iter().filter(|&&t| t < 6).map(|&t| l.tank_glass[t as usize]));
            let main = match r.targets.as_slice() {
                [] => name,
                [a] => format!("{name}  →  бак {}", a + 1),
                ts => format!("{name}  →  баки {}", ts.iter().map(|t| (t + 1).to_string()).collect::<Vec<_>>().join(" + ")),
            };
            let mut sub = match (&advice.note, r.end) {
                (Some(_), _) => "брать всегда".into(),
                (None, Some(e)) => pct(e.reached),
                (None, None) => String::new(),
            };
            // 25.10: «Подтвердить» сразу после карты игра не принимает — сперва бак
            if !r.targets.is_empty() {
                sub += if sub.is_empty() { "сначала карта, потом бак" } else { " · сначала карта, потом бак" };
            }
            plan.pill = Some(Pill { main, sub });
        }
    }
    plan
}
