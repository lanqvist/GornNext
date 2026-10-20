//! `gorn-cli facts <папка> [--db файл] [--rounds R]`: журнал фактов тюрьмы
//! (этап 7) по сохранённым кадрам — советник проходит кадры, как в повторе
//! (`replay`), и каждое его решение пишется в базу фактов. Так журнал сразу
//! получает историю всех забегов, у которых сохранены кадры.
//!
//! Папка — день Горна (кадры `ЧЧММСС-…`) или папка дней (`ГГГГ-ММ-ДД`).
//! Время факта — из имени дня и кадра (местное), по нему же делятся забеги.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use chrono::TimeZone;
use gorn_core::facts::Journal;
use gorn_prison::advisor::{OwnAdvisor, QUICK_MS};
use gorn_prison::solver::{StopRule, STOP_DEFAULTS};
use serde_json::Value;

/// Время кадра: день из имени папки, часы-минуты-секунды из имени файла.
fn frame_ts(day: &str, file: &str) -> Option<i64> {
    let d = chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d").ok()?;
    let hms = file.get(..6)?;
    let t = chrono::NaiveTime::parse_from_str(hms, "%H%M%S").ok()?;
    chrono::Local.from_local_datetime(&d.and_time(t)).earliest().map(|x| x.timestamp_millis())
}

fn days_of(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut days: Vec<PathBuf> = std::fs::read_dir(root)
        .map_err(|e| format!("{}: {e}", root.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.file_name().and_then(|n| n.to_str()).is_some_and(|n| chrono::NaiveDate::parse_from_str(n, "%Y-%m-%d").is_ok()))
        .collect();
    days.sort();
    if days.is_empty() {
        days.push(root.to_path_buf());
    }
    Ok(days)
}

pub fn run(root: &Path, db: &Path, rounds: Option<u64>, budget: f64) -> Result<(), String> {
    let journal = Journal::open(db, &format!("replay {}", env!("CARGO_PKG_VERSION"))).map_err(|e| e.to_string())?;
    let (stop, quick) = match rounds {
        Some(r) => (StopRule { max_rounds: r, ..STOP_DEFAULTS }, f64::INFINITY),
        None => (STOP_DEFAULTS, QUICK_MS),
    };
    let (mut total, mut frames) = (0usize, 0usize);
    let mut runs = std::collections::BTreeSet::new();
    for day in days_of(root)? {
        let name = day.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
        // Память забега — с нуля на каждый день, как у повтора.
        let mut adv = OwnAdvisor::new();
        let got: Arc<Mutex<Vec<Value>>> = Arc::default();
        let sink = got.clone();
        adv.facts = Some(Box::new(move |f| sink.lock().unwrap().push(f)));
        let mut err = None;
        crate::replay::run(&day, "000000", "999999", budget, stop, quick, &mut adv, |_, o| {
            frames += 1;
            let ts = frame_ts(&name, &o.file).unwrap_or(0);
            for f in got.lock().unwrap().drain(..) {
                match journal.add(&f, ts) {
                    Ok(run) => {
                        runs.insert(run);
                        total += 1;
                    }
                    Err(e) => err = Some(e.to_string()),
                }
            }
        })?;
        if let Some(e) = err {
            return Err(format!("{name}: {e}"));
        }
        println!("{name}: кадров {frames}, фактов {total}, забегов {}", runs.len());
    }
    println!("журнал: {} — фактов {total}, забегов {}", db.display(), runs.len());
    Ok(())
}
