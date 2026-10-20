//! Светлый текст на тёмном фоне: разбивка строки на знаки по столбцам и
//! распознавание цифр по эталонам. Работает в пикселях кадра, пороги
//! масштабируются по geo.s. Перенос `recognize/text.ts`.

use super::sample::{jmax, jmin, jround, normalize, Box4, Frame, Geometry};

/// Рамка знака в пикселях кадра.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Glyph {
    pub x0: i64,
    pub y0: i64,
    pub x1: i64,
    pub y1: i64,
}

/// Размер нормированной картинки знака.
pub const GLYPH_W: usize = 8;
pub const GLYPH_H: usize = 12;

#[derive(Clone, Copy, Debug)]
pub struct InkOptions {
    /// Насколько текст ярче медианы строки.
    pub contrast: f64,
    /// Бледный текст: порог снижается до 45 % разницы между медианой и самыми яркими, но не ниже 25.
    pub faint: bool,
    /// Насыщенность текста не выше.
    pub max_sat: f64,
}

/// Знаки строки: область (опорные координаты) → рамки знаков слева направо.
pub fn glyphs(frame: &Frame, geo: &Geometry, b: Box4, opt: InkOptions) -> Vec<Glyph> {
    let fx0 = jmax(0.0, jround(b[0] * geo.s + geo.tx)) as i64;
    let fy0 = jmax(0.0, jround(b[1] * geo.s + geo.ty)) as i64;
    let fx1 = jmin(frame.width as f64, jround(b[2] * geo.s + geo.tx)) as i64;
    let fy1 = jmin(frame.height as f64, jround(b[3] * geo.s + geo.ty)) as i64;
    let w = fx1 - fx0;
    let h = fy1 - fy0;
    if w <= 0 || h <= 0 {
        return vec![];
    }
    let (w, h) = (w as usize, h as usize);
    let (data, width) = (frame.data, frame.width);
    let mut lum = vec![0.0f32; w * h];
    let mut ink = vec![0u8; w * h];
    let mut hist = [0u32; 256];
    for y in 0..h {
        let mut k = ((fy0 as usize + y) * width + fx0 as usize) * 4;
        for x in 0..w {
            let l = (data[k] as f64 + data[k + 1] as f64 + data[k + 2] as f64) / 3.0;
            lum[y * w + x] = l as f32;
            hist[l as usize] += 1;
            k += 4;
        }
    }
    let n = (w * h) as f64;
    let mut acc = 0u64;
    let mut median = 0usize;
    while median < 256 {
        acc += hist[median] as u64;
        if acc as f64 * 2.0 >= n {
            break;
        }
        median += 1;
    }
    let mut top = 255usize;
    acc = 0;
    while top > 0 {
        acc += hist[top] as u64;
        if acc as f64 >= n * 0.01 {
            break;
        }
        top -= 1;
    }
    let (median, top) = (median as f64, top as f64);
    let thr = median + if opt.faint { jmax(25.0, jmin(opt.contrast, 0.45 * (top - median))) } else { opt.contrast };
    for y in 0..h {
        let mut k = ((fy0 as usize + y) * width + fx0 as usize) * 4;
        for x in 0..w {
            let (r, g, bl) = (data[k] as i32, data[k + 1] as i32, data[k + 2] as i32);
            let max = r.max(g).max(bl);
            let min = r.min(g).min(bl);
            ink[y * w + x] =
                (lum[y * w + x] as f64 > thr && max > 0 && (max - min) as f64 / max as f64 <= opt.max_sat) as u8;
            k += 4;
        }
    }
    let mut out = vec![];
    let mut start: i64 = -1;
    for x in 0..=w {
        let mut has = false;
        if x < w {
            for y in 0..h {
                if ink[y * w + x] != 0 {
                    has = true;
                    break;
                }
            }
        }
        if has && start < 0 {
            start = x as i64;
        }
        if !has && start >= 0 {
            let mut y0 = h as i64;
            let mut y1: i64 = -1;
            for xx in start as usize..x {
                for y in 0..h {
                    if ink[y * w + xx] != 0 {
                        y0 = y0.min(y as i64);
                        y1 = y1.max(y as i64);
                    }
                }
            }
            out.push(Glyph { x0: fx0 + start, y0: fy0 + y0, x1: fx0 + x as i64, y1: fy0 + y1 + 1 });
            start = -1;
        }
    }
    out
}

/// Картинка знака GLYPH_W×GLYPH_H по его рамке (среднее по ячейкам), нормированная.
pub fn glyph_vec(frame: &Frame, g: &Glyph, out: &mut [f32]) {
    let (data, width) = (frame.data, frame.width as i64);
    let cw = (g.x1 - g.x0) as f64 / GLYPH_W as f64;
    let ch = (g.y1 - g.y0) as f64 / GLYPH_H as f64;
    let clamp = |x: f64| -> usize { jround(x).clamp(1.0, 4.0) as usize };
    let nx = clamp(cw);
    let ny = clamp(ch);
    let mut o = 0;
    for j in 0..GLYPH_H {
        for i in 0..GLYPH_W {
            let mut s = 0.0f64;
            for v in 0..ny {
                let py = (g.y0 as f64 + (j as f64 + (v as f64 + 0.5) / ny as f64) * ch).floor() as i64;
                for uu in 0..nx {
                    let px = (g.x0 as f64 + (i as f64 + (uu as f64 + 0.5) / nx as f64) * cw).floor() as i64;
                    let k = ((py * width + px) * 4) as usize;
                    s += data[k] as f64 + data[k + 1] as f64 + data[k + 2] as f64;
                }
            }
            out[o] = (s / (3 * nx * ny) as f64) as f32;
            o += 1;
        }
    }
    normalize(out);
}

/// Эталоны цифр: (цифра, нормированная картинка) и среднее отношение сторон для 0–9.
pub struct DigitSet {
    pub protos: Vec<(usize, Vec<f32>)>,
    pub aspect: Vec<f64>,
}

/// Цифра знака и сходство; −1, если знак не похож ни на одну цифру.
pub fn read_digit(frame: &Frame, g: &Glyph, set: &DigitSet, min: f64) -> (i32, f64) {
    let mut buf = [0.0f32; GLYPH_W * GLYPH_H];
    glyph_vec(frame, g, &mut buf);
    let aspect = (g.x1 - g.x0) as f64 / (g.y1 - g.y0).max(1) as f64;
    let mut d = -1;
    let mut score = min;
    for (pd, v) in &set.protos {
        // Отношение сторон отсекает буквы и «№» с похожим рисунком.
        if (aspect - set.aspect[*pd]).abs() > 0.22 {
            continue;
        }
        let mut s = 0.0f64;
        for i in 0..buf.len() {
            s += buf[i] as f64 * v[i] as f64;
        }
        if s > score {
            score = s;
            d = *pd as i32;
        }
    }
    (d, score)
}
