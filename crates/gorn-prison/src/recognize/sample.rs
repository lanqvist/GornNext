//! Выборка пикселей в опорных координатах: маленькие картинки из областей
//! кадра (среднее по ячейке, чтобы 4K и 1080p давали одно и то же),
//! нормировка и сравнение. Перенос `recognize/sample.ts`.
//!
//! Буферы — f32, как Float32Array эталона: каждая запись округляется до f32,
//! а счёт между ними идёт в f64 (числа JS). Так результат совпадает до бита.

/// Кадр: RGBA построчно, как ImageData.
#[derive(Clone, Copy)]
pub struct Frame<'a> {
    pub width: usize,
    pub height: usize,
    pub data: &'a [u8],
}

/// Где интерфейс тюрьмы на кадре: точка опорных координат (x, y) 1920×1080
/// лежит в (s·x + tx, s·y + ty).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geometry {
    pub width: usize,
    pub height: usize,
    pub s: f64,
    pub tx: f64,
    pub ty: f64,
    /// Насколько уверенно найдена доска (0…1), для отладки.
    pub score: f64,
}

pub type Box4 = [f64; 4];

/// `Math.round` из JS: ближайшее целое, половина — вверх.
#[inline]
pub fn jround(x: f64) -> f64 {
    let r = x.floor();
    if x - r >= 0.5 {
        r + 1.0
    } else {
        r
    }
}

/// `Math.max` из JS для двух чисел.
#[inline]
pub fn jmax(a: f64, b: f64) -> f64 {
    crate::policy::instant::jmax(a, b)
}

/// `Math.min` из JS для двух чисел: NaN заразен, −0 меньше +0.
#[inline]
pub fn jmin(a: f64, b: f64) -> f64 {
    if a < b {
        a
    } else if b < a {
        b
    } else if a == b {
        if a == 0.0 && a.is_sign_positive() {
            b
        } else {
            a
        }
    } else {
        f64::NAN
    }
}

/// Сколько точек на сторону ячейки брать при усреднении (не больше 4).
#[inline]
fn steps(span: f64) -> usize {
    let r = jround(span);
    if r < 1.0 {
        1
    } else if r > 4.0 {
        4
    } else {
        r as usize
    }
}

/// Область (x0, y0)–(x1, y1) опорных координат → картинка w×h в out.
/// channels 1: яркость (R+G+B)/3; 3: R, G, B подряд по точкам.
pub fn sample_into(frame: &Frame, geo: &Geometry, b: Box4, w: usize, h: usize, channels: usize, out: &mut [f32]) {
    let (data, width, height) = (frame.data, frame.width as i64, frame.height as i64);
    let [x0, y0, x1, y1] = b;
    let cw = ((x1 - x0) / w as f64) * geo.s;
    let ch = ((y1 - y0) / h as f64) * geo.s;
    let nx = steps(cw);
    let ny = steps(ch);
    let fx0 = x0 * geo.s + geo.tx;
    let fy0 = y0 * geo.s + geo.ty;
    let inv = 1.0 / (nx * ny) as f64;
    let mut o = 0;
    for j in 0..h {
        for i in 0..w {
            let (mut r, mut g, mut bl) = (0.0f64, 0.0f64, 0.0f64);
            for v in 0..ny {
                let mut py = (fy0 + (j as f64 + (v as f64 + 0.5) / ny as f64) * ch).floor() as i64;
                if py < 0 {
                    py = 0;
                } else if py >= height {
                    py = height - 1;
                }
                let row = py * width;
                for uu in 0..nx {
                    let mut px = (fx0 + (i as f64 + (uu as f64 + 0.5) / nx as f64) * cw).floor() as i64;
                    if px < 0 {
                        px = 0;
                    } else if px >= width {
                        px = width - 1;
                    }
                    let k = ((row + px) * 4) as usize;
                    r += data[k] as f64;
                    g += data[k + 1] as f64;
                    bl += data[k + 2] as f64;
                }
            }
            if channels == 1 {
                out[o] = ((r + g + bl) * inv * (1.0 / 3.0)) as f32;
                o += 1;
            } else {
                out[o] = (r * inv) as f32;
                out[o + 1] = (g * inv) as f32;
                out[o + 2] = (bl * inv) as f32;
                o += 3;
            }
        }
    }
}

/// То же в новый буфер.
pub fn sample(frame: &Frame, geo: &Geometry, b: Box4, w: usize, h: usize, channels: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; w * h * channels];
    sample_into(frame, geo, b, w, h, channels, &mut out);
    out
}

/// Вычесть среднее и привести к единичной длине; вернуть разброс до нормировки.
pub fn normalize(v: &mut [f32]) -> f64 {
    let mut m = 0.0f64;
    for &x in v.iter() {
        m += x as f64;
    }
    m /= v.len() as f64;
    let mut s = 0.0f64;
    for x in v.iter_mut() {
        let d = *x as f64 - m;
        *x = d as f32;
        s += d * d;
    }
    let n = s.sqrt();
    if n > 1e-6 {
        for x in v.iter_mut() {
            *x = (*x as f64 / n) as f32;
        }
    }
    (s / v.len() as f64).sqrt()
}

#[inline]
pub fn dot(a: &[f32], b: &[f32]) -> f64 {
    let mut s = 0.0f64;
    for i in 0..a.len() {
        s += a[i] as f64 * b[i] as f64;
    }
    s
}

/// Перевод опорных координат в пиксели кадра.
#[inline]
pub fn to_frame(geo: &Geometry, x: f64, y: f64) -> (f64, f64) {
    (x * geo.s + geo.tx, y * geo.s + geo.ty)
}

pub fn box_to_frame(geo: &Geometry, b: Box4) -> Box4 {
    [b[0] * geo.s + geo.tx, b[1] * geo.s + geo.ty, b[2] * geo.s + geo.tx, b[3] * geo.s + geo.ty]
}

/// Оттенок (0…360), насыщенность и яркость (0…1) цвета.
pub fn hsv(r: f64, g: f64, b: f64) -> (f64, f64, f64) {
    let max = jmax(jmax(r, g), b);
    let min = jmin(jmin(r, g), b);
    let d = max - min;
    let mut h = 0.0;
    if d > 0.0 {
        if max == r {
            h = ((g - b) / d + 6.0) % 6.0;
        } else if max == g {
            h = (b - r) / d + 2.0;
        } else {
            h = (r - g) / d + 4.0;
        }
        h *= 60.0;
    }
    (h, if max > 0.0 { d / max } else { 0.0 }, max / 255.0)
}
