//! Ход → подсказка поверх игры: текст и метки в пикселях кадра (стрелка от
//! надзирателя к камере, кольцо, рамка на карточке). Перенос `hint.ts`.

use serde_json::{json, Value};

use crate::actions::*;
use crate::model::{GIFT_NAMES, OFFICE, TOOL_NAMES};
use crate::recognize::sample::Box4;
use crate::view::{BoardView, PickView};

#[derive(Clone, Debug, PartialEq)]
pub enum HintMark {
    Arrow { from: [f64; 2], to: [f64; 2] },
    Ring { at: [f64; 2], r: f64 },
    Box(Box4),
}

impl HintMark {
    pub fn to_json(&self) -> Value {
        match self {
            HintMark::Arrow { from, to } => json!({ "kind": "arrow", "from": from, "to": to }),
            HintMark::Ring { at, r } => json!({ "kind": "ring", "at": at, "r": r }),
            HintMark::Box(b) => json!({ "kind": "box", "box": b.to_vec() }),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Hint {
    pub text: String,
    pub marks: Vec<HintMark>,
}

#[derive(Clone, Debug, Default)]
pub struct HintContext<'a> {
    /// Доска на этом кадре (для мест камер и карточек).
    pub board: Option<&'a BoardView>,
    /// Выбор орудий на этом кадре.
    pub pick: Option<&'a PickView>,
    /// Места подарков сокровищницы.
    pub gifts: Option<Vec<Box4>>,
    /// Кнопка «Обновить» на выборе орудий.
    pub reroll: Option<Box4>,
    /// Камера надзирателя (из памяти, если на кадре не видно).
    pub warden: Option<i32>,
    /// «Крысиная тропа»: переход, а не обход.
    pub ratway: bool,
    /// Экран выбора цели до первого укуса: Замена и конец дня — через «Отменить».
    pub cancel_first: bool,
    /// Короткая причина хода от стратегии.
    pub reason: String,
}

/// Русское название орудия (без прозвища зверя после двоеточия).
pub fn tool_name(i: i32) -> &'static str {
    TOOL_NAMES[i as usize].split(':').next().unwrap_or("")
}

fn cell_name(n: i32) -> String {
    if n == OFFICE {
        "допросную".to_string()
    } else {
        format!("№{}", n + 1)
    }
}

/// Подпись вместе с причиной — не длиннее этого.
const TEXT_MAX: usize = 60;

fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

pub fn hint(code: i32, ctx: &HintContext) -> Hint {
    let mut marks: Vec<HintMark> = vec![];
    let at = ctx.board.map(|b| &b.at);
    let from = match (at, ctx.warden) {
        (Some(at), Some(w)) => at.cells.get(w as usize).copied(),
        _ => None,
    };
    let kind = code >> 6;
    let arg = code & 63;

    let to_cell = |marks: &mut Vec<HintMark>, n: i32| {
        let Some(c) = at.and_then(|a| a.cells.get(n as usize)) else { return };
        if let Some(f) = from {
            if Some(n) != ctx.warden {
                marks.push(HintMark::Arrow { from: [f.x, f.y], to: [c.x, c.y] });
            }
        }
        marks.push(HintMark::Ring { at: [c.x, c.y], r: c.r });
    };
    let card = |marks: &mut Vec<HintMark>, k: &str| {
        let (Some(at), Some(b)) = (at, ctx.board) else { return };
        if b.decision.is_none() {
            return;
        }
        if let Some(i) = at.decision_kinds.iter().position(|x| x == k) {
            if let Some(bx) = at.decision_cards.get(i) {
                marks.push(HintMark::Box(*bx));
            }
        }
    };

    if ctx.cancel_first && (kind == A_SWAP || kind == A_END) {
        if let Some(c) = at.and_then(|a| a.cancel) {
            marks.push(HintMark::Box(c));
        }
        return Hint {
            text: format!("Отменить, затем «{}»", if kind == A_SWAP { "Заменить" } else { "Завершить день" }),
            marks,
        };
    }

    let mut text = String::new();
    match kind {
        A_PATROL => {
            text = if ctx.ratway { format!("Выбери {}", cell_name(arg)) } else { format!("Иди в {}", cell_name(arg)) };
            to_cell(&mut marks, arg);
            let i = ctx.board.and_then(|b| b.doors.iter().position(|&d| d as i32 == arg));
            if let Some(bx) = i.and_then(|i| at.and_then(|a| a.door_cards.get(i))) {
                marks.push(HintMark::Box(*bx));
            }
        }
        A_DEVOUR => {
            text = format!("Пожирание: съесть {}", cell_name(arg));
            card(&mut marks, "consume");
            to_cell(&mut marks, arg);
        }
        A_CHAIN => {
            text = format!("Цепочка: съесть {}", cell_name(arg));
            to_cell(&mut marks, arg);
        }
        A_STOPCHAIN => {
            text = "Остановить цепочку (Отменить)".to_string();
            if let Some(c) = at.and_then(|a| a.cancel) {
                marks.push(HintMark::Box(c));
            }
        }
        A_SWAP => {
            text = "Заменить".to_string();
            card(&mut marks, "replace");
            if let Some(f) = from {
                marks.push(HintMark::Ring { at: [f.x, f.y], r: f.r });
            }
        }
        A_END => {
            text = "Завершить день".to_string();
            card(&mut marks, "end");
        }
        A_HUNT => {
            text = format!("Метка добычи: {}", cell_name(arg));
            card(&mut marks, "hunt");
            to_cell(&mut marks, arg);
        }
        A_BESTOW | A_INSTIGATE => {
            let bestow = kind == A_BESTOW;
            text = if bestow { "Наградить" } else { "Подстрекнуть" }.to_string();
            card(&mut marks, if bestow { "bestow" } else { "instigate" });
        }
        A_ACCEPT => text = "Принять событие".to_string(),
        A_SKIP => text = "Пропустить событие".to_string(),
        A_TOOL => {
            text = format!("Взять: {}", tool_name(arg));
            let id = crate::model::TOOL_IDS[arg as usize];
            if let Some(p) = ctx.pick {
                if let Some(i) = p.cards.iter().position(|c| c.as_deref() == Some(id)) {
                    if let Some(bx) = p.boxes.get(i) {
                        marks.push(HintMark::Box(*bx));
                    }
                }
            }
        }
        A_GIFT => {
            text = format!("Взять: {}", GIFT_NAMES[arg as usize]);
            if let Some(bx) = ctx.gifts.as_ref().and_then(|g| g.get(arg as usize)) {
                marks.push(HintMark::Box(*bx));
            }
        }
        A_REROLL => {
            text = "Обновить выбор".to_string();
            if let Some(r) = ctx.reroll {
                marks.push(HintMark::Box(r));
            }
        }
        _ => {}
    }
    if !ctx.reason.is_empty() && utf16_len(&text) + 3 + utf16_len(&ctx.reason) <= TEXT_MAX {
        text = format!("{text} · {}", ctx.reason);
    }
    Hint { text, marks }
}
