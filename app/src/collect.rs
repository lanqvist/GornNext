//! Сбор всех экранов тюрьмы (для перевода на английский клиент): пока игрок в
//! тюрьме, каждый новый экран сохраняется в PNG, рядом JSON — строки лога с
//! прошлого кадра и день. Без советника и разметки: по этим кадрам снимаются
//! эталоны надписей (заголовки окон, карточки, цели, названия орудий) для
//! другого языка клиента.
//!
//! Кадр берётся из того же потока экрана, что у подсказок, без ожидания: как
//! только экран успокоился после смены. Экран, почти не отличимый от прошлого
//! сохранённого, не пишется; анимация без конца — не чаще FORCE.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::json;

use crate::capture::{Image, ScreenStream};

/// Экран не менялся столько — снимаем.
const QUIET: Duration = Duration::from_millis(450);
/// Экран так и не успокоился — снимаем как есть, но не чаще.
const FORCE: Duration = Duration::from_secs(4);
/// Как часто смотреть на поток.
const EVERY: Duration = Duration::from_millis(150);
const SIG_W: usize = 128;
const SIG_H: usize = 72;
/// Точка отпечатка «другая», если яркость разошлась больше чем на это.
const CELL_DIFF: i32 = 20;
/// Экран новый, если другая хотя бы такая доля точек (0,6 % — ~55 из 9 216:
/// всплывшая подсказка или сменившийся заголовок; огонь факелов — меньше).
const NEW_SHARE: f64 = 0.006;
const MAX_LINES: usize = 80;

pub struct Collect {
    pub root: PathBuf,
    saved_sig: Option<Vec<u8>>,
    /// Смена экрана, по которой уже решали (снят кадр или пропущен как тот же).
    seen_change: Option<Option<Instant>>,
    /// Первая ещё не снятая смена экрана.
    pending_since: Option<Instant>,
    last_look: Option<Instant>,
    /// Строки лога тюрьмы с прошлого кадра.
    lines: Vec<String>,
    /// Хоткей: следующий кадр — без сравнения с прошлым.
    force: bool,
    pub saved: u64,
}

impl Collect {
    pub fn new(root: PathBuf) -> Self {
        Collect {
            root,
            saved_sig: None,
            seen_change: None,
            pending_since: None,
            last_look: None,
            lines: vec![],
            force: false,
            saved: 0,
        }
    }

    pub fn lines(&mut self, lines: &[String]) {
        for l in lines {
            if l.contains("S16") || l.contains("PageStack") || l.contains("Eating_red_screen") {
                self.lines.push(l.clone());
            }
        }
        if self.lines.len() > MAX_LINES {
            let cut = self.lines.len() - MAX_LINES;
            self.lines.drain(..cut);
        }
    }

    /// Снять следующий кадр в любом случае (хоткей «подсказка сейчас»).
    pub fn force(&mut self) {
        self.force = true;
    }

    /// Вышли из тюрьмы: первый экран в следующий раз — заново.
    pub fn reset(&mut self) {
        self.saved_sig = None;
        self.seen_change = None;
        self.pending_since = None;
        self.lines.clear();
    }

    /// Посмотреть на поток и снять экран, если он новый и успокоился.
    pub fn tick(&mut self, stream: &ScreenStream, day: Option<(i32, i32)>) {
        let now = Instant::now();
        if self.last_look.is_some_and(|t| now - t < EVERY) {
            return;
        }
        self.last_look = Some(now);
        let Some((img, changed, quiet)) = stream.peek(QUIET) else {
            return;
        };
        if !self.force && self.seen_change == Some(changed) {
            return;
        }
        let pending = *self.pending_since.get_or_insert(now);
        let forced = now - pending >= FORCE;
        if !quiet && !forced && !self.force {
            return;
        }
        self.seen_change = Some(changed);
        self.pending_since = None;
        let sig = signature(&img);
        let manual = std::mem::take(&mut self.force);
        if !manual && self.saved_sig.as_ref().is_some_and(|old| share_differs(old, &sig) < NEW_SHARE) {
            return;
        }
        self.saved_sig = Some(sig);
        self.save(img, quiet, manual, day);
    }

    fn save(&mut self, img: std::sync::Arc<Image>, quiet: bool, manual: bool, day: Option<(i32, i32)>) {
        let now = chrono::Local::now();
        let base = self
            .root
            .join(now.format("%Y-%m-%d").to_string())
            .join(format!("{}-collect", now.format("%H%M%S-%3f")));
        let info = json!({
            "kind": "collect",
            "manual": manual,
            "settled": quiet,
            "size": [img.width, img.height],
            "day": day.map(|(c, m)| json!({ "cur": c, "max": m })),
            "lines": std::mem::take(&mut self.lines),
        });
        crate::shots::write_info(&base, &info);
        let png = base.with_extension("png");
        std::thread::spawn(move || {
            if let Err(e) = crate::shots::write_png(&png, &img) {
                eprintln!("[тюрьма] сбор: кадр не сохранён: {e}");
            }
        });
        self.saved += 1;
    }
}

/// Яркость 128×72 точек сетки.
fn signature(img: &Image) -> Vec<u8> {
    let (w, h) = (img.width, img.height);
    let mut sig = vec![0u8; SIG_W * SIG_H];
    if w == 0 || h == 0 {
        return sig;
    }
    for j in 0..SIG_H {
        let y = (j * 2 + 1) * h / (SIG_H * 2);
        for i in 0..SIG_W {
            let x = (i * 2 + 1) * w / (SIG_W * 2);
            let k = (y * w + x) * 4;
            let d = &img.data;
            sig[j * SIG_W + i] = ((d[k] as u32 + d[k + 1] as u32 + d[k + 2] as u32) / 3) as u8;
        }
    }
    sig
}

/// Доля точек отпечатка, разошедшихся заметно.
fn share_differs(a: &[u8], b: &[u8]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 1.0;
    }
    let n = a
        .iter()
        .zip(b)
        .filter(|(x, y)| (**x as i32 - **y as i32).abs() > CELL_DIFF)
        .count();
    n as f64 / a.len() as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_change_is_same_screen() {
        let mut a = Image::whole(256, 144, vec![40; 256 * 144 * 4]);
        let sa = signature(&a);
        // Огонёк 6×6 пикселей — тот же экран.
        for y in 10..16 {
            for x in 10..16 {
                let k = (y * 256 + x) * 4;
                a.data[k..k + 3].copy_from_slice(&[250, 250, 250]);
            }
        }
        assert!(share_differs(&sa, &signature(&a)) < NEW_SHARE);
        // Окно на треть экрана — новый.
        for y in 40..100 {
            for x in 60..200 {
                let k = (y * 256 + x) * 4;
                a.data[k..k + 3].copy_from_slice(&[200, 180, 120]);
            }
        }
        assert!(share_differs(&sa, &signature(&a)) >= NEW_SHARE);
    }
}
