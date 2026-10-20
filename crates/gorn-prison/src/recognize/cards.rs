//! Ряд карточек под доской: двери и решения. Карточки ищутся по подписям.
//! Перенос `recognize/cards.ts`.

use super::sample::{dot, normalize, sample_into, Box4, Frame, Geometry};
use super::templates::Proto;
use super::text::{glyphs, read_digit, DigitSet, Glyph, InkOptions};

/// Полоса подписей карточек (опорные px).
pub const CARD_TITLE: Box4 = [440.0, 862.0, 1480.0, 904.0];
pub const CARD_Y0: f64 = 748.0;
pub const CARD_Y1: f64 = 926.0;
pub const CARD_HALF_W: f64 = 166.0;

/// Центры карточек при 1, 2, 3 и 4 карточках.
pub fn slots(n: usize) -> &'static [f64] {
    match n {
        4 => &[442.0, 787.0, 1133.0, 1478.0],
        3 => &[614.0, 959.0, 1304.0],
        2 => &[787.0, 1132.0],
        1 => &[959.0],
        _ => &[],
    }
}

pub fn card_box(cx: f64) -> Box4 {
    [cx - CARD_HALF_W, CARD_Y0, cx + CARD_HALF_W, CARD_Y1]
}

/// Подписи карточек: группы знаков, разделённые промежутком шире 40 опорных px.
pub fn title_groups(frame: &Frame, geo: &Geometry) -> Vec<Vec<Glyph>> {
    let gs: Vec<Glyph> = glyphs(frame, geo, CARD_TITLE, InkOptions { contrast: 70.0, max_sat: 0.6, faint: true })
        .into_iter()
        .filter(|g| (g.y1 - g.y0) as f64 >= 8.0 * geo.s && (g.x1 - g.x0) as f64 >= 2.0 * geo.s)
        .collect();
    let mut groups: Vec<Vec<Glyph>> = vec![];
    for g in gs {
        match groups.last_mut() {
            Some(last) if ((g.x0 - last.last().unwrap().x1) as f64) < 40.0 * geo.s => last.push(g),
            _ => groups.push(vec![g]),
        }
    }
    // Подпись — хотя бы 5 знаков (короче — блики свечей, курсор).
    groups.into_iter().filter(|gr| gr.len() >= 5).collect()
}

/// Центры карточек (опорные px) по подписям; пусто — карточек нет.
pub fn card_centers(frame: &Frame, geo: &Geometry) -> Vec<f64> {
    let groups = title_groups(frame, geo);
    if groups.is_empty() || groups.len() > 4 {
        return vec![];
    }
    let centers: Vec<f64> =
        groups.iter().map(|gr| ((gr[0].x0 + gr[gr.len() - 1].x1) as f64 / 2.0 - geo.tx) / geo.s).collect();
    let sl = slots(groups.len());
    for i in 0..sl.len() {
        if (centers[i] - sl[i]).abs() > 60.0 {
            return vec![];
        }
    }
    sl.to_vec()
}

/// Номер камеры на карточке двери: 1–16, 0 — допросная. None — подпись не читается.
pub fn read_door_number(frame: &Frame, geo: &Geometry, cx: f64, digits: &DigitSet) -> Option<i32> {
    let all: Vec<Glyph> =
        glyphs(frame, geo, [cx - 160.0, CARD_TITLE[1], cx + 160.0, CARD_TITLE[3]], InkOptions { contrast: 70.0, max_sat: 0.3, faint: true })
            .into_iter()
            .filter(|g| (g.y1 - g.y0) as f64 >= 8.0 * geo.s && (g.x1 - g.x0) as f64 >= 2.0 * geo.s)
            .collect();
    // Подпись — самая длинная цепочка знаков без широких промежутков.
    let mut gs: Vec<Glyph> = vec![];
    let mut run: Vec<Glyph> = vec![];
    for g in all {
        if let Some(last) = run.last() {
            if (g.x0 - last.x1) as f64 > 14.0 * geo.s {
                run = vec![];
            }
        }
        run.push(g);
        if run.len() > gs.len() {
            gs = run.clone();
        }
    }
    if gs.len() < 5 {
        return None;
    }
    let mut bottoms: Vec<i64> = gs.iter().map(|g| g.y1).collect();
    bottoms.sort();
    let base = bottoms[bottoms.len() >> 1];
    // Высота строчных букв — нижняя четверть знаков на строке; цифры заметно выше.
    let mut heights: Vec<i64> =
        gs.iter().filter(|g| ((g.y1 - base).abs() as f64) <= 2.0 * geo.s).map(|g| g.y1 - g.y0).collect();
    heights.sort();
    let xh = heights.get(heights.len() >> 2).copied().unwrap_or(0) as f64;
    let mut n = String::new();
    for g in gs.iter().rev() {
        if ((g.y1 - base).abs() as f64) > 2.0 * geo.s || ((g.y1 - g.y0) as f64) < 1.25 * xh {
            break;
        }
        let (d, _) = read_digit(frame, g, digits, 0.4);
        if d < 0 {
            break;
        }
        n.insert_str(0, &d.to_string());
    }
    // «Пойти в допросную» — длинная подпись без цифр; короткая без цифр не прочитана.
    if n.is_empty() {
        return if gs.len() >= 13 { Some(0) } else { None };
    }
    let v: f64 = n.parse().ok()?;
    if (2.0..=16.0).contains(&v) {
        Some(v as i32)
    } else {
        None
    }
}

/// Значок карточки решения → класс по прототипам.
pub fn classify_card<'a>(frame: &Frame, geo: &Geometry, cx: f64, protos: &'a [Proto]) -> (&'a str, f64) {
    let mut buf = [0.0f32; 12 * 12 * 3];
    sample_into(frame, geo, [cx - 48.0, 770.0, cx + 48.0, 866.0], 12, 12, 3, &mut buf);
    normalize(&mut buf);
    let mut label = "";
    let mut score = -2.0;
    for p in protos {
        let s = dot(&buf, &p.v);
        if s > score {
            score = s;
            label = &p.label;
        }
    }
    (label, score)
}
