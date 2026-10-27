//! Экраны без доски: выбор орудий (три карты) и сокровищница (четыре подарка).
//! Перенос `recognize/pick.ts`.

use super::layout::{pick_art, pick_card, pick_title, treasure_art, treasure_card, PICK_CARDS, PICK_HEADER, PICK_REROLL, TREASURE_CARDS};
use super::sample::{box_to_frame, dot, jmax, normalize, sample_into, Box4, Frame, Geometry};
use super::templates::{get, nearest, scores_by_label, ScreensTemplate};
use crate::model::TOOL_IDS;
use crate::view::PickView;

const HEADER_MIN: f64 = 0.6;
const CARD_MIN: f64 = 0.8;
const REROLL_MIN: f64 = 0.6;
const GIFT_MIN: f64 = 0.6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickKind {
    Start,
    Reward,
}

/// Заголовок выбора орудий; None — не выбор.
pub fn pick_header(frame: &Frame, geo: &Geometry, t: &ScreensTemplate) -> Option<PickKind> {
    let mut buf = [0.0f32; 100 * 6];
    sample_into(frame, geo, PICK_HEADER, 100, 6, 1, &mut buf);
    if normalize(&mut buf) < 3.0 {
        return None;
    }
    let (label, score) = nearest(&buf, &t.pick_headers);
    if score < HEADER_MIN {
        return None;
    }
    Some(if label == "reward" { PickKind::Reward } else { PickKind::Start })
}

/// Сдвиги карты по вертикали (опорные px): карта под курсором «всплывает» —
/// в начале анимации на 3–9, к концу на 48 [кадры 26.10 23:44:46, 21:9].
const CARD_DY: [f64; 10] = [0.0, -6.0, -3.0, -9.0, 3.0, -42.0, -45.0, -48.0, -51.0, -54.0];

/// Название карты N (0–2) в пикселях кадра: на месте и у карты под курсором
/// (поднята на 48) — для чтения текстом, когда картинка карты не узнана.
pub fn title_boxes(geo: &Geometry, card: usize) -> [Box4; 2] {
    let cx = PICK_CARDS[card];
    // Двухстрочное мелкое название («Пыточный намордник бешеного...») — с запасом по высоте.
    let b = pick_title(cx);
    let b = [b[0], b[1] - 8.0, b[2], b[3] + 8.0];
    [box_to_frame(geo, b), box_to_frame(geo, shift_y(b, -48.0))]
}

fn shift_y(b: Box4, dy: f64) -> Box4 {
    [b[0], b[1] + dy, b[2], b[3] + dy]
}

/// Карта орудия: сумма сходства картинки и названия по каждому орудию.
fn read_card(frame: &Frame, geo: &Geometry, cx: f64, t: &ScreensTemplate) -> Option<String> {
    let mut id: Option<&str> = None;
    let mut best = CARD_MIN;
    let mut art_buf = [0.0f32; 16 * 16 * 3];
    let mut title_buf = [0.0f32; 86 * 11];
    for dy in CARD_DY {
        // Картинку ищем и с горизонтальным сдвигом.
        let mut art_scores: Vec<(&str, f64)> = vec![];
        let mut best_art = -2.0;
        for dx in [-12.0, -6.0, 0.0, 6.0, 12.0] {
            sample_into(frame, geo, shift_y(pick_art(cx + dx), dy), 16, 16, 3, &mut art_buf);
            normalize(&mut art_buf);
            let s = scores_by_label(&art_buf, &t.pick_art);
            let top = s.iter().fold(f64::NEG_INFINITY, |a, e| jmax(a, e.1));
            if top > best_art {
                best_art = top;
                art_scores = s;
            }
        }
        sample_into(frame, geo, shift_y(pick_title(cx), dy), 86, 11, 1, &mut title_buf);
        normalize(&mut title_buf);
        let title_scores = scores_by_label(&title_buf, &t.pick_titles);
        for &(label, a) in &art_scores {
            let s = (a + jmax(get(&title_scores, label).unwrap_or(0.0), 0.0)) / 2.0;
            if s > best {
                best = s;
                id = Some(label);
            }
        }
    }
    id.filter(|l| TOOL_IDS.contains(l)).map(str::to_string)
}

/// Разбор карты для эталонов (`gorn-cli protos --pick`): по каждому орудию —
/// (сходство картинки, названия, итог) при лучшем сдвиге, и векторы картинки и
/// названия при сдвиге лучшего итога. Тот же поиск, что у `read_card`.
pub fn card_debug(frame: &Frame, geo: &Geometry, cx: f64, t: &ScreensTemplate) -> (Vec<(String, f64, f64, f64)>, Vec<f32>, Vec<f32>) {
    let mut best: Vec<(String, f64, f64, f64)> = vec![];
    let mut top = f64::NEG_INFINITY;
    let mut vecs = (vec![], vec![]);
    let mut art_buf = [0.0f32; 16 * 16 * 3];
    let mut title_buf = [0.0f32; 86 * 11];
    for dy in CARD_DY {
        sample_into(frame, geo, shift_y(pick_title(cx), dy), 86, 11, 1, &mut title_buf);
        normalize(&mut title_buf);
        let title_scores = scores_by_label(&title_buf, &t.pick_titles);
        for dx in [-12.0, -6.0, 0.0, 6.0, 12.0] {
            sample_into(frame, geo, shift_y(pick_art(cx + dx), dy), 16, 16, 3, &mut art_buf);
            normalize(&mut art_buf);
            let art = scores_by_label(&art_buf, &t.pick_art);
            let mut rows: Vec<(String, f64, f64, f64)> = art
                .iter()
                .map(|&(l, a)| {
                    let ti = get(&title_scores, l).unwrap_or(0.0);
                    (l.to_string(), a, ti, (a + jmax(ti, 0.0)) / 2.0)
                })
                .collect();
            rows.sort_by(|a, b| b.3.partial_cmp(&a.3).unwrap_or(std::cmp::Ordering::Equal));
            if rows.first().is_some_and(|r| r.3 > top) {
                top = rows[0].3;
                best = rows;
                vecs = (art_buf.to_vec(), title_buf.to_vec());
            }
        }
    }
    (best, vecs.0, vecs.1)
}

pub fn read_pick(frame: &Frame, geo: &Geometry, t: &ScreensTemplate, kind: PickKind) -> PickView {
    let cards = PICK_CARDS.iter().map(|&cx| read_card(frame, geo, cx, t)).collect();
    let boxes = PICK_CARDS.iter().map(|&cx| box_to_frame(geo, pick_card(cx))).collect();
    let mut buf = [0.0f32; 44 * 10];
    sample_into(frame, geo, PICK_REROLL, 44, 10, 1, &mut buf);
    let mut reroll = false;
    if normalize(&mut buf) >= 3.0 {
        for r in &t.reroll {
            if dot(&buf, r) >= REROLL_MIN {
                reroll = true;
            }
        }
    }
    PickView { cards, boxes, reroll, reward: kind == PickKind::Reward }
}

/// Места четырёх подарков сокровищницы — по геометрии, без пикселей.
pub fn treasure_boxes(geo: &Geometry) -> Vec<Box4> {
    TREASURE_CARDS.iter().map(|&cx| box_to_frame(geo, treasure_card(cx))).collect()
}

/// Кнопка «Обновить» на выборе орудий.
pub fn reroll_box(geo: &Geometry) -> Box4 {
    box_to_frame(geo, PICK_REROLL)
}

/// Заголовок окна события («Цепная реакция») справа от картинки — для чтения
/// текстом [кадр 27.10 10:06:21, 21:9: заголовок 1302–1580 × 217–255 опорных].
pub fn event_title_box(geo: &Geometry) -> Box4 {
    box_to_frame(geo, [1200.0, 198.0, 1690.0, 272.0])
}

/// Сокровищница: все четыре места похожи на подарки.
pub fn read_treasure(frame: &Frame, geo: &Geometry, t: &ScreensTemplate) -> Option<Vec<Box4>> {
    let mut buf = [0.0f32; 16 * 16 * 3];
    for &cx in &TREASURE_CARDS {
        sample_into(frame, geo, treasure_art(cx), 16, 16, 3, &mut buf);
        normalize(&mut buf);
        if nearest(&buf, &t.gifts).1 < GIFT_MIN {
            return None;
        }
    }
    Some(treasure_boxes(geo))
}
