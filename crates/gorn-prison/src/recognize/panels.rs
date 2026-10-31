//! Панели вокруг доски: орудия слева, хранилище призматических справа, цель.
//! Игра прижимает их к краям экрана: на 21:9 — `Geometry::left` / `right`.
//! Перенос `recognize/panels.ts`.

use super::layout::{chromatic_slot, tool_slot, CHROMATIC_HEADER, OBJECTIVE, OBJECTIVE_TEXT, TOOL_SLOTS};
use super::sample::{box_to_frame, dot, jmax, normalize, sample_into, Box4, Frame, Geometry};
use super::templates::{get, nearest, scores_by_label, ScreensTemplate};
use crate::model::TOOL_IDS;

const TOOL_MIN: f64 = 0.75;
/// Контраст места орудия, ниже которого там фон: у иконок на своих кадрах — от 5,1.
const BLANK_CONTRAST: f64 = 4.5;
const OBJECTIVE_MIN: f64 = 0.8;
/// Занятое место хранилища — от 0,58 (и в просмотре тюрьмы); фон — до 0,3.
const FILLED_MIN: f64 = 0.4;
/// Средняя яркость места (0–255), при которой это вспышка влетающей призмы:
/// почти белое пятно (240–254), портрет — 80–120, пустое — около 20.
const FLASH_MIN: f64 = 200.0;
/// Заголовок «Призматический узник» на обычной доске — 0,8 и выше. Было 0,6: с
/// эталоном английского «Chromatic Prisoner» фон скрытой панели давал 0,60–0,65,
/// панель читалась пустой, и хранилище в памяти обнулялось [кадры 31.10, 6 раз за 6 забегов].
const HEADER_MIN: f64 = 0.75;

/// Орудия, у которых есть эталон иконки на панели.
pub fn panel_tool_ids(t: &ScreensTemplate) -> Vec<&'static str> {
    TOOL_IDS.iter().copied().filter(|id| t.tools.iter().any(|p| p.label == *id)).collect()
}

/// Места панели орудий слева направо: орудие или None — иконка без эталона.
/// Первое пустое место — конец списка.
pub fn read_tool_slots(frame: &Frame, geo: &Geometry, t: &ScreensTemplate) -> Vec<Option<String>> {
    let geo = &geo.left();
    let mut out = vec![];
    let mut buf = [0.0f32; 12 * 18 * 3];
    for i in 0..TOOL_SLOTS {
        sample_into(frame, geo, tool_slot(i), 12, 18, 3, &mut buf);
        let contrast = normalize(&mut buf);
        let (label, score) = nearest(&buf, &t.tools);
        // Эталон пустого места — фон за ним на 16:9; на 21:9 за местом другой
        // кусок фона. Неузнанное и блёклое место — тоже пустое.
        if label == "empty" || (score < TOOL_MIN && contrast < BLANK_CONTRAST) {
            break;
        }
        out.push((score >= TOOL_MIN && TOOL_IDS.contains(&label)).then(|| label.to_string()));
    }
    out
}

/// Призматических в хранилище; −1 — панель скрыта.
pub fn read_chromatic(frame: &Frame, geo: &Geometry, t: &ScreensTemplate) -> i32 {
    let geo = &geo.right();
    let mut head_buf = [0.0f32; 92 * 20];
    sample_into(frame, geo, CHROMATIC_HEADER, 92, 20, 1, &mut head_buf);
    normalize(&mut head_buf);
    let mut head = -1.0;
    for h in &t.chromatic_header {
        head = jmax(head, dot(&head_buf, h));
    }
    let mut filled = [false; 10];
    let mut flash = None;
    let mut buf = [0.0f32; 12 * 16 * 3];
    for (i, f) in filled.iter_mut().enumerate() {
        sample_into(frame, geo, chromatic_slot(i), 12, 16, 3, &mut buf);
        let bright = buf.iter().map(|&x| x as f64).sum::<f64>() / buf.len() as f64;
        if bright >= FLASH_MIN {
            flash = Some(i);
        }
        normalize(&mut buf);
        let s = scores_by_label(&buf, &t.chromatic);
        let fill = get(&s, "filled").unwrap_or(-2.0);
        // Без порога фон за скрытой панелью (21:9) иногда чуть ближе к «занято».
        *f = fill >= FILLED_MIN && fill > get(&s, "empty").unwrap_or(-2.0);
    }
    // Призмы влетают в хранилище по порядку, вспышка — последние из них; её
    // свечение задевает следующие пустые места, и они читаются занятыми
    // [кадры 26.10 21:48:06 и 23:51:14, 21:9: 4 вместо 2 и 8 вместо 6].
    if let Some(last) = flash {
        return last as i32 + 1;
    }
    // Хранилище заполняется по порядку: пустое место, за которым два занятых подряд, — занято.
    for i in (0..=7).rev() {
        if filled[i + 1] && filled[i + 2] {
            filled[i] = true;
        }
    }
    // Одинокое занятое после пустого — отблеск (свеча, вспышка нового узника).
    let n = filled.iter().take_while(|&&f| f).count() as i32;
    if n == 0 && head < HEADER_MIN {
        -1
    } else {
        n
    }
}

/// Описание цели с прогрессом «N/M» — прямоугольник кадра для чтения текстом.
pub fn objective_text_box(geo: &Geometry) -> Box4 {
    box_to_frame(&geo.left(), OBJECTIVE_TEXT)
}

/// Отпечаток описания цели: пока он тот же, прогресс не перечитывается.
pub fn objective_text_print(frame: &Frame, geo: &Geometry) -> Vec<f32> {
    let mut buf = vec![0.0f32; 64 * 12];
    sample_into(frame, &geo.left(), OBJECTIVE_TEXT, 64, 12, 1, &mut buf);
    normalize(&mut buf);
    buf
}

/// Цель забега; None — не узнана.
pub fn read_objective(frame: &Frame, geo: &Geometry, t: &ScreensTemplate) -> Option<String> {
    let geo = &geo.left();
    let mut buf = [0.0f32; 80 * 15];
    sample_into(frame, geo, OBJECTIVE, 80, 15, 1, &mut buf);
    if normalize(&mut buf) < 3.0 {
        return None;
    }
    let (label, score) = nearest(&buf, &t.objectives);
    (score >= OBJECTIVE_MIN).then(|| label.to_string())
}
