//! `gorn-cli replay`: свой путь подсказок на сохранённых кадрах тюрьмы
//! против прежнего. Перенос `src/cli/prison-replay.ts`: кадры дня Горна
//! (PNG и JSON с видом кадра, строками лога и прежними подсказками) идут по
//! порядку через советник, как в потоке подсказок.
//!
//! С `--rounds R` решатель считает без срока (до остановки или R раундов) —
//! ход в ход как `replay` в tools/reference.mts; так сверяется весь путь.

use std::path::{Path, PathBuf};
use std::time::Instant as Clock;

use gorn_prison::adapter::Cause;
use gorn_prison::advisor::{last_hint, move_key, OwnAdvisor, Reply, Shot, ShotKind};
use gorn_prison::hint::{hint, HintContext};
use gorn_prison::recognize::{read, Geometry, ReadOptions};
use gorn_prison::solver::{StopRule, STOP_DEFAULTS};
use gorn_prison::view::{Decision, ScreenView};
use serde_json::{json, Value};

use crate::frames::decode_png;

/// Последняя подсказка из сохранённого JSON кадра.
fn saved_last_hint(hints: &Value) -> String {
    let mut text = String::new();
    for h in hints.as_array().into_iter().flatten() {
        if h["type"] == "hint" {
            text = h["text"].as_str().unwrap_or("").to_string();
        }
    }
    text
}

/// Первое совпадение `check cur=(\d+) max=(\d+)` в строке (nonzero — max как `[1-9]\d*`).
fn check_day(l: &str, nonzero: bool) -> Option<(i32, i32)> {
    let digits = |s: &str| s.bytes().take_while(u8::is_ascii_digit).count();
    for (i, _) in l.match_indices("check cur=") {
        let rest = &l[i + 10..];
        let n1 = digits(rest);
        if n1 == 0 {
            continue;
        }
        let Some(r2) = rest[n1..].strip_prefix(" max=") else { continue };
        let n2 = digits(r2);
        if n2 == 0 || (nonzero && r2.starts_with('0')) {
            continue;
        }
        return Some((rest[..n1].parse().unwrap_or(i32::MAX), r2[..n2].parse().unwrap_or(i32::MAX)));
    }
    None
}

/// `/Btn_S16Board_(?:RouteCell|ActionCell|Cell)/`
fn board_click(l: &str) -> bool {
    ["Btn_S16Board_RouteCell", "Btn_S16Board_ActionCell", "Btn_S16Board_Cell"].iter().any(|b| {
        l.match_indices(b).any(|(i, _)| l[i + b.len()..].bytes().next().is_none_or(|c| !(c.is_ascii_alphanumeric() || c == b'_')))
    })
}

/// Один кадр повтора: что ответил советник.
pub struct FrameOut<'a> {
    pub file: String,
    pub saved: Option<&'a Value>,
    pub png: Option<PathBuf>,
    pub replies: Vec<Reply>,
    pub ms_total: f64,
}

/// Пройти кадры папки по порядку. stop/quick — правило решателя.
pub fn run(dir: &Path, from: &str, to: &str, budget: f64, stop: StopRule, quick: f64, adv: &mut OwnAdvisor, mut on: impl FnMut(&mut OwnAdvisor, FrameOut)) -> Result<(), String> {
    let mut files: Vec<String> = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|f| f.ends_with(".json") && f.len() >= 6 && &f[..6] >= from && &f[..6] <= to)
        .collect();
    files.sort();
    adv.stop = stop;
    adv.quick_ms = quick;
    let mut id = 0u64;
    let mut prev_kind = String::new();
    let mut log_day: Option<(i32, i32)> = None;
    let mut day_fresh = false;
    let mut morning_done = -1;
    for f in &files {
        let text = std::fs::read_to_string(dir.join(f)).map_err(|e| format!("{f}: {e}"))?;
        let saved: Value = serde_json::from_str(&text).map_err(|e| format!("{f}: {e}"))?;
        let kind = saved["kind"].as_str().unwrap_or("").to_string();
        let lines: Vec<String> =
            saved["lines"].as_array().into_iter().flatten().filter_map(|l| l.as_str().map(String::from)).collect();
        let tc = &saved["treasureClosed"];
        // Окно подарков без снимка раньше не сохранялось: доска за ним после начала дня — утренний подарок.
        if kind == "board" && tc.as_bool() == Some(true) && prev_kind != "treasure" && day_fresh {
            if let Some(ld) = log_day.filter(|ld| morning_done != ld.0) {
                morning_done = ld.0;
                id += 1;
                adv.replies.clear();
                let shot = Shot {
                    id,
                    kind: ShotKind::Treasure,
                    fresh: false,
                    after: Cause::Morning,
                    treasure_view: false,
                    rerolled: false,
                    treasure_closed: false,
                    theme: None,
                    day: Some(ld),
                    as_treasure: false,
                    width: 1920,
                    height: 1080,
                    budget_ms: budget,
                    settled: None,
                    events: vec![],
                };
                let t0 = Clock::now();
                adv.handle(&shot, None);
                let replies = std::mem::take(&mut adv.replies);
                let ms = t0.elapsed().as_secs_f64() * 1000.0;
                on(adv, FrameOut { file: format!("{f}#утро"), saved: None, png: None, replies, ms_total: ms });
            }
        }
        for l in &lines {
            if check_day(l, true).is_some_and(|d| d.0 > 0) {
                day_fresh = true;
            }
            if board_click(l) {
                day_fresh = false;
            }
        }
        let png = dir.join(f.replace(".json", ".png"));
        let img = if png.exists() { Some(decode_png(&png)?) } else { None };
        for l in &lines {
            if let Some(d) = check_day(l, false).filter(|d| d.1 > 0) {
                log_day = Some(d);
            }
        }
        let day = match saved["day"].as_object() {
            Some(d) => Some((d["cur"].as_i64().unwrap_or(0) as i32, d["max"].as_i64().unwrap_or(0) as i32)),
            None => log_day,
        };
        let shot = Shot {
            id: {
                id += 1;
                id
            },
            kind: ShotKind::parse(&kind).unwrap_or(ShotKind::Manual),
            fresh: saved["fresh"].as_bool().unwrap_or(false),
            after: Cause::parse(saved["after"].as_str().unwrap_or("")),
            treasure_view: saved["treasureView"].as_bool().unwrap_or(false),
            rerolled: saved["rerolled"].as_bool().unwrap_or(false),
            treasure_closed: match tc.as_bool() {
                Some(b) => b,
                None => kind == "board" && prev_kind == "treasure",
            },
            as_treasure: match saved["asTreasure"].as_bool() {
                Some(b) => b,
                None => kind == "board" && move_key(&saved_last_hint(&saved["hints"])) == "показать доску",
            },
            theme: saved["theme"].as_i64().map(|t| t as i32),
            day,
            width: img.as_ref().map(|i| i.width).unwrap_or(1920),
            height: img.as_ref().map(|i| i.height).unwrap_or(1080),
            budget_ms: budget,
            settled: saved["settled"].as_bool(),
            events: gorn_prison::logevents::compact(&lines),
        };
        prev_kind = kind;
        adv.replies.clear();
        let t0 = Clock::now();
        let frame = img.as_ref().map(|i| i.frame());
        adv.handle(&shot, frame.as_ref());
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        let replies = std::mem::take(&mut adv.replies);
        on(adv, FrameOut { file: f.clone(), saved: Some(&saved), png: img.is_some().then_some(png), replies, ms_total: ms });
    }
    Ok(())
}

/// Строка сверки, как `replay` в reference.mts.
pub fn frame_json(adv: &OwnAdvisor, o: &FrameOut) -> Value {
    json!({
        "file": o.file,
        "replies": o.replies.iter().map(Reply::to_json).collect::<Vec<_>>(),
        "memory": adv.adapter.mem.to_json(),
        "options": adv.last_options.iter().map(|(c, m)| json!([c, m])).collect::<Vec<_>>(),
    })
}

fn q(xs: &[f64], p: f64) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    let mut s = xs.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    s[((p * s.len() as f64) as usize).min(s.len() - 1)]
}

/// `gorn-cli replay`: отчёт, как prison-replay.ts.
pub fn report(dir: &Path, from: &str, to: &str, budget: f64, rounds: Option<u64>, diffs: Option<&str>, json_out: bool) -> Result<(), String> {
    let mut adv = OwnAdvisor::new();
    let (stop, quick) = match rounds {
        Some(r) => (StopRule { max_rounds: r, ..STOP_DEFAULTS }, f64::INFINITY),
        None => (STOP_DEFAULTS, gorn_prison::advisor::QUICK_MS),
    };
    struct Row {
        file: String,
        screen: String,
        old: String,
        mine: String,
        ms: f64,
        read_ms: f64,
        gap: Option<f64>,
    }
    let mut rows: Vec<Row> = vec![];
    let mut geo: Option<Geometry> = None;
    run(dir, from, to, budget, stop, quick, &mut adv, |adv, o| {
        if json_out {
            println!("{}", frame_json(adv, &o));
            if std::env::var("GORN_DEBUG_STATE").is_ok_and(|f| o.file.starts_with(&f)) {
                if let Some((s, seed, stop)) = &adv.last_search {
                    eprintln!("{} seed {seed} min {} max {} state {:?}", o.file, stop.min_rounds, stop.max_rounds, &s.m[..]);
                }
            }
            return;
        }
        let Some(saved) = o.saved else { return };
        // Вид экрана — для отчёта (отдельный проход распознавания).
        let (screen, read_ms) = match &o.png {
            Some(p) => {
                let img = decode_png(p).unwrap();
                let t = Clock::now();
                let (v, g) = read(&img.frame(), geo.as_ref(), ReadOptions::default());
                let ms = t.elapsed().as_secs_f64() * 1000.0;
                geo = g.or(geo);
                let s = match v {
                    ScreenView::Board(b) => {
                        if b.chain.is_some() {
                            "chain".to_string()
                        } else if let Some(d) = &b.decision {
                            match d {
                                Decision::Manage { .. } => "manage",
                                Decision::Office => "office",
                                Decision::Ratway => "ratway",
                                Decision::Event => "event",
                            }
                            .to_string()
                        } else if !b.doors.is_empty() {
                            "patrol".to_string()
                        } else {
                            "board".to_string()
                        }
                    }
                    ScreenView::Pick(_) => "pick".into(),
                    ScreenView::Treasure(_) => "treasure".into(),
                    ScreenView::Other => "other".into(),
                };
                (s, ms)
            }
            None => (if saved["kind"] == "treasure" { "treasure".into() } else { "без PNG".into() }, 0.0),
        };
        let old = saved_last_hint(&saved["hints"]);
        let mine = last_hint(&o.replies);
        let old_key = move_key(&old);
        let mut gap = None;
        if !old_key.is_empty() && adv.last_options.len() > 1 {
            let best = adv.last_options.iter().map(|o| o.1).fold(f64::NEG_INFINITY, f64::max);
            let ctx = HintContext::default();
            if let Some(t) = adv.last_options.iter().find(|(c, _)| move_key(&hint(*c, &ctx).text) == old_key) {
                gap = Some(best - t.1);
            }
        }
        rows.push(Row { file: o.file.clone(), screen, old, mine, ms: o.ms_total, read_ms, gap });
    })?;
    if json_out {
        return Ok(());
    }
    let skipped = rows.iter().filter(|r| r.screen == "без PNG").count();
    rows.retain(|r| r.screen != "без PNG");
    let keyed: Vec<(String, String)> = rows.iter().map(|r| (move_key(&r.old), move_key(&r.mine))).collect();
    let both: Vec<usize> = (0..rows.len()).filter(|&i| !keyed[i].0.is_empty() && !keyed[i].1.is_empty()).collect();
    let same = both.iter().filter(|&&i| keyed[i].0 == keyed[i].1).count();
    println!("кадров: {} ({}); не переиграть: {skipped} (нет PNG)", rows.len() + skipped, dir.display());
    println!(
        "оба дали ход: {}, совпало: {same} ({:.1} %)",
        both.len(),
        100.0 * same as f64 / both.len().max(1) as f64
    );
    println!(
        "ход только у прежнего: {}, только у своего: {}",
        keyed.iter().filter(|k| !k.0.is_empty() && k.1.is_empty()).count(),
        keyed.iter().filter(|k| k.0.is_empty() && !k.1.is_empty()).count()
    );
    let mut screens: Vec<(String, usize, usize)> = vec![];
    for &i in &both {
        let s = &rows[i].screen;
        let e = match screens.iter_mut().find(|x| &x.0 == s) {
            Some(e) => e,
            None => {
                screens.push((s.clone(), 0, 0));
                screens.last_mut().unwrap()
            }
        };
        e.1 += 1;
        if keyed[i].0 == keyed[i].1 {
            e.2 += 1;
        }
    }
    screens.sort_by(|a, b| b.1.cmp(&a.1));
    println!("по экранам (совпало / оба дали ход):");
    for (s, n, k) in &screens {
        println!("  {s}: {k}/{n} ({:.0} %)", 100.0 * *k as f64 / *n as f64);
    }
    let gaps: Vec<f64> = both.iter().filter(|&&i| keyed[i].0 != keyed[i].1).filter_map(|&i| rows[i].gap).collect();
    if !gaps.is_empty() {
        let small = gaps.iter().filter(|&&g| g < 0.1).count();
        println!(
            "расхождения, оценённые своим решателем: {}; ход прежнего хуже меньше чем на 0,1 призм.: {small} ({:.0} %), медиана разницы {:.2}, p90 {:.2}",
            gaps.len(),
            100.0 * small as f64 / gaps.len() as f64,
            q(&gaps, 0.5),
            q(&gaps, 0.9)
        );
    }
    let read: Vec<f64> = rows.iter().filter(|r| r.read_ms > 0.0).map(|r| r.read_ms).collect();
    println!("распознавание: медиана {:.1} мс, p95 {:.1} мс", q(&read, 0.5), q(&read, 0.95));
    let ms: Vec<f64> = rows.iter().map(|r| r.ms).collect();
    println!("кадр целиком (распознавание + совет): медиана {:.0} мс, p95 {:.0} мс", q(&ms, 0.5), q(&ms, 0.95));
    let dif: Vec<usize> = (0..rows.len()).filter(|&i| keyed[i].0 != keyed[i].1 && (!keyed[i].0.is_empty() || !keyed[i].1.is_empty())).collect();
    match diffs {
        Some(path) => {
            let mut out = vec![
                "# Расхождения своего пути с прежним".to_string(),
                String::new(),
                format!("Папка: {}", dir.display()),
                String::new(),
                "| Кадр | Экран | Прежний | Свой | Разница |".into(),
                "|---|---|---|---|---|".into(),
            ];
            for &i in &dif {
                let r = &rows[i];
                out.push(format!(
                    "| {} | {} | {} | {} | {} |",
                    dir.join(r.file.replace(".json", ".png")).display(),
                    r.screen,
                    if r.old.is_empty() { "—" } else { &r.old },
                    if r.mine.is_empty() { "—" } else { &r.mine },
                    r.gap.map(|g| format!("{g:.2}")).unwrap_or_default()
                ));
            }
            std::fs::write(path, out.join("\n") + "\n").map_err(|e| e.to_string())?;
            println!("расхождений: {}, список — {path}", dif.len());
        }
        None => println!("расхождений: {} (список — ключом --diffs файл.md)", dif.len()),
    }
    Ok(())
}
