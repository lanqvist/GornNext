//! Доска тюрьмы: камеры, надзиратель, значки, ряд карточек. Работает при
//! известной геометрии и читает только маленькие области кадра.
//! Перенос `recognize/board.ts`.

use super::cards::{card_box, card_centers, classify_card, read_door_number};
use super::cells::{
    border_color, chain_jaw_share, color_share, event_share, face_vec, glow, is_gold, jaw_vec, lock_score, prey_vec, FACE_LEN, NB,
};
use super::layout::{portrait, CANCEL, CELL_R, CHAIN_BANNER, DAYS, HEADER, HEADER_H, HEADER_W, WARDEN};
use super::panels::{read_chromatic, read_objective, read_tool_slots};
use super::sample::{box_to_frame, dot, hsv, normalize, sample_into, to_frame, Frame, Geometry};
use super::templates::{nearest, CellsTemplate, ScreensTemplate, Templates};
use super::text::{glyphs, read_digit, DigitSet, InkOptions};
use crate::view::{BoardAt, BoardView, CellAt, CellKind, CellView, Decision};

const BEASTS: [&str; 4] = ["mammoth", "lion", "wolf", "rat"];

/// Пороги, подобранные по своим кадрам.
const LOCK_SURE: f64 = 0.5;
const LOCK_MAYBE: f64 = 0.25;
const PORTRAIT_MIN: f64 = 0.5;
const EVENT_MIN: f64 = 0.08;
const JAW_MIN: f64 = 0.63;
const CHAIN_JAW_MIN: f64 = 0.05;
const WARDEN_MIN: f64 = 0.12;
const PREY_MIN: f64 = 0.7;
/// Яркость рамки портрета, ниже которой редкость считается не нарисованной.
const DARK_BORDER: f64 = 0.3;

const G_RED: usize = 1;
const G_ORANGE: usize = 2;
const G_YELLOW: usize = 3;
const G_GREEN: usize = 4;

/// Лучший класс портрета и его сходство.
pub fn classify_face<'a>(frame: &Frame, geo: &Geometry, cell: usize, t: &'a CellsTemplate) -> (&'a str, f64) {
    let mut buf = [0.0f32; FACE_LEN];
    face_vec(frame, geo, cell, &mut buf);
    let mut label = "";
    let mut score = -2.0;
    for p in &t.beasts {
        let s = dot(&buf, &p.v);
        if s > score {
            score = s;
            label = &p.label;
        }
    }
    (label, score)
}

/// Редкость по рамке портрета и свечению вокруг.
pub fn read_tier(frame: &Frame, geo: &Geometry, cell: usize, chain_target: bool) -> u8 {
    let (r, g, b) = border_color(frame, geo, cell);
    let (h, s, _) = hsv(r, g, b);
    let gl = glow(frame, geo, cell);
    let both = 2 * NB;
    if gl[both + G_GREEN] as f64 > 0.04 && !chain_target {
        return 5;
    }
    if s >= 0.12 && (180.0..252.0).contains(&h) {
        return 1;
    }
    if s >= 0.12 && (252.0..330.0).contains(&h) {
        return 2;
    }
    if s < 0.3 {
        return 0;
    }
    // На цели цепочки вокруг портрета красное пламя клыков: смотрим только рамку.
    if chain_target {
        return if (12.0..70.0).contains(&h) { 3 } else { 4 };
    }
    let red = gl[both + G_RED] as f64;
    let orange = gl[both + G_ORANGE] as f64 + gl[both + G_YELLOW] as f64;
    if red > orange {
        return 4;
    }
    if orange > red {
        return 3;
    }
    if (12.0..70.0).contains(&h) {
        3
    } else {
        4
    }
}

/// Одна камера (не допросная). chain — экран цепочки.
pub fn read_cell(frame: &Frame, geo: &Geometry, cell: usize, t: &CellsTemplate, chain: bool) -> CellView {
    let lock = lock_score(frame, geo, cell, &t.lock);
    let (label, score) = classify_face(frame, geo, cell, t);
    let mut view = CellView::new(CellKind::Empty);
    let beast = BEASTS.iter().position(|&b| b == label);
    let is_beast = beast.is_some() && score >= PORTRAIT_MIN;
    if lock >= LOCK_SURE || (lock >= LOCK_MAYBE && !is_beast) {
        view.kind = CellKind::Locked;
        if label == "unknown" && score >= PORTRAIT_MIN {
            view.shown_tier = Some(read_tier(frame, geo, cell, false));
        }
    } else if is_beast {
        view.kind = CellKind::Inmate;
        view.beast = beast.map(|b| b as u8);
        let target = chain && chain_jaw_share(frame, geo, cell) >= CHAIN_JAW_MIN;
        let tier = read_tier(frame, geo, cell, target);
        view.tier = Some(tier);
        // Тёмная бронзовая рамка без свечения — редкость не нарисована.
        if tier == 0 && !chain {
            let (r, g, b) = border_color(frame, geo, cell);
            if hsv(r, g, b).2 < DARK_BORDER {
                view.tier_unknown = Some(true);
            }
        }
        if chain {
            view.jaws = Some(target);
        } else {
            let mut buf = [0.0f32; 22 * 15];
            jaw_vec(frame, geo, cell, &mut buf);
            view.jaws = Some(dot(&buf, &t.jaw) >= JAW_MIN);
        }
    }
    if event_share(frame, geo, cell) >= EVENT_MIN {
        view.event = Some(true);
    }
    view
}

/// Камера надзирателя: где ярче всего золото стрелки.
pub fn read_warden(frame: &Frame, geo: &Geometry) -> Option<usize> {
    let mut best: Option<usize> = None;
    let mut best_share = WARDEN_MIN;
    for (c, &(x, y)) in WARDEN.iter().enumerate() {
        let share = color_share(frame, geo, [x - 10.0, y - 10.0, x + 10.0, y + 10.0], 2.0, is_gold);
        if share > best_share {
            best_share = share;
            best = Some(c);
        }
    }
    best
}

pub fn read_cells(frame: &Frame, geo: &Geometry, t: &CellsTemplate, chain: bool) -> Vec<CellView> {
    let mut cells = vec![CellView::new(CellKind::Office)];
    for c in 1..16 {
        cells.push(read_cell(frame, geo, c, t, chain));
    }
    cells
}

// ── вся доска ───────────────────────────────────────────────────────────────

const CHAIN_RED: f64 = 0.3;
const HEADER_MIN: f64 = 0.6;
const CARD_MIN: f64 = 0.6;

fn is_banner_red(r: f64, g: f64, b: f64) -> bool {
    r > 90.0 && r > 2.0 * g && r > 2.0 * b
}

/// Экран цепочки: красная полоса «Выберите узника для пожирания».
pub fn is_chain(frame: &Frame, geo: &Geometry) -> bool {
    color_share(frame, geo, CHAIN_BANNER, 4.0, is_banner_red) >= CHAIN_RED
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Header {
    Patrol,
    Manage,
    Ratway,
}

/// Надпись над карточками.
pub fn read_header(frame: &Frame, geo: &Geometry, t: &ScreensTemplate) -> Option<Header> {
    let mut buf = [0.0f32; HEADER_W * HEADER_H];
    sample_into(frame, geo, HEADER, HEADER_W, HEADER_H, 1, &mut buf);
    if normalize(&mut buf) < 3.0 {
        return None;
    }
    let (label, score) = nearest(&buf, &t.headers);
    if score < HEADER_MIN {
        return None;
    }
    match label {
        "patrol" => Some(Header::Patrol),
        "manage" => Some(Header::Manage),
        "ratway" => Some(Header::Ratway),
        _ => None,
    }
}

/// Дни до бунта: крупные цифры сверху.
pub fn read_days(frame: &Frame, geo: &Geometry, digits: &DigitSet) -> Option<i32> {
    let gs: Vec<_> = glyphs(frame, geo, DAYS, InkOptions { contrast: 50.0, max_sat: 1.0, faint: false })
        .into_iter()
        .filter(|g| (g.y1 - g.y0) as f64 >= 24.0 * geo.s)
        .collect();
    if gs.is_empty() || gs.len() > 2 {
        return None;
    }
    let mut s = String::new();
    for g in &gs {
        let (d, _) = read_digit(frame, g, digits, 0.6);
        if d < 0 {
            return None;
        }
        s.push_str(&d.to_string());
    }
    let n: i32 = s.parse().ok()?;
    (1..=30).contains(&n).then_some(n)
}

/// Опорный центр камеры для меток: портрет, у допросной — середина комнаты.
fn cell_center(c: usize) -> (f64, f64) {
    if c == 0 {
        (518.0, 140.0)
    } else {
        portrait(c)
    }
}

fn reachable(from: usize, to: usize) -> bool {
    let d = (to + 16 - from) % 16;
    (1..=6).contains(&d)
}

fn looks_like_doors(frame: &Frame, geo: &Geometry, centers: &[f64], warden: Option<usize>, t: &Templates) -> bool {
    let Some(warden) = warden else { return false };
    if centers.is_empty() {
        return false;
    }
    let mut office = 0;
    for &cx in centers {
        if classify_card(frame, geo, cx, &t.screens.cards).1 >= CARD_MIN {
            return false;
        }
        let Some(n) = read_door_number(frame, geo, cx, &t.digits) else { return false };
        if !reachable(warden, if n == 0 { 0 } else { (n - 1) as usize }) {
            return false;
        }
        if n == 0 {
            office += 1;
        }
    }
    office <= 1
}

/// Доска целиком при известной геометрии.
pub fn read_board(frame: &Frame, geo: &Geometry, t: &Templates) -> BoardView {
    let chain = is_chain(frame, geo);
    let mut header = if chain { None } else { read_header(frame, geo, &t.screens) };
    let mut cells = read_cells(frame, geo, &t.cells, chain);
    // Метка «Добыча»: прицел внизу портрета.
    if let Some(prey) = &t.screens.prey {
        let mut buf = [0.0f32; 16 * 18 * 3];
        for (n, c) in cells.iter_mut().enumerate() {
            if c.kind == CellKind::Inmate {
                prey_vec(frame, geo, n, &mut buf);
                if dot(&buf, prey) >= PREY_MIN {
                    c.prey = Some(true);
                }
            }
        }
    }
    let mut at = BoardAt {
        cells: (0..cells.len())
            .map(|c| {
                let (cx, cy) = cell_center(c);
                let (x, y) = to_frame(geo, cx, cy);
                CellAt { x, y, r: CELL_R * geo.s }
            })
            .collect(),
        door_cards: vec![],
        decision_cards: vec![],
        decision_kinds: vec![],
        cancel: chain.then(|| box_to_frame(geo, CANCEL)),
    };
    // На экране цепочки панели скрыты; видна строка дней — читаем и панели.
    let days_left = read_days(frame, geo, &t.digits);
    let panels = !chain || days_left.is_some();
    let tool_slots = panels.then(|| read_tool_slots(frame, geo, &t.screens));
    let warden = read_warden(frame, geo);
    let mut board = BoardView {
        cells,
        warden,
        days_left,
        doors: vec![],
        decision: None,
        chain: None,
        tools: tool_slots.as_ref().map(|s| s.iter().flatten().cloned().collect()).unwrap_or_default(),
        tool_slots,
        chromatic: if panels { read_chromatic(frame, geo, &t.screens) } else { -1 },
        objective: if panels { read_objective(frame, geo, &t.screens) } else { None },
        at: BoardAt { cells: vec![], door_cards: vec![], decision_cards: vec![], decision_kinds: vec![], cancel: None },
    };
    if chain {
        board.chain = Some(board.cells.iter().enumerate().filter(|(_, c)| c.jaws == Some(true)).map(|(i, _)| i).collect());
        board.at = at;
        return board;
    }
    if header == Some(Header::Ratway) {
        board.decision = Some(Decision::Ratway);
        board.at = at;
        return board;
    }
    let centers = card_centers(frame, geo);
    // Надпись не узнана, но каждая карточка читается как дверь в пределах хода — выбор двери.
    if header.is_none() && looks_like_doors(frame, geo, &centers, board.warden, t) {
        header = Some(Header::Patrol);
    }
    // Ряд решений без надписи: каждая карточка уверенно узнаётся как карточка решения.
    if header.is_none()
        && !centers.is_empty()
        && centers.iter().all(|&cx| classify_card(frame, geo, cx, &t.screens.cards).1 >= CARD_MIN)
    {
        header = Some(Header::Manage);
    }
    match header {
        None => {}
        Some(Header::Patrol) => {
            for &cx in &centers {
                let Some(n) = read_door_number(frame, geo, cx, &t.digits) else { continue };
                let cell = if n == 0 { 0 } else { (n - 1) as usize };
                // Двери — на 1–6 шагов вперёд от надзирателя: иное — ошибка чтения.
                if let Some(w) = board.warden {
                    if !reachable(w, cell) {
                        continue;
                    }
                }
                board.doors.push(cell);
                at.door_cards.push(box_to_frame(geo, card_box(cx)));
            }
        }
        Some(_) => {
            let mut kinds: Vec<String> = vec![];
            for &cx in &centers {
                let (label, score) = classify_card(frame, geo, cx, &t.screens.cards);
                if score < CARD_MIN {
                    continue;
                }
                kinds.push(label.to_string());
                at.decision_cards.push(box_to_frame(geo, card_box(cx)));
                at.decision_kinds.push(label.to_string());
            }
            let has = |k: &str| kinds.iter().any(|x| x == k);
            if has("bestow") || has("instigate") {
                board.decision = Some(Decision::Office);
            } else if !kinds.is_empty() {
                board.decision =
                    Some(Decision::Manage { consume: has("consume"), replace: has("replace"), end: has("end"), hunt: has("hunt") });
            }
        }
    }
    board.at = at;
    board
}
