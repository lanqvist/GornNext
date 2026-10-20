//! Кадры тюрьмы на диск: PNG и JSON рядом — вид кадра, строки лога, которые
//! его вызвали, и ответы советника. Тот же формат, что у Горна на Electron:
//! `gorn-cli audit --shots ПАПКА` и `replay` разбирают их без переделок.
//! Очистка — правила `core/prisonShots.ts`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use serde_json::{json, Value};

use crate::capture::Image;
use crate::triggers::PrisonShot;

pub struct Shots {
    pub root: PathBuf,
    /// Кадр → (путь без расширения, описание).
    saving: BTreeMap<u64, (PathBuf, Value)>,
}

impl Shots {
    pub fn new(root: PathBuf) -> Self {
        Shots { root, saving: BTreeMap::new() }
    }

    /// Запомнить кадр: описание — сразу (следующий кадр может прервать расчёт),
    /// картинка — в фоне. Возвращает путь без расширения.
    pub fn note(&mut self, id: u64, kind: &str, shot: &PrisonShot, frame: Option<Arc<Image>>) -> PathBuf {
        let now = chrono::Local::now();
        let base = self.root.join(now.format("%Y-%m-%d").to_string()).join(format!("{}-{kind}", now.format("%H%M%S-%3f")));
        let info = json!({
            "kind": kind,
            "fresh": shot.fresh,
            "after": shot.after.name(),
            "treasureView": shot.treasure_view,
            "treasureClosed": shot.treasure_closed,
            "rerolled": shot.rerolled,
            "theme": shot.theme,
            "day": shot.day.map(|(c, m)| json!({ "cur": c, "max": m })),
            "lines": shot.lines,
            "hints": [],
        });
        write_info(&base, &info);
        if let Some(img) = frame {
            let png = base.with_extension("png");
            std::thread::spawn(move || {
                if let Err(e) = write_png(&png, &img) {
                    eprintln!("[тюрьма] кадр не сохранён: {e}");
                }
            });
        }
        self.saving.insert(id, (base.clone(), info));
        // Прерванные кадры ответа не дождутся — не копить их.
        while self.saving.len() > 50 {
            let first = *self.saving.keys().next().unwrap();
            self.saving.remove(&first);
        }
        base
    }

    /// Ответ советника — в описание кадра; итоговый ответ записывает файл.
    pub fn reply(&mut self, id: u64, reply: Value, done: bool) {
        let Some((_, info)) = self.saving.get_mut(&id) else { return };
        if let Some(h) = info["hints"].as_array_mut() {
            h.push(reply);
        }
        if done {
            let (base, info) = self.saving.remove(&id).unwrap();
            write_info(&base, &info);
        }
    }
}

fn write_info(base: &Path, info: &Value) {
    if let Some(dir) = base.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(e) = std::fs::write(base.with_extension("json"), serde_json::to_string_pretty(info).unwrap()) {
        eprintln!("[тюрьма] описание кадра не сохранено: {e}");
    }
}

fn write_png(path: &Path, img: &Image) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let f = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(f), img.width as u32, img.height as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_compression(png::Compression::Fast);
    let mut w = enc.write_header().map_err(|e| e.to_string())?;
    w.write_image_data(&img.data).map_err(|e| e.to_string())
}

// ── очистка ─────────────────────────────────────────────────────────────────

const HOUR: Duration = Duration::from_secs(3600);
/// Картинка кадра с подсказкой живёт 2 часа, сбоев и ручных — 7 дней, описания — 30 дней.
const HINT_PNG: Duration = Duration::from_secs(2 * 3600);
const MISS_PNG: Duration = Duration::from_secs(7 * 24 * 3600);
const JSON_AGE: Duration = Duration::from_secs(30 * 24 * 3600);
/// Сверх потолка удаляются самые старые картинки.
const MAX_BYTES: u64 = 1 << 30;

#[derive(PartialEq, Eq)]
enum Outcome {
    Hint,
    Miss,
    Manual,
}

/// Исход кадра по его JSON (`shotOutcome`).
fn outcome(info: &Value) -> Outcome {
    if info["kind"] == "manual" {
        return Outcome::Manual;
    }
    let hints = info["hints"].as_array().cloned().unwrap_or_default();
    if hints.iter().any(|h| h["type"] == "hint" && h["text"].as_str().is_some_and(|t| t.contains("не узнал"))) {
        return Outcome::Miss;
    }
    let has = |t: &str| hints.iter().any(|h| h["type"] == t);
    if has("hint") && !has("error") {
        Outcome::Hint
    } else {
        Outcome::Miss
    }
}

/// Убрать лишнее из папки кадров (`planPrune`).
pub fn prune(root: &Path) {
    let now = SystemTime::now();
    let mut kept: Vec<(SystemTime, PathBuf, u64)> = vec![];
    let mut dropped = 0usize;
    let Ok(days) = std::fs::read_dir(root) else { return };
    for day in days.flatten().filter(|d| d.path().is_dir()) {
        let mut bases: Vec<PathBuf> = std::fs::read_dir(day.path())
            .map(|rd| {
                rd.flatten()
                    .map(|e| e.path())
                    .filter(|p| p.extension().is_some_and(|x| x == "png" || x == "json"))
                    .map(|p| p.with_extension(""))
                    .collect()
            })
            .unwrap_or_default();
        bases.sort();
        bases.dedup();
        for base in bases {
            let (png, js) = (base.with_extension("png"), base.with_extension("json"));
            let pm = std::fs::metadata(&png).ok();
            let jm = std::fs::metadata(&js).ok();
            let at = jm.as_ref().or(pm.as_ref()).and_then(|m| m.modified().ok()).unwrap_or(now);
            let age = now.duration_since(at).unwrap_or_default();
            let oc = std::fs::read_to_string(&js).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).map(|v| outcome(&v)).unwrap_or(Outcome::Miss);
            if age > JSON_AGE {
                let _ = std::fs::remove_file(&png);
                let _ = std::fs::remove_file(&js);
                dropped += 1;
                continue;
            }
            if let Some(pm) = pm {
                let limit = if oc == Outcome::Hint { HINT_PNG } else { MISS_PNG };
                if age > limit {
                    let _ = std::fs::remove_file(&png);
                    dropped += 1;
                } else {
                    kept.push((at, png, pm.len()));
                }
            }
        }
        let _ = std::fs::remove_dir(day.path());
    }
    // Потолок: самые старые картинки первыми.
    let mut total: u64 = kept.iter().map(|k| k.2).sum();
    kept.sort_by_key(|k| k.0);
    for (_, png, size) in kept {
        if total <= MAX_BYTES {
            break;
        }
        let _ = std::fs::remove_file(&png);
        total -= size;
        dropped += 1;
    }
    if dropped > 0 {
        eprintln!("[тюрьма] очистка кадров: удалено {dropped}");
    }
    let _ = HOUR;
}
