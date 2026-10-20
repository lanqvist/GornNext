//! Признаки одной камеры: замок, портрет узника, свечение редкости, значки
//! («!», клыки). Только выборка и числа; решения — в board.rs.
//! Перенос `recognize/cells.ts`.

use super::layout::{portrait, LOCK};
use super::sample::{dot, jmin, jround, normalize, sample_into, Box4, Frame, Geometry};

/// Замок: рамка вокруг центра значка, опорные px.
pub const LOCK_RX: f64 = 9.0;
pub const LOCK_RY: f64 = 13.5;

/// Сходство места замка камеры с эталоном замка (−1…1), лучшее в ±1 px.
pub fn lock_score(frame: &Frame, geo: &Geometry, cell: usize, tpl: &[f32]) -> f64 {
    let (x, y) = LOCK[cell];
    let mut best = -1.0;
    let mut buf = [0.0f32; 18 * 27];
    for dy in -1..=1 {
        for dx in -1..=1 {
            let (dx, dy) = (dx as f64, dy as f64);
            sample_into(frame, geo, [x + dx - LOCK_RX, y + dy - LOCK_RY, x + dx + LOCK_RX, y + dy + LOCK_RY], 18, 27, 1, &mut buf);
            if normalize(&mut buf) < 4.0 {
                continue;
            }
            let s = dot(&buf, tpl);
            if s > best {
                best = s;
            }
        }
    }
    best
}

/// Лицо на портрете: без клыков сверху и без рамки по бокам.
pub const FACE: (f64, f64, f64, f64, usize, usize) = (-22.0, -20.0, 22.0, 30.0, 11, 12);
pub const FACE_LEN: usize = 11 * 12 * 3;

/// Нормированная цветная картинка лица на портрете камеры.
pub fn face_vec(frame: &Frame, geo: &Geometry, cell: usize, out: &mut [f32]) {
    let (x, y) = portrait(cell);
    let (x0, y0, x1, y1, w, h) = FACE;
    sample_into(frame, geo, [x + x0, y + y0, x + x1, y + y1], w, h, 3, out);
    normalize(out);
}

/// Корзины цвета свечения: white, red, orange, yellow, green, blue, purple, pink.
pub const NB: usize = 8;

fn bin(r: f64, g: f64, b: f64) -> i32 {
    let max = if r > g {
        if r > b {
            r
        } else {
            b
        }
    } else if g > b {
        g
    } else {
        b
    };
    let min = if r < g {
        if r < b {
            r
        } else {
            b
        }
    } else if g < b {
        g
    } else {
        b
    };
    let v = max / 255.0;
    let s = if max > 0.0 { (max - min) / max } else { 0.0 };
    if s < 0.18 {
        return if v > 0.5 { 0 } else { -1 };
    }
    if s < 0.4 || v < 0.15 {
        return -1;
    }
    let d = max - min;
    let h = if max == r {
        (((g - b) / d + 6.0) % 6.0) * 60.0
    } else if max == g {
        ((b - r) / d + 2.0) * 60.0
    } else {
        ((r - g) / d + 4.0) * 60.0
    };
    if !(15.0..335.0).contains(&h) {
        return 1;
    }
    if h < 45.0 {
        return 2;
    }
    if h < 70.0 {
        return 3;
    }
    if h < 170.0 {
        return 4;
    }
    if h < 248.0 {
        return 5;
    }
    if h < 300.0 {
        return 6;
    }
    7
}

/// Области свечения вокруг портрета (опорные px от центра портрета).
pub const GLOW_REGIONS: [(f64, f64, f64, f64); 4] =
    [(-44.0, -36.0, -30.0, 20.0), (30.0, -36.0, 44.0, 20.0), (-36.0, -58.0, -18.0, -40.0), (18.0, -58.0, 36.0, -40.0)];
const GLOW_CELL: f64 = 2.0;

/// Доли пикселей каждой корзины цвета: [часть × корзина], части — бока, верх, всё вместе.
pub fn glow(frame: &Frame, geo: &Geometry, cell: usize) -> [f32; 3 * NB] {
    let mut out = [0.0f32; 3 * NB];
    let (px, py) = portrait(cell);
    let mut buf = [0.0f32; 32 * 32 * 3];
    for (r, &(x0, y0, x1, y1)) in GLOW_REGIONS.iter().enumerate() {
        let w = ((x1 - x0) / GLOW_CELL) as usize;
        let h = ((y1 - y0) / GLOW_CELL) as usize;
        sample_into(frame, geo, [px + x0, py + y0, px + x1, py + y1], w, h, 3, &mut buf);
        let base = if r < 2 { 0 } else { NB }; // бока вместе, верх вместе
        for i in 0..w * h {
            let k = bin(buf[i * 3] as f64, buf[i * 3 + 1] as f64, buf[i * 3 + 2] as f64);
            if k >= 0 {
                out[base + k as usize] = (out[base + k as usize] as f64 + 1.0) as f32;
            }
        }
    }
    // Нормировка на число пикселей каждой пары областей.
    let side = 2.0 * ((14.0 * 56.0) / (GLOW_CELL * GLOW_CELL));
    let top = 2.0 * ((18.0 * 18.0) / (GLOW_CELL * GLOW_CELL));
    for k in 0..NB {
        out[k] = (out[k] as f64 / side) as f32;
        out[NB + k] = (out[NB + k] as f64 / top) as f32;
    }
    for k in 0..NB {
        out[2 * NB + k] = ((out[k] as f64 * side + out[NB + k] as f64 * top) / (side + top)) as f32;
    }
    out
}

/// Доля пикселей области, подходящих под условие на цвет (выборка с шагом cell опорных px).
pub fn color_share(frame: &Frame, geo: &Geometry, b: Box4, cell: f64, test: fn(f64, f64, f64) -> bool) -> f64 {
    let w = jround((b[2] - b[0]) / cell).max(1.0) as usize;
    let h = jround((b[3] - b[1]) / cell).max(1.0) as usize;
    let mut buf = vec![0.0f32; w * h * 3];
    sample_into(frame, geo, b, w, h, 3, &mut buf);
    let mut n = 0;
    for i in 0..w * h {
        if test(buf[i * 3] as f64, buf[i * 3 + 1] as f64, buf[i * 3 + 2] as f64) {
            n += 1;
        }
    }
    n as f64 / (w * h) as f64
}

/// Оранжевый «!» события.
pub fn is_event_orange(r: f64, g: f64, b: f64) -> bool {
    r > 170.0 && r > g && g > b && (r - b) / r > 0.55 && g - b > 0.35 * (r - b) && g - b < 0.85 * (r - b)
}

/// Красно-розовые клыки над портретом.
pub fn is_jaw_red(r: f64, g: f64, b: f64) -> bool {
    r > 140.0 && r > g * 1.6 && r > b * 1.5
}

/// Золото стрелки надзирателя.
pub fn is_gold(r: f64, g: f64, b: f64) -> bool {
    let max = if r > g {
        if r > b {
            r
        } else {
            b
        }
    } else if g > b {
        g
    } else {
        b
    };
    let min = if r < g {
        if r < b {
            r
        } else {
            b
        }
    } else if g < b {
        g
    } else {
        b
    };
    max > 120.0 && (max - min) / max > 0.45 && r >= g && g > b * 1.3 && r - g < 100.0
}

/// «!» события: правее камеры, чуть выше замка.
pub const EVENT_DX: f64 = 69.0;
pub const EVENT_DY: f64 = -22.0;

pub fn event_share(frame: &Frame, geo: &Geometry, cell: usize) -> f64 {
    let (x, y) = LOCK[cell];
    color_share(
        frame,
        geo,
        [x + EVENT_DX - 6.0, y + EVENT_DY - 15.0, x + EVENT_DX + 6.0, y + EVENT_DY + 15.0],
        2.0,
        is_event_orange,
    )
}

/// Клыки над портретом: серая картинка 22×15.
pub const JAW: (f64, f64, f64, f64, usize, usize) = (-22.0, -46.0, 22.0, -16.0, 22, 15);

pub fn jaw_vec(frame: &Frame, geo: &Geometry, cell: usize, out: &mut [f32]) {
    let (x, y) = portrait(cell);
    let (x0, y0, x1, y1, w, h) = JAW;
    sample_into(frame, geo, [x + x0, y + y0, x + x1, y + y1], w, h, 1, out);
    normalize(out);
}

/// Метка «Добыча» (золотой прицел внизу портрета): цветная картинка 16×18.
pub const PREY: (f64, f64, f64, f64, usize, usize) = (-16.0, 24.0, 16.0, 60.0, 16, 18);

pub fn prey_vec(frame: &Frame, geo: &Geometry, cell: usize, out: &mut [f32]) {
    let (x, y) = portrait(cell);
    let (x0, y0, x1, y1, w, h) = PREY;
    sample_into(frame, geo, [x + x0, y + y0, x + x1, y + y1], w, h, 3, out);
    normalize(out);
}

/// Пламенные клыки цепочки вокруг портрета-цели: красное по бокам и снизу.
pub fn chain_jaw_share(frame: &Frame, geo: &Geometry, cell: usize) -> f64 {
    let (x, y) = portrait(cell);
    let a = color_share(frame, geo, [x - 30.0, y + 46.0, x + 30.0, y + 64.0], 2.0, is_jaw_red);
    let b = color_share(frame, geo, [x - 30.0, y - 70.0, x + 30.0, y - 50.0], 2.0, is_jaw_red);
    jmin(a, b)
}

/// Средний цвет рамки портрета (узкие полосы по бокам), R, G, B.
pub fn border_color(frame: &Frame, geo: &Geometry, cell: usize) -> (f64, f64, f64) {
    let (x, y) = portrait(cell);
    let mut a = [0.0f32; 3];
    let mut b = [0.0f32; 3];
    sample_into(frame, geo, [x - 32.0, y - 24.0, x - 29.0, y + 24.0], 1, 1, 3, &mut a);
    sample_into(frame, geo, [x + 29.0, y - 24.0, x + 32.0, y + 24.0], 1, 1, 3, &mut b);
    (
        (a[0] as f64 + b[0] as f64) / 2.0,
        (a[1] as f64 + b[1] as f64) / 2.0,
        (a[2] as f64 + b[2] as f64) / 2.0,
    )
}
