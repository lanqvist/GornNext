//! Распознавание экрана тюрьмы: кадр → ScreenView. Перенос `recognize/`
//! (gorn-tli) бит в бит: выборка, нормировка и пороги — те же, буферы f32
//! как Float32Array эталона.
//!
//! Геометрию (где доска) ищет `calibrate` — редко: первая доска, смена
//! разрешения. `read` принимает найденную раньше геометрию и возвращает ту,
//! что стоит хранить дальше.

pub mod board;
pub mod cards;
pub mod cells;
pub mod geometry;
pub mod layout;
pub mod panels;
pub mod pick;
pub mod sample;
pub mod templates;
pub mod text;

pub use geometry::{calibrate, default_geometry};
pub use pick::{reroll_box, treasure_boxes};
pub use sample::{Frame, Geometry};
pub use templates::templates;

use crate::view::ScreenView;
use board::read_board;
use geometry::board_score;
use pick::{pick_header, read_pick, read_treasure};

/// Доля подписей номеров, при которой экран считается доской.
const BOARD_MIN: f64 = 0.6;

#[derive(Clone, Copy, Debug, Default)]
pub struct ReadOptions {
    /// Здесь ждём доску (по логу или хоткей), либо прежняя геометрия уже не
    /// раз не нашла её: при том же размере кадра подписей на месте нет —
    /// искать доску заново (масштаб интерфейса меняется без смены разрешения).
    pub recalibrate: bool,
}

/// Что на кадре. geo — геометрия с прошлых кадров (или None). Возвращает
/// экран и геометрию для следующих кадров: найденную доску или прежнюю.
pub fn read(frame: &Frame, geo: Option<&Geometry>, opts: ReadOptions) -> (ScreenView, Option<Geometry>) {
    let t = templates();
    let bt = &t.board;
    let known = geo.filter(|g| g.width == frame.width && g.height == frame.height).copied();

    if let Some(k) = &known {
        if board_score(frame, k, bt) >= BOARD_MIN {
            return (ScreenView::Board(read_board(frame, k, t)), known);
        }
    }

    // Экраны без доски — в том же масштабе интерфейса, что и доска.
    let plain = known.unwrap_or_else(|| default_geometry(frame));
    // Сокровищница раньше выбора: её заголовок стоит там же.
    if let Some(gifts) = read_treasure(frame, &plain, &t.screens) {
        return (ScreenView::Treasure(gifts), known);
    }
    if let Some(kind) = pick_header(frame, &plain, &t.screens) {
        return (ScreenView::Pick(read_pick(frame, &plain, &t.screens, kind)), known);
    }

    // При том же размере кадра доска обычно стоит на месте: раз подписей нет,
    // это другой экран. Калибровка — без геометрии или когда доску здесь ждём.
    if known.is_some() && !opts.recalibrate {
        return (ScreenView::Other, known);
    }
    if let Some(found) = calibrate(frame, bt) {
        if found.score >= BOARD_MIN {
            return (ScreenView::Board(read_board(frame, &found, t)), Some(found));
        }
    }
    (ScreenView::Other, known)
}
