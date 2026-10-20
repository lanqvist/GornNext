//! Калибровка: где доска тюрьмы на кадре. Перенос `recognize/geometry.ts`.
//!
//! 1. Грубо — карта перепадов яркости против усреднённой карты своих кадров,
//!    ячейка 16 опорных px, несколько масштабов вокруг «по высоте кадра».
//! 2. Точно — подписи номеров камер 1–16 рядом с грубым местом, масштаб и
//!    сдвиг наименьшими квадратами.
//!
//! На следующих кадрах калибровка не нужна: `board_score` проверяет подписи
//! в известном месте.

use std::sync::OnceLock;

use super::layout::NUMBER;
use super::sample::{dot, jmin, jround, normalize, sample_into, Frame, Geometry};
use super::templates::BoardTemplate;

/// Подпись номера: рамка вокруг центра, опорные px.
pub const NUMBER_RX: f64 = 10.0;
pub const NUMBER_RY: f64 = 9.0;

/// Геометрия по умолчанию: масштаб по высоте, по ширине по центру.
pub fn default_geometry(frame: &Frame) -> Geometry {
    let (w, h) = (frame.width as f64, frame.height as f64);
    let s = jmin(h / 1080.0, w / 1920.0);
    Geometry { width: frame.width, height: frame.height, s, tx: (w - 1920.0 * s) / 2.0, ty: (h - 1080.0 * s) / 2.0, score: 0.0 }
}

/// Сходство подписи номера i с эталоном, если подпись стоит в (x, y) опорных.
fn number_at(frame: &Frame, geo: &Geometry, tpl: &BoardTemplate, i: usize, x: f64, y: f64) -> f64 {
    let mut buf = [0.0f32; 20 * 18];
    sample_into(frame, geo, [x - NUMBER_RX, y - NUMBER_RY, x + NUMBER_RX, y + NUMBER_RY], 20, 18, 1, &mut buf);
    if normalize(&mut buf) < 3.0 {
        return 0.0;
    }
    dot(&buf, &tpl.numbers[i])
}

/// Доля подписей номеров, найденных на своих местах (0…1).
pub fn board_score(frame: &Frame, geo: &Geometry, tpl: &BoardTemplate) -> f64 {
    let mut ok = 0;
    for (i, &(x, y)) in NUMBER.iter().enumerate() {
        if number_at(frame, geo, tpl, i, x, y) > 0.6 {
            ok += 1;
        }
    }
    ok as f64 / NUMBER.len() as f64
}

struct Map {
    w: usize,
    h: usize,
    g: Vec<f32>,
}

/// Карта перепадов яркости кадра: ячейка cell опорных px при масштабе s.
fn gradient_map(frame: &Frame, s: f64, cell: f64) -> Map {
    let step = cell * s;
    let w = (frame.width as f64 / step).floor() as usize;
    let h = (frame.height as f64 / step).floor() as usize;
    let geo = Geometry { width: frame.width, height: frame.height, s: 1.0, tx: 0.0, ty: 0.0, score: 0.0 };
    let mut lum = vec![0.0f32; w * h];
    sample_into(frame, &geo, [0.0, 0.0, w as f64 * step, h as f64 * step], w, h, 1, &mut lum);
    Map { w, h, g: gradient(&lum, w, h) }
}

pub fn gradient(lum: &[f32], w: usize, h: usize) -> Vec<f32> {
    let mut g = vec![0.0f32; w * h];
    for y in 1..h.saturating_sub(1) {
        for x in 1..w.saturating_sub(1) {
            let i = y * w + x;
            let gx = lum[i + 1] as f64 - lum[i - 1] as f64;
            let gy = lum[i + w] as f64 - lum[i - w] as f64;
            g[i] = (gx * gx + gy * gy).sqrt() as f32;
        }
    }
    g
}

/// Уменьшить карту вдвое (среднее 2×2).
fn pool2(src: &[f32], w: usize, h: usize) -> Map {
    let (w2, h2) = (w >> 1, h >> 1);
    let mut g = vec![0.0f32; w2 * h2];
    for y in 0..h2 {
        for x in 0..w2 {
            let i = 2 * y * w + 2 * x;
            g[y * w2 + x] = ((src[i] as f64 + src[i + 1] as f64 + src[i + w] as f64 + src[i + w + 1] as f64) / 4.0) as f32;
        }
    }
    Map { w: w2, h: h2, g }
}

struct Level {
    w: usize,
    h: usize,
    cell: f64,
    x0: f64,
    y0: f64,
    /// Нормированный шаблон.
    t: Vec<f32>,
}

/// Шаблон карты перепадов на ячейках 8 и 16 опорных px.
fn levels(tpl: &BoardTemplate) -> &'static (Level, Level) {
    static L: OnceLock<(Level, Level)> = OnceLock::new();
    L.get_or_init(|| {
        let g = &tpl.grad;
        let src: Vec<f32> = g.data.iter().map(|&b| b as f32).collect();
        let p8 = pool2(&src, g.w, g.h);
        let p16 = pool2(&p8.g, p8.w, p8.h);
        let make = |p: Map, cell: f64| {
            let mut t = p.g;
            normalize(&mut t);
            Level { w: p.w, h: p.h, cell, x0: g.x0, y0: g.y0, t }
        };
        (make(p8, g.cell * 2.0), make(p16, g.cell * 4.0))
    })
}

/// Лучший сдвиг шаблона на карте (ячейки), NCC: (score, x, y).
fn best_shift(map: &Map, c: &Level, cx: i64, cy: i64, rx: i64, ry: i64) -> (f64, i64, i64) {
    let mut best = (-1.0, cx, cy);
    let n = (c.w * c.h) as f64;
    for oy in cy - ry..=cy + ry {
        if oy < 0 || oy as usize + c.h > map.h {
            continue;
        }
        for ox in cx - rx..=cx + rx {
            if ox < 0 || ox as usize + c.w > map.w {
                continue;
            }
            let (mut sum, mut sq, mut cross) = (0.0f64, 0.0f64, 0.0f64);
            for y in 0..c.h {
                let k0 = (oy as usize + y) * map.w + ox as usize;
                let t0 = y * c.w;
                for x in 0..c.w {
                    let v = map.g[k0 + x] as f64;
                    sum += v;
                    sq += v * v;
                    cross += v * c.t[t0 + x] as f64;
                }
            }
            let var_sum = sq - (sum * sum) / n;
            if var_sum <= 0.0 {
                continue;
            }
            // Шаблон с нулевым средним: вычитать среднее из окна не нужно.
            let score = cross / var_sum.sqrt();
            if score > best.0 {
                best = (score, ox, oy);
            }
        }
    }
    best
}

/// Масштабы относительно «по высоте кадра», которые пробует калибровка.
const SCALES: [f64; 9] = [1.0, 0.95, 1.05, 0.9, 1.1, 0.85, 0.8, 0.75, 0.7];

/// Ниже этого сходства карты перепадов доски на кадре нет.
const COARSE_MIN: f64 = 0.35;

/// Найти доску на кадре. None — доски нет (или она не узнаётся).
pub fn calibrate(frame: &Frame, tpl: &BoardTemplate) -> Option<Geometry> {
    let base = default_geometry(frame);
    if let Some(q) = refine(frame, &base, tpl, 6) {
        if q.score >= 0.85 {
            return Some(q);
        }
    }
    let (l8, l16) = levels(tpl);
    let (fw, fh) = (frame.width as f64, frame.height as f64);
    // Грубо: ячейка 16 px, все масштабы, сдвиг до ±15 % кадра.
    let mut cands: Vec<(f64, f64, i64, i64)> = vec![];
    for k in SCALES {
        let s = base.s * k;
        let map = gradient_map(frame, s, l16.cell);
        let cx = jround((l16.x0 * s + (fw - 1920.0 * s) / 2.0) / (l16.cell * s)) as i64;
        let cy = jround((l16.y0 * s + (fh - 1080.0 * s) / 2.0) / (l16.cell * s)) as i64;
        let rx = (map.w as f64 * 0.15).ceil() as i64;
        let ry = (map.h as f64 * 0.15).ceil() as i64;
        let r = best_shift(&map, l16, cx, cy, rx, ry);
        cands.push((r.0, s, r.1, r.2));
    }
    // По убыванию сходства, равные — в прежнем порядке (сортировка JS устойчивая).
    cands.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    for &(score, s, x, y) in cands.iter().take(2) {
        if score < COARSE_MIN {
            break;
        }
        // Точнее: ячейка 8 px рядом с найденным местом.
        let map = gradient_map(frame, s, l8.cell);
        let r = best_shift(&map, l8, x * 2, y * 2, 2, 2);
        let geo = Geometry {
            width: frame.width,
            height: frame.height,
            s,
            tx: (r.1 as f64 * l8.cell - l8.x0) * s,
            ty: (r.2 as f64 * l8.cell - l8.y0) * s,
            score: 0.0,
        };
        if let Some(fine) = refine(frame, &geo, tpl, 8) {
            if fine.score >= 0.6 {
                return Some(fine);
            }
        }
    }
    None
}

/// Уточнить геометрию по подписям номеров: каждую ищем в радиусе r опорных px.
fn refine(frame: &Frame, geo: &Geometry, tpl: &BoardTemplate, r: i32) -> Option<Geometry> {
    let mut pts: Vec<(f64, f64, f64, f64)> = vec![];
    for (i, &(nx, ny)) in NUMBER.iter().enumerate() {
        let mut best = 0.6;
        let (mut bx, mut by) = (0.0, 0.0);
        let mut found = false;
        for dy in -r..=r {
            for dx in -r..=r {
                let s = number_at(frame, geo, tpl, i, nx + dx as f64, ny + dy as f64);
                if s > best {
                    best = s;
                    bx = dx as f64;
                    by = dy as f64;
                    found = true;
                }
            }
        }
        if found {
            pts.push((nx, ny, (nx + bx) * geo.s + geo.tx, (ny + by) * geo.s + geo.ty));
        }
    }
    if pts.len() < 6 {
        return None;
    }
    // fx = s·x + tx, fy = s·y + ty — общий масштаб, наименьшие квадраты.
    let (mut mx, mut my, mut mfx, mut mfy) = (0.0, 0.0, 0.0, 0.0);
    for p in &pts {
        mx += p.0;
        my += p.1;
        mfx += p.2;
        mfy += p.3;
    }
    let n = pts.len() as f64;
    mx /= n;
    my /= n;
    mfx /= n;
    mfy /= n;
    let (mut num, mut den) = (0.0, 0.0);
    for p in &pts {
        num += (p.0 - mx) * (p.2 - mfx) + (p.1 - my) * (p.3 - mfy);
        den += (p.0 - mx) * (p.0 - mx) + (p.1 - my) * (p.1 - my);
    }
    let s = num / den;
    let mut out = Geometry { width: frame.width, height: frame.height, s, tx: mfx - s * mx, ty: mfy - s * my, score: 0.0 };
    out.score = board_score(frame, &out, tpl);
    Some(out)
}
