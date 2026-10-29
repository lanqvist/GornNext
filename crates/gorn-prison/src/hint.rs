//! Ход → подсказка поверх игры: подпись и метки в пикселях кадра. Метка
//! несёт шаг хода: что жать сейчас, что следующим кликом, что только
//! показать. Подпись — плашка у той кнопки, которую жать.

use serde_json::{json, Value};

use crate::actions::*;
use crate::model::{GIFT_NAMES, OFFICE, TOOL_NAMES};
use crate::recognize::sample::Box4;
use crate::view::{BoardAt, BoardView, CellAt, PickView};

/// Шаг метки: жать сейчас.
pub const NOW: u8 = 1;
/// Шаг метки: следующий клик того же хода.
pub const THEN: u8 = 2;
/// Шаг метки: только показать — куда придёт надзиратель, кто кого ест.
pub const SHOW: u8 = 0;

#[derive(Clone, Debug, PartialEq)]
pub enum HintMark {
    Arrow { from: [f64; 2], to: [f64; 2], step: u8 },
    Ring { at: [f64; 2], r: f64, step: u8 },
    Box { b: Box4, step: u8 },
    /// Путь обхода по двору вдоль камер: от надзирателя до цели.
    Path { points: Vec<[f64; 2]> },
}

impl HintMark {
    pub fn to_json(&self) -> Value {
        match self {
            HintMark::Arrow { from, to, step } => json!({ "kind": "arrow", "from": from, "to": to, "step": step }),
            HintMark::Ring { at, r, step } => json!({ "kind": "ring", "at": at, "r": r, "step": step }),
            HintMark::Box { b, step } => json!({ "kind": "box", "box": b.to_vec(), "step": step }),
            HintMark::Path { points } => json!({ "kind": "path", "points": points }),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    /// Точка — середина верхнего края плашки.
    Top,
    /// Точка — середина нижнего края.
    Bottom,
    /// Точка — центр.
    Center,
}

impl Anchor {
    pub fn name(self) -> &'static str {
        match self {
            Anchor::Top => "top",
            Anchor::Bottom => "bottom",
            Anchor::Center => "center",
        }
    }
}

/// Где плашка с подписью.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Plate {
    /// Под карточкой, которую жать (над ней — если снизу нет места; решает окно).
    Card(Box4),
    /// Во дворе доски возле камеры, которую жать.
    Yard { at: [f64; 2], anchor: Anchor },
}

impl Plate {
    pub fn to_json(&self) -> Value {
        match self {
            Plate::Card(b) => json!({ "card": b.to_vec() }),
            Plate::Yard { at, anchor } => json!({ "at": at, "anchor": anchor.name() }),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Hint {
    /// Строка хода с причиной — для журнала, сверки и `move_key`.
    pub text: String,
    /// Крупно на плашке: что сделать этим кликом.
    pub title: String,
    /// Мелко: следующий шаг и причина.
    pub sub: String,
    pub marks: Vec<HintMark>,
    /// None — плашка сверху по центру экрана.
    pub plate: Option<Plate>,
    /// Тепловизор: жар каждого варианта хода (заполняет советник по оценкам решателя).
    pub heat: Vec<HeatSpot>,
}

/// Вариант хода на тепловизоре: где он на экране и насколько хорош.
#[derive(Clone, Debug, PartialEq)]
pub struct HeatSpot {
    /// Камера (центр и радиус) — свечение под ней.
    pub cell: Option<CellAt>,
    /// Карточка — свечение по её рамке.
    pub card: Option<Box4>,
    /// Оценка решателя — среднее итога забега, «4,8».
    pub label: String,
    /// Накал 0…1: 1 — лучший вариант, ниже — на сколько хуже (шаг — 1 призм.).
    pub t: f64,
    /// Не лучший, но разница в пределах погрешности: «≈ почти равно».
    pub near: bool,
}

impl HeatSpot {
    pub fn to_json(&self) -> Value {
        json!({
            "cell": self.cell.map(|c| json!({ "x": c.x, "y": c.y, "r": c.r })),
            "card": self.card.map(|b| b.to_vec()),
            "label": self.label,
            "t": self.t,
            "near": self.near,
        })
    }
}

impl Hint {
    /// Подсказка без меток: только подпись.
    pub fn plain(text: &str) -> Hint {
        Hint { text: text.to_string(), title: text.to_string(), sub: String::new(), marks: vec![], plate: None, heat: vec![] }
    }

    /// Ход из двух кликов: на плашке и метках — номера шагов.
    pub fn two_steps(&self) -> bool {
        self.marks.iter().any(|m| matches!(m, HintMark::Ring { step: THEN, .. } | HintMark::Box { step: THEN, .. }))
    }
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
    /// Дней до конца забега (с сегодняшним): числа тепловизора — с поправкой
    /// `forecast_gap`. None — числа как у решателя.
    pub days_left: Option<i32>,
}

/// На сколько итог забега выше ожидания решателя: ожидание — доигрывание
/// мгновенной стратегией, а ходит решатель, он сильнее. Самоигра 1 000 забегов
/// (`gorn-cli forecast-gap`, 400 раундов, 72 446 решений): с 20 дней разрыв
/// ~1,8, за день до конца ~0,1, у ожидания у потолка хранилища — меньше;
/// ячейки дней × ожидания — в пределах ±0,3 от формулы. Живой журнал
/// (92 забега): дни 0–4 — ожидание 6,09, с поправкой 7,67, итог 8,01; дни
/// 15–19 — 7,67, 8,07, 7,91. Одна на позицию — порядок ходов не меняет.
pub fn forecast_gap(days_left: i32, f: f64) -> f64 {
    let ramp = 1.0 - (-(days_left.max(0) as f64) / 11.0).exp();
    (ramp * (-0.5787 + 0.8256 * f - 0.0614 * f * f)).max(0.0)
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

/// Двор доски: прямоугольник внутри кольца камер (по центрам камер).
struct Yard {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    r: f64,
}

fn yard(at: &BoardAt) -> Option<Yard> {
    if at.cells.len() < 16 {
        return None;
    }
    let r = at.cells.iter().map(|c| c.r).sum::<f64>() / at.cells.len() as f64;
    let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for c in &at.cells {
        x0 = x0.min(c.x);
        y0 = y0.min(c.y);
        x1 = x1.max(c.x);
        y1 = y1.max(c.y);
    }
    Some(Yard { x0, y0, x1, y1, r })
}

impl Yard {
    /// Точка коридора напротив камеры: дорожка обхода.
    fn lane(&self, c: &CellAt) -> [f64; 2] {
        let d = self.r * 1.6;
        [c.x.clamp(self.x0 + d, self.x1 - d), c.y.clamp(self.y0 + d, self.y1 - d)]
    }

    /// Плашка в свободной части двора, ближе всего к камере.
    fn plate(&self, c: &CellAt) -> Plate {
        // Плашка ~300×70 при камере r=70: оценка в радиусах камеры.
        let (pw, ph) = (self.r * 4.3, self.r);
        let (left, right) = (self.x0 + self.r * 1.6, self.x1 - self.r * 1.6);
        let (top, bottom) = (self.y0 + self.r * 1.85, self.y1 - self.r * 1.05);
        let x = c.x.clamp(left + pw / 2.0, (right - pw / 2.0).max(left + pw / 2.0));
        if c.y <= self.y0 + self.r * 0.5 {
            Plate::Yard { at: [x, top], anchor: Anchor::Top }
        } else if c.y >= self.y1 - self.r * 0.5 {
            Plate::Yard { at: [x, bottom], anchor: Anchor::Bottom }
        } else {
            let x = if c.x < (self.x0 + self.x1) / 2.0 { left + pw / 2.0 } else { right - pw / 2.0 };
            Plate::Yard { at: [x, c.y.clamp(top + ph / 2.0, (bottom - ph / 2.0).max(top + ph / 2.0))], anchor: Anchor::Center }
        }
    }
}

pub fn hint(code: i32, ctx: &HintContext) -> Hint {
    let mut marks: Vec<HintMark> = vec![];
    let at = ctx.board.map(|b| &b.at);
    let yd = at.and_then(yard);
    let cell = |n: i32| at.and_then(|a| a.cells.get(n as usize)).copied();
    let from = ctx.warden.and_then(cell);
    let kind = code >> 6;
    let arg = code & 63;

    let ring = |marks: &mut Vec<HintMark>, n: i32, step: u8| {
        if let Some(c) = cell(n) {
            marks.push(HintMark::Ring { at: [c.x, c.y], r: c.r, step });
        }
    };
    // Стрелка от надзирателя (его камеры — едока) к камере: только показать.
    let arrow = |marks: &mut Vec<HintMark>, n: i32| {
        if let (Some(f), Some(c)) = (from, cell(n)) {
            if Some(n) != ctx.warden {
                marks.push(HintMark::Arrow { from: [f.x, f.y], to: [c.x, c.y], step: SHOW });
            }
        }
    };
    // Карточка ряда решений — только когда ряд на экране.
    let card = |k: &str| -> Option<Box4> {
        let (at, b) = (at?, ctx.board?);
        b.decision.as_ref()?;
        let i = at.decision_kinds.iter().position(|x| x == k)?;
        at.decision_cards.get(i).copied()
    };
    let yard_plate = |n: i32| -> Option<Plate> { Some(yd.as_ref()?.plate(&cell(n)?)) };
    // Клик по карточке, если она видна; иначе — по камере (выбор цели).
    let card_then_cell = |marks: &mut Vec<HintMark>, b: Option<Box4>, n: i32| -> Option<Plate> {
        match b {
            Some(b) => {
                marks.push(HintMark::Box { b, step: NOW });
                ring(marks, n, THEN);
                Some(Plate::Card(b))
            }
            None => {
                ring(marks, n, NOW);
                arrow(marks, n);
                yard_plate(n)
            }
        }
    };

    if ctx.cancel_first && (kind == A_SWAP || kind == A_END) {
        let next = if kind == A_SWAP { "Заменить" } else { "Завершить день" };
        let cancel = at.and_then(|a| a.cancel);
        if let Some(c) = cancel {
            marks.push(HintMark::Box { b: c, step: NOW });
        }
        return Hint {
            text: format!("Отменить, затем «{next}»"),
            title: "Отменить".to_string(),
            sub: format!("затем «{next}»"),
            marks,
            plate: cancel.map(Plate::Card),
            heat: vec![],
        };
    }

    let mut text = String::new();
    let mut title = String::new();
    let mut then = String::new();
    let mut plate = None;
    match kind {
        A_PATROL => {
            text = if ctx.ratway { format!("Выбери {}", cell_name(arg)) } else { format!("Иди в {}", cell_name(arg)) };
            title = text.clone();
            let i = ctx.board.and_then(|b| b.doors.iter().position(|&d| d as i32 == arg));
            let door = i.and_then(|i| at.and_then(|a| a.door_cards.get(i))).copied();
            match door {
                Some(b) => {
                    marks.push(HintMark::Box { b, step: NOW });
                    ring(&mut marks, arg, SHOW);
                    plate = Some(Plate::Card(b));
                }
                None => {
                    ring(&mut marks, arg, NOW);
                    plate = yard_plate(arg);
                }
            }
            if ctx.ratway {
                arrow(&mut marks, arg);
            } else if let (Some(yd), Some(w)) = (&yd, ctx.warden) {
                // Обход — по часовой стрелке: камеры по порядку номеров.
                let steps = (arg - w).rem_euclid(16);
                let points: Vec<[f64; 2]> = (0..=steps).filter_map(|k| cell((w + k) % 16)).map(|c| yd.lane(&c)).collect();
                if points.len() >= 2 {
                    marks.push(HintMark::Path { points });
                }
            }
        }
        A_DEVOUR => {
            text = format!("Пожирание: съесть {}", cell_name(arg));
            let b = card("consume");
            if b.is_some() {
                title = "Пожирание".to_string();
                then = format!("затем {}", cell_name(arg));
            } else {
                title = format!("Съесть {}", cell_name(arg));
            }
            plate = card_then_cell(&mut marks, b, arg);
        }
        A_CHAIN => {
            text = format!("Цепочка: съесть {}", cell_name(arg));
            title = format!("Съесть {}", cell_name(arg));
            then = "цепочка".to_string();
            ring(&mut marks, arg, NOW);
            arrow(&mut marks, arg);
            plate = yard_plate(arg);
        }
        A_STOPCHAIN => {
            text = "Остановить цепочку (Отменить)".to_string();
            title = "Отменить".to_string();
            then = "остановить цепочку".to_string();
            if let Some(c) = at.and_then(|a| a.cancel) {
                marks.push(HintMark::Box { b: c, step: NOW });
                plate = Some(Plate::Card(c));
            }
        }
        A_SWAP => {
            text = "Заменить".to_string();
            title = text.clone();
            if let Some(b) = card("replace") {
                marks.push(HintMark::Box { b, step: NOW });
                plate = Some(Plate::Card(b));
            }
            if let Some(f) = from {
                marks.push(HintMark::Ring { at: [f.x, f.y], r: f.r, step: SHOW });
            }
        }
        A_END => {
            text = "Завершить день".to_string();
            title = text.clone();
            if let Some(b) = card("end") {
                marks.push(HintMark::Box { b, step: NOW });
                plate = Some(Plate::Card(b));
            }
        }
        A_HUNT => {
            text = format!("Метка добычи: {}", cell_name(arg));
            let b = card("hunt");
            if b.is_some() {
                title = "Выслеживание".to_string();
                then = format!("затем {}", cell_name(arg));
            } else {
                title = format!("Пометить {}", cell_name(arg));
                then = "добыча".to_string();
            }
            plate = card_then_cell(&mut marks, b, arg);
        }
        A_BESTOW | A_INSTIGATE => {
            let bestow = kind == A_BESTOW;
            text = if bestow { "Наградить" } else { "Подстрекнуть" }.to_string();
            title = text.clone();
            if let Some(b) = card(if bestow { "bestow" } else { "instigate" }) {
                marks.push(HintMark::Box { b, step: NOW });
                plate = Some(Plate::Card(b));
            }
        }
        A_ACCEPT => text = "Принять событие".to_string(),
        A_SKIP => text = "Пропустить событие".to_string(),
        A_TOOL => {
            text = format!("Взять: {}", tool_name(arg));
            let id = crate::model::TOOL_IDS[arg as usize];
            let i = ctx.pick.and_then(|p| p.cards.iter().position(|c| c.as_deref() == Some(id)));
            if let Some(b) = i.and_then(|i| ctx.pick.and_then(|p| p.boxes.get(i))).copied() {
                marks.push(HintMark::Box { b, step: NOW });
                plate = Some(Plate::Card(b));
            }
        }
        A_GIFT => {
            text = format!("Взять: {}", GIFT_NAMES[arg as usize]);
            if let Some(b) = ctx.gifts.as_ref().and_then(|g| g.get(arg as usize)).copied() {
                marks.push(HintMark::Box { b, step: NOW });
                plate = Some(Plate::Card(b));
            }
        }
        A_REROLL => {
            text = "Обновить выбор".to_string();
            if let Some(r) = ctx.reroll {
                marks.push(HintMark::Box { b: r, step: NOW });
                plate = Some(Plate::Card(r));
            }
        }
        _ => {}
    }
    if title.is_empty() {
        title = text.clone();
    }
    if !ctx.reason.is_empty() && utf16_len(&text) + 3 + utf16_len(&ctx.reason) <= TEXT_MAX {
        text = format!("{text} · {}", ctx.reason);
    }
    let sub = [then.as_str(), ctx.reason.as_str()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
    Hint { text, title, sub, marks, plate, heat: vec![] }
}

/// Где вариант хода на экране: камера и/или карточка. None — показать негде
/// (событие: кнопки окна события не распознаются).
fn spot_of(code: i32, ctx: &HintContext) -> Option<(Option<CellAt>, Option<Box4>)> {
    let at = ctx.board.map(|b| &b.at);
    let kind = code >> 6;
    let arg = code & 63;
    let cell = at.and_then(|a| a.cells.get(arg as usize)).copied();
    let card = |k: &str| -> Option<Box4> {
        let (at, b) = (at?, ctx.board?);
        b.decision.as_ref()?;
        let i = at.decision_kinds.iter().position(|x| x == k)?;
        at.decision_cards.get(i).copied()
    };
    let cancel = at.and_then(|a| a.cancel);
    let spot = match kind {
        A_PATROL => {
            let i = ctx.board.and_then(|b| b.doors.iter().position(|&d| d as i32 == arg));
            (cell, i.and_then(|i| at.and_then(|a| a.door_cards.get(i))).copied())
        }
        A_DEVOUR | A_CHAIN | A_HUNT => (cell, None),
        // Выбор цели до первого укуса: Замена и конец дня — через «Отменить».
        A_SWAP | A_END if ctx.cancel_first => (None, cancel),
        A_SWAP => (None, card("replace")),
        A_END => (None, card("end")),
        A_BESTOW => (None, card("bestow")),
        A_INSTIGATE => (None, card("instigate")),
        A_STOPCHAIN => (None, cancel),
        A_TOOL => {
            let id = crate::model::TOOL_IDS[arg as usize];
            let p = ctx.pick?;
            let i = p.cards.iter().position(|c| c.as_deref() == Some(id))?;
            (None, p.boxes.get(i).copied())
        }
        A_GIFT => (None, ctx.gifts.as_ref().and_then(|g| g.get(arg as usize)).copied()),
        A_REROLL => (None, ctx.reroll),
        _ => (None, None),
    };
    (spot.0.is_some() || spot.1.is_some()).then_some(spot)
}

/// Тепловизор: варианты хода (код, среднее итога, отставание от лидера в SE)
/// → жар на экране. Два варианта в одном месте (Замена и конец дня через
/// «Отменить») — показывается лучший из них.
pub fn heat(options: &[(i32, f64, f64)], ctx: &HintContext) -> Vec<HeatSpot> {
    use crate::solver::{SWITCH_GAP, SWITCH_SE};
    let best = options.iter().map(|o| o.1).fold(f64::NEG_INFINITY, f64::max);
    if !best.is_finite() {
        return vec![];
    }
    let shift = ctx.days_left.map_or(0.0, |d| forecast_gap(d, best));
    // Два знака после запятой, только если с одним разные оценки выглядят одинаково.
    let one: Vec<String> = options.iter().map(|o| format!("{:.1}", o.1 + shift)).collect();
    let close = (1..one.len()).any(|i| one[..i].contains(&one[i]));
    let fmt = |v: f64| if close { format!("{v:.2}") } else { format!("{v:.1}") }.replace('.', ",");
    let mut ranked: Vec<&(i32, f64, f64)> = options.iter().collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let mut out: Vec<HeatSpot> = vec![];
    for &&(code, mean, gap_se) in &ranked {
        let Some((cell, card)) = spot_of(code, ctx) else { continue };
        if out.iter().any(|h| h.cell == cell && h.card == card) {
            continue;
        }
        let d = best - mean;
        out.push(HeatSpot {
            cell,
            card,
            label: fmt(mean + shift),
            t: (1.0 - d).clamp(0.25, 1.0),
            near: d > 0.0 && !(d >= SWITCH_GAP && gap_se >= SWITCH_SE),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::{CellKind, CellView, Decision};

    /// Доска 1920×1080, как на кадрах: верх 1–6, справа 7–8, низ 9–14, слева 15–16.
    fn board(decision: Option<Decision>, doors: Vec<usize>) -> BoardView {
        let top = [518.0, 698.0, 870.0, 1044.0, 1218.0, 1392.0];
        let mut cells: Vec<CellAt> = top.iter().map(|&x| CellAt { x, y: 139.0, r: 70.0 }).collect();
        cells.push(CellAt { x: 1400.0, y: 280.0, r: 70.0 });
        cells.push(CellAt { x: 1400.0, y: 418.0, r: 70.0 });
        for &x in [1416.0, 1233.0, 1052.0, 869.0, 686.0, 503.0].iter() {
            cells.push(CellAt { x, y: 560.0, r: 70.0 });
        }
        cells.push(CellAt { x: 512.0, y: 418.0, r: 70.0 });
        cells.push(CellAt { x: 512.0, y: 280.0, r: 70.0 });
        BoardView {
            cells: vec![CellView::new(CellKind::Empty); 16],
            warden: None,
            days_left: None,
            doors,
            decision,
            chain: None,
            tools: vec![],
            tool_slots: None,
            chromatic: -1,
            objective: None,
            at: BoardAt {
                cells,
                door_cards: vec![[448.0, 747.0, 780.0, 925.0], [793.0, 747.0, 1125.0, 925.0], [1138.0, 747.0, 1470.0, 925.0]],
                decision_cards: vec![[621.0, 747.0, 953.0, 925.0], [966.0, 747.0, 1298.0, 925.0]],
                decision_kinds: vec!["consume".into(), "end".into()],
                cancel: None,
            },
        }
    }

    fn manage() -> Option<Decision> {
        Some(Decision::Manage { consume: true, replace: false, end: true, hunt: false })
    }

    #[test]
    fn devour_card_first_then_cell() {
        let b = board(manage(), vec![]);
        let ctx = HintContext { board: Some(&b), warden: Some(14), ..Default::default() };
        let h = hint(A_DEVOUR << 6 | 5, &ctx);
        assert_eq!(h.text, "Пожирание: съесть №6");
        assert_eq!((h.title.as_str(), h.sub.as_str()), ("Пожирание", "затем №6"));
        assert!(h.two_steps());
        assert!(matches!(h.marks[0], HintMark::Box { step: NOW, .. }));
        assert!(matches!(h.marks[1], HintMark::Ring { step: THEN, .. }));
        assert!(!h.marks.iter().any(|m| matches!(m, HintMark::Arrow { .. })), "стрелки на первом шаге нет");
        assert_eq!(h.plate, Some(Plate::Card([621.0, 747.0, 953.0, 925.0])));
    }

    #[test]
    fn target_screen_plate_in_yard_near_cell() {
        let b = board(None, vec![]);
        let ctx = HintContext { board: Some(&b), warden: Some(14), ..Default::default() };
        let h = hint(A_DEVOUR << 6 | 5, &ctx);
        assert_eq!(h.title, "Съесть №6");
        assert!(!h.two_steps());
        assert!(matches!(h.marks[0], HintMark::Ring { step: NOW, .. }));
        assert!(matches!(h.marks[1], HintMark::Arrow { step: SHOW, .. }));
        // Верхний ряд, угол справа: плашка под ним во дворе, не заходя на №7.
        let Some(Plate::Yard { at, anchor }) = h.plate else { panic!("{:?}", h.plate) };
        assert_eq!(anchor, Anchor::Top);
        assert!(at[1] > 139.0 + 70.0 && at[0] + 4.3 * 70.0 / 2.0 <= 1416.0 - 70.0 * 1.6 + 1e-9, "{at:?}");
    }

    #[test]
    fn heat_ranks_options_and_marks_near() {
        let b = board(manage(), vec![]);
        let ctx = HintContext { board: Some(&b), warden: Some(14), ..Default::default() };
        let opts = [(A_END << 6, 4.1, 6.0), (A_DEVOUR << 6 | 5, 4.8, 0.0), (A_DEVOUR << 6 | 13, 4.7, 1.2)];
        let h = heat(&opts, &ctx);
        let labels: Vec<&str> = h.iter().map(|s| s.label.as_str()).collect();
        assert_eq!(labels, ["4,8", "4,7", "4,1"], "лучший первым");
        assert_eq!(h[0].t, 1.0);
        assert!(!h[0].near && h[1].near && !h[2].near, "≈ — только в пределах погрешности");
        assert_eq!(h[0].cell.map(|c| c.x), Some(1392.0));
        assert_eq!(h[2].card, Some([966.0, 747.0, 1298.0, 925.0]), "конец дня — на карточке");
        // Одинаковые при одном знаке — два знака.
        let h = heat(&[(A_DEVOUR << 6 | 5, 4.84, 0.0), (A_DEVOUR << 6 | 13, 4.81, 0.5)], &ctx);
        assert_eq!(h[0].label, "4,84");
        assert_eq!(h[1].label, "4,81");
    }

    /// Поправка на силу решателя: одна на позицию — числа выше, порядок,
    /// жар и «≈» те же; к концу забега и у потолка хранилища — почти ноль.
    #[test]
    fn heat_labels_with_forecast_gap() {
        let b = board(manage(), vec![]);
        let opts = [(A_END << 6, 4.1, 6.0), (A_DEVOUR << 6 | 5, 4.8, 0.0), (A_DEVOUR << 6 | 13, 4.7, 1.2)];
        let raw = heat(&opts, &HintContext { board: Some(&b), warden: Some(14), ..Default::default() });
        let ctx = HintContext { board: Some(&b), warden: Some(14), days_left: Some(19), ..Default::default() };
        let h = heat(&opts, &ctx);
        let shift = forecast_gap(19, 4.8);
        assert!((1.4..1.8).contains(&shift), "{shift}");
        assert_eq!(h[0].label, format!("{:.1}", 4.8 + shift).replace('.', ","));
        for (a, c) in raw.iter().zip(&h) {
            assert_eq!((a.cell.map(|x| x.x), a.card, a.t, a.near), (c.cell.map(|x| x.x), c.card, c.t, c.near));
        }
        assert!(forecast_gap(1, 7.0) < 0.25 && forecast_gap(20, 12.5) < 0.2 && forecast_gap(20, 14.0) == 0.0);
        assert!(forecast_gap(20, 7.0) > 1.6);
    }

    #[test]
    fn patrol_door_card_and_path() {
        let b = board(None, vec![1, 3, 6]);
        let ctx = HintContext { board: Some(&b), warden: Some(0), reason: "событие".into(), ..Default::default() };
        let h = hint(A_PATROL << 6 | 3, &ctx);
        assert_eq!(h.text, "Иди в №4 · событие");
        assert_eq!((h.title.as_str(), h.sub.as_str()), ("Иди в №4", "событие"));
        assert_eq!(h.plate, Some(Plate::Card([793.0, 747.0, 1125.0, 925.0])));
        assert!(matches!(h.marks[1], HintMark::Ring { step: SHOW, .. }));
        let Some(HintMark::Path { points }) = h.marks.last() else { panic!() };
        // Допросная → №2 → №3 → №4: четыре точки по коридору под верхним рядом.
        assert_eq!(points.len(), 4);
        assert!(points.iter().all(|p| p[1] == 139.0 + 70.0 * 1.6));
    }
}
