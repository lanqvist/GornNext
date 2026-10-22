//! `gorn-cli audit`: аудит забегов тюрьмы по кадрам Горна. Перенос
//! `src/cli/prison-audit.ts` и `prison-audit-core.ts`.
//!
//! Берёт папки кадров за последние --days дней («Изображения/Горн — тюрьма/
//! <дата>»), делит их на забеги и каждый завершённый забег прогоняет заново
//! через свой путь подсказок. Находки: подсказка молчала, карта или иконка
//! не узнана, ожидание скачет без хода, хранилище откатилось, день разошёлся
//! с логом, живой совет заметно хуже нынешнего, совет ходит по кругу
//! («петля»), обрыв связи. Кадры забега копируются в папку данных, отчёт —
//! «gorn-prison-data/audits/<дата>/<ЧЧММСС>.md», разобранные — в audits/index.json.

use std::path::{Path, PathBuf};

use gorn_prison::adapter::Cause;
use gorn_prison::advisor::{last_hint, move_key, OwnAdvisor, Shot, ShotKind};
use gorn_prison::hint::{hint, HintContext};
use gorn_prison::model::{START_EVENT_IDS, TOOL_IDS};
use gorn_prison::recognize::{read, Geometry, ReadOptions};
use gorn_prison::view::{Decision, ScreenView};
use serde_json::{json, Value};

use crate::frames::{data_dir, decode_png};

// ── ядро: чистые функции ────────────────────────────────────────────────────

/// Кадр для разбивки на забеги.
#[allow(dead_code)]
pub struct FrameHead {
    pub file: String,
    pub kind: String,
    pub fresh: bool,
}

/// Забеги по кадрам одного дня (from, to, start). Забег начинается с выбора
/// орудий в начале забега (fresh) вместе с окнами цели и темы прямо перед ним.
/// Кадры до первого начала — хвост прошлого забега (start = false).
pub fn split_runs(frames: &[FrameHead]) -> Vec<(usize, usize, bool)> {
    let mut starts: Vec<usize> = vec![];
    for i in 0..frames.len() {
        if frames[i].kind != "instruments" || !frames[i].fresh {
            continue;
        }
        let mut s = i;
        while s > 0 && (frames[s - 1].kind == "objective" || frames[s - 1].kind == "theme") {
            s -= 1;
        }
        // Повторный кадр того же выбора (после «Обновить») — тот же забег.
        if let Some(&last) = starts.last() {
            if i - last <= 3 && frames[last..i].iter().all(|f| f.kind != "board") {
                continue;
            }
        }
        starts.push(s);
    }
    let mut runs = vec![];
    if !frames.is_empty() && (starts.is_empty() || starts[0] > 0) {
        runs.push((0, starts.first().copied().unwrap_or(frames.len()) - 1, false));
    }
    for (k, &s) in starts.iter().enumerate() {
        runs.push((s, starts.get(k + 1).copied().unwrap_or(frames.len()) - 1, true));
    }
    runs
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct Mem {
    pub day: i32,
    pub day_limit: i32,
    pub storage: i32,
    pub tools: Vec<String>,
    pub assumed: Vec<String>,
    pub step: String,
    pub start: String,
}

/// Запись повтора по одному кадру.
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct FrameRec {
    pub file: String,
    /// ЧЧММСС из имени файла.
    pub time: String,
    pub kind: String,
    pub has_png: bool,
    /// День по логу (cur с единицы), если строка check cur была.
    pub log_day: Option<(i32, i32)>,
    /// Последняя живая подсказка.
    pub live: String,
    /// Горн ответил на кадр.
    pub answered: bool,
    /// Что на кадре по распознаванию.
    pub screen: String,
    /// На кадре есть ход.
    pub actionable: bool,
    /// Неузнанных карт на выборе орудий.
    pub pick_unknown: usize,
    /// Совет повтора и лучшее ожидание.
    pub mine: String,
    pub best: Option<f64>,
    /// На сколько живой ход хуже лучшего по повтору.
    pub live_gap: Option<f64>,
    /// В логе кадра подтверждено событие.
    pub event: bool,
    pub mem: Mem,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Finding {
    pub kind: &'static str,
    pub time: String,
    pub text: String,
    /// Та же находка на соседних кадрах подряд.
    pub count: Option<u32>,
}

/// Пороги находок.
const JUMP_SAME: f64 = 1.0;
const JUMP_ANY: f64 = 1.5;
const GAP: f64 = 0.15;
const LOOP: u32 = 3;
const ANSWER_SOON_S: i64 = 3;

fn hhmmss(t: &str) -> String {
    format!("{}:{}:{}", &t[0..2], &t[2..4], &t[4..6])
}

fn seconds(t: &str) -> i64 {
    let n = |a: usize, b: usize| t[a..b].parse::<i64>().unwrap_or(0);
    (n(0, 2) * 60 + n(2, 4)) * 60 + n(4, 6)
}

/// Подтверждение окна события в логе игры.
fn is_event_line(l: &str) -> bool {
    word_at_end(l, "Btn_S16Board_EventPop_Confirm")
}

fn word_at_end(l: &str, w: &str) -> bool {
    l.match_indices(w).any(|(i, _)| l[i + w.len()..].bytes().next().is_none_or(|c| !(c.is_ascii_alphanumeric() || c == b'_')))
}

/// Строки лога игры об обрыве связи.
fn is_net_line(l: &str) -> bool {
    word_at_end(l, "NetGame DisConnect")
        || l.contains("ShowReconnect")
        || l.contains("Reconnect Fail")
        || l.contains("BackToLogin()")
        || l.contains("MessageATimeout")
}

fn fmt2(x: f64) -> String {
    format!("{x:.2}")
}

/// Находки по записям одного забега.
pub fn findings(recs: &[FrameRec]) -> Vec<Finding> {
    let mut out: Vec<Finding> = vec![];
    let add = |out: &mut Vec<Finding>, kind: &'static str, r: &FrameRec, text: String| {
        if let Some(last) = out.last_mut() {
            if last.kind == kind && last.text == text {
                last.count = Some(last.count.unwrap_or(1) + 1);
                return;
            }
        }
        out.push(Finding { kind, time: hhmmss(&r.time), text, count: None });
    };
    let no_jump = |s: &str| matches!(s, "pick" | "treasure" | "board");
    let mut max_storage = -1;
    let mut prev: Option<&FrameRec> = None;
    let mut event = false;
    for (i, r) in recs.iter().enumerate() {
        event |= r.event;
        let next = recs.get(i + 1);
        let soon = next.is_some_and(|n| n.screen == r.screen && !n.live.is_empty() && seconds(&n.time) - seconds(&r.time) <= ANSWER_SOON_S);
        if r.has_png && r.actionable && r.live.is_empty() && r.answered && !soon && r.kind != "manual" {
            let tail = if r.mine.is_empty() { "; Горн молчит и сейчас".to_string() } else { format!("; сейчас Горн советует «{}»", r.mine) };
            add(&mut out, "зависание", r, format!("подсказки не было на экране «{}»{tail}", r.screen));
        }
        if r.pick_unknown > 0 {
            add(&mut out, "карта", r, format!("на выборе орудий не узнано карт: {}", r.pick_unknown));
        }
        if let Some(g) = r.live_gap.filter(|&g| g >= GAP) {
            add(&mut out, "расхождение", r, format!("живой совет «{}» хуже нынешнего «{}» на {} призм.", r.live, r.mine, fmt2(g)));
        }
        if r.screen != "other" && r.screen != "pick" {
            if r.mem.storage < max_storage {
                add(&mut out, "хранилище", r, format!("хранилище в памяти {}, а было {max_storage}", r.mem.storage));
            }
            max_storage = max_storage.max(r.mem.storage);
        }
        if let Some((cur, _)) = r.log_day {
            if cur > 0 && r.mem.day + 1 != cur && r.kind == "board" {
                add(&mut out, "день", r, format!("день в памяти {}, по логу {cur}", r.mem.day + 1));
            }
        }
        if let (Some(p), Some(best)) = (prev, r.best) {
            if let Some(pb) = p.best {
                if p.mem.day == r.mem.day && !event && !no_jump(&r.screen) && !no_jump(&p.screen) {
                    let d = best - pb;
                    let same = p.screen == r.screen && p.mine == r.mine;
                    if d.abs() >= if same { JUMP_SAME } else { JUMP_ANY } {
                        add(
                            &mut out,
                            "скачок",
                            r,
                            format!(
                                "ожидание {} → {} ({} «{}» → «{}»){}",
                                fmt2(pb),
                                fmt2(best),
                                hhmmss(&p.time),
                                p.screen,
                                r.screen,
                                if same { " без хода" } else { "" }
                            ),
                        );
                    }
                }
            }
        }
        if r.best.is_some() {
            prev = Some(r);
            event = false;
        }
    }
    out.extend(loops(recs));
    if let Some(last) = recs.iter().rev().find(|r| !r.mem.tools.is_empty() || !r.mem.assumed.is_empty()) {
        if !last.mem.assumed.is_empty() {
            out.push(Finding {
                kind: "иконка",
                time: hhmmss(&last.time),
                text: format!("иконка на панели не узнаётся, орудие только «предположительно»: {}", last.mem.assumed.join(", ")),
                count: None,
            });
        }
    }
    out
}

/// Вид хода в подсказке без цели: «Пожирание: съесть №12» → «Пожирание».
fn move_kind(live: &str) -> String {
    live.split(" · ").next().unwrap_or("").split(':').next().unwrap_or("").trim().to_string()
}

/// Петля советов: в пределах дня живой совет скачет между двумя ходами по
/// кругу — A → B → A → B … (19.10 23:30: «Пожирание: съесть №12» ↔
/// «Отменить, затем «Завершить день»» — игра такую цель не предлагала).
fn loops(recs: &[FrameRec]) -> Vec<Finding> {
    let mut out = vec![];
    let mut seq: Vec<(&FrameRec, String)> = vec![];
    let mut day: Option<i32> = None;
    let mut alt: u32 = 0;
    let mut reported = false;
    for r in recs {
        if r.live.is_empty() {
            continue;
        }
        let d = r.log_day.map(|d| d.0).unwrap_or(r.mem.day + 1);
        if Some(d) != day {
            day = Some(d);
            seq.clear();
            alt = 0;
            reported = false;
        }
        let key = move_kind(&r.live);
        if seq.last().is_some_and(|s| s.1 == key) {
            continue; // тот же совет на соседних кадрах
        }
        seq.push((r, key));
        let n = seq.len();
        alt = if n >= 3 && seq[n - 1].1 == seq[n - 3].1 { alt + 1 } else { 0 };
        if alt >= LOOP && !reported {
            reported = true;
            let (a, b) = (seq[n - 1].0, seq[n - 2].0);
            out.push(Finding {
                kind: "петля",
                time: hhmmss(&seq[n - 1 - alt as usize - 1].0.time),
                text: format!(
                    "совет ходит по кругу в день {d}: «{}» ↔ «{}» ({} раз подряд, до {}) — модель предлагает ход, которого игра не даёт",
                    a.live,
                    b.live,
                    alt + 1,
                    hhmmss(&a.time)
                ),
                count: None,
            });
        }
    }
    out
}

pub struct Summary {
    pub start: Option<f64>,
    pub prisms: i32,
    pub days: String,
    pub tools: Vec<String>,
    pub theme: String,
}

/// Сводка забега.
pub fn summary(recs: &[FrameRec]) -> Summary {
    let first_patrol = recs.iter().find(|r| r.screen == "patrol" && r.best.is_some());
    let prisms = recs.iter().map(|r| r.mem.storage).fold(0, i32::max);
    let last = recs.last();
    let last_log = recs.iter().rev().find(|r| r.log_day.is_some_and(|d| d.0 > 0)).and_then(|r| r.log_day);
    Summary {
        start: first_patrol.and_then(|r| r.best),
        prisms,
        days: match (last_log, last) {
            (Some((c, m)), _) => format!("{c}/{m}"),
            (None, Some(l)) => format!("{}/{}", l.mem.day + 1, l.mem.day_limit),
            _ => "—".into(),
        },
        tools: last.map(|l| l.mem.tools.clone()).unwrap_or_default(),
        theme: last.map(|l| l.mem.start.clone()).unwrap_or_else(|| "none".into()),
    }
}

// ── прогон забега ───────────────────────────────────────────────────────────

fn check_day(l: &str) -> Option<(i32, i32)> {
    let digits = |s: &str| s.bytes().take_while(u8::is_ascii_digit).count();
    for (i, _) in l.match_indices("check cur=") {
        let rest = &l[i + 10..];
        let n1 = digits(rest);
        if n1 == 0 {
            continue;
        }
        let Some(r2) = rest[n1..].strip_prefix(" max=") else { continue };
        let n2 = digits(r2);
        if n2 == 0 {
            continue;
        }
        return Some((rest[..n1].parse().unwrap_or(0), r2[..n2].parse().unwrap_or(0)));
    }
    None
}

fn saved_hint(hints: &Value) -> String {
    let mut text = String::new();
    for h in hints.as_array().into_iter().flatten() {
        if h["type"] == "hint" {
            text = h["text"].as_str().unwrap_or("").to_string();
        }
    }
    text
}

fn screen_name(v: &ScreenView) -> String {
    match v {
        ScreenView::Board(b) => {
            if b.chain.is_some() {
                "chain".into()
            } else if let Some(d) = &b.decision {
                match d {
                    Decision::Manage { .. } => "manage",
                    Decision::Office => "office",
                    Decision::Ratway => "ratway",
                    Decision::Event => "event",
                }
                .into()
            } else if !b.doors.is_empty() {
                "patrol".into()
            } else {
                "board".into()
            }
        }
        ScreenView::Pick(_) => "pick".into(),
        ScreenView::Treasure(_) => "treasure".into(),
        ScreenView::Other => "other".into(),
    }
}

/// Прогнать кадры забега через свой путь подсказок.
pub fn replay(dir: &Path, data: &Path, files: &[String], budget: f64, rounds: Option<u64>) -> Result<(Vec<FrameRec>, Vec<Value>), String> {
    let mut adv = OwnAdvisor::new();
    if let Some(r) = rounds {
        // Без срока, до остановки или r раундов — детерминированно (для сверки).
        adv.stop = gorn_prison::solver::StopRule { max_rounds: r, ..gorn_prison::solver::STOP_DEFAULTS };
        adv.quick_ms = f64::INFINITY;
    }
    let mut geo: Option<Geometry> = None;
    let mut id = 0u64;
    let mut log_day: Option<(i32, i32)> = None;
    let mut recs = vec![];
    let mut saved_all = vec![];
    let day_name = dir.file_name().map(|d| d.to_string_lossy().to_string()).unwrap_or_default();
    for f in files {
        let s: Value = serde_json::from_str(&std::fs::read_to_string(dir.join(f)).map_err(|e| format!("{f}: {e}"))?)
            .map_err(|e| format!("{f}: {e}"))?;
        // Картинку Горн удаляет через 2 часа — тогда берём копию из папки данных.
        let name = f.replace(".json", ".png");
        let png = [dir.join(&name), data.join("runs").join(&day_name).join(&name)].into_iter().find(|p| p.exists());
        let img = match &png {
            Some(p) => Some(decode_png(p)?),
            None => None,
        };
        let kind = s["kind"].as_str().unwrap_or("").to_string();
        let lines: Vec<String> = s["lines"].as_array().into_iter().flatten().filter_map(|l| l.as_str().map(String::from)).collect();
        let mut screen = if kind == "treasure" { "treasure".to_string() } else { "other".to_string() };
        let mut actionable = false;
        let mut pick_unknown = 0;
        if let Some(img) = &img {
            let (v, g) = read(&img.frame(), geo.as_ref(), ReadOptions::default());
            geo = g.or(geo);
            screen = screen_name(&v);
            match &v {
                ScreenView::Board(_) => {
                    // «Выберите цель» на кадре с началом дня — вчерашняя надпись после «Выслеживания»: хода нет.
                    let day_start = lines
                        .iter()
                        .any(|l| l.match_indices("check cur=").any(|(i, _)| l[i + 10..].bytes().next().is_some_and(|c| c.is_ascii_digit())));
                    actionable = screen != "board" && !(screen == "ratway" && day_start);
                }
                ScreenView::Pick(p) => {
                    actionable = true;
                    pick_unknown = p.cards.iter().filter(|c| c.is_none()).count();
                }
                _ => {}
            }
        }
        for l in &lines {
            if let Some(d) = check_day(l).filter(|d| d.1 > 0) {
                log_day = Some(d);
            }
        }
        let day = match s["day"].as_object() {
            Some(d) => Some((d["cur"].as_i64().unwrap_or(0) as i32, d["max"].as_i64().unwrap_or(0) as i32)),
            None => log_day,
        };
        id += 1;
        let shot = Shot {
            id,
            kind: ShotKind::parse(&kind).unwrap_or(ShotKind::Manual),
            fresh: s["fresh"].as_bool().unwrap_or(false),
            after: Cause::parse(s["after"].as_str().unwrap_or("")),
            treasure_view: s["treasureView"].as_bool().unwrap_or(false),
            rerolled: s["rerolled"].as_bool().unwrap_or(false),
            treasure_closed: s["treasureClosed"].as_bool().unwrap_or(false),
            as_treasure: s["asTreasure"].as_bool().unwrap_or(false),
            theme: s["theme"].as_i64().map(|t| t as i32),
            day,
            width: img.as_ref().map(|i| i.width).unwrap_or(1920),
            height: img.as_ref().map(|i| i.height).unwrap_or(1080),
            budget_ms: budget,
            settled: s["settled"].as_bool(),
            events: gorn_prison::logevents::compact(&lines),
        };
        adv.replies.clear();
        let frame = img.as_ref().map(|i| i.frame());
        adv.handle(&shot, frame.as_ref());
        let mine = last_hint(&adv.replies);
        let live = saved_hint(&s["hints"]);
        let opts = &adv.last_options;
        // Один вариант решатель не считает (ожидание 0) — оценки нет.
        let best = (opts.len() > 1).then(|| opts.iter().map(|o| o.1).fold(f64::NEG_INFINITY, f64::max));
        let mut live_gap = None;
        if let Some(b) = best {
            if !live.is_empty() && move_key(&live) != move_key(&mine) {
                let ctx = HintContext::default();
                if let Some(t) = opts.iter().find(|(c, _)| move_key(&hint(*c, &ctx).text) == move_key(&live)) {
                    live_gap = Some(b - t.1);
                }
            }
        }
        let m = &adv.adapter.mem;
        recs.push(FrameRec {
            file: f.clone(),
            time: f[..6].to_string(),
            kind,
            has_png: img.is_some(),
            log_day: day,
            live,
            answered: s["hints"].as_array().is_some_and(|h| !h.is_empty()),
            screen,
            actionable,
            pick_unknown,
            mine,
            best,
            live_gap,
            event: lines.iter().any(|l| is_event_line(l)),
            mem: Mem {
                day: m.day,
                day_limit: m.day_limit,
                storage: m.storage,
                tools: m.tools.iter().map(|&t| TOOL_IDS[t as usize].to_string()).collect(),
                assumed: m.assumed.iter().map(|a| TOOL_IDS[a.0 as usize].to_string()).collect(),
                step: m.step.name().to_string(),
                start: START_EVENT_IDS[m.start as usize].to_string(),
            },
        });
        saved_all.push(s);
    }
    Ok((recs, saved_all))
}

/// Метка времени строки лога игры: мс от эпохи (UTC).
fn stamp_ms(l: &str) -> Option<i64> {
    // [ГГГГ.ММ.ДД-ЧЧ.ММ.СС:ммм]
    let b = l.as_bytes();
    if b.len() < 25 || b[0] != b'[' || b[24] != b']' {
        return None;
    }
    let num = |a: usize, n: usize| -> Option<i64> {
        let s = l.get(a..a + n)?;
        s.bytes().all(|c| c.is_ascii_digit()).then(|| s.parse().ok()).flatten()
    };
    let (y, mo, d) = (num(1, 4)?, num(6, 2)?, num(9, 2)?);
    let (h, mi, s, ms) = (num(12, 2)?, num(15, 2)?, num(18, 2)?, num(21, 3)?);
    if &l[5..6] != "." || &l[8..9] != "." || &l[11..12] != "-" || &l[14..15] != "." || &l[17..18] != "." || &l[20..21] != ":" {
        return None;
    }
    // дни от эпохи (алгоритм Хиннанта)
    let (yy, mm) = if mo <= 2 { (y - 1, mo + 9) } else { (y, mo - 3) };
    let era = yy.div_euclid(400);
    let yoe = yy - era * 400;
    let doy = (153 * mm + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some((((days * 24 + h) * 60 + mi) * 60 + s) * 1000 + ms)
}

/// Папка логов игры (Steam-библиотеки из libraryfolders.vdf).
fn find_log_directory() -> Option<PathBuf> {
    let pf86 = std::env::var("ProgramFiles(x86)").unwrap_or_else(|_| r"C:\Program Files (x86)".into());
    let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".into());
    let mut libs: Vec<PathBuf> = vec![];
    for root in [Path::new(&pf86).join("Steam"), Path::new(&pf).join("Steam")] {
        if !root.exists() {
            continue;
        }
        libs.push(root.clone());
        if let Ok(text) = std::fs::read_to_string(root.join("steamapps").join("libraryfolders.vdf")) {
            for line in text.lines() {
                let t = line.trim();
                if let Some(rest) = t.strip_prefix("\"path\"") {
                    let v = rest.trim();
                    if v.len() >= 2 && v.starts_with('"') && v.ends_with('"') {
                        libs.push(PathBuf::from(v[1..v.len() - 1].replace("\\\\", "\\")));
                    }
                }
            }
        }
    }
    libs.dedup();
    for lib in libs {
        for game in ["Torchlight Infinite", "Torchlight Infinite CN"] {
            let dir = lib.join("steamapps").join("common").join(game).join("UE_game").join("TorchLight").join("Saved").join("Logs");
            if dir.exists() {
                return Some(dir);
            }
        }
    }
    None
}

/// Обрывы связи в логе игры за время забега (по меткам строк кадров).
fn net_events(saved: &[Value], files: &[String], log: &[String]) -> Vec<Finding> {
    let lines_of = |s: &Value| -> Vec<String> { s["lines"].as_array().into_iter().flatten().filter_map(|l| l.as_str().map(String::from)).collect() };
    let stamps: Vec<i64> = saved.iter().flat_map(|s| lines_of(s).into_iter().filter_map(|l| stamp_ms(&l))).collect();
    if stamps.is_empty() {
        return vec![];
    }
    let lo = stamps.iter().min().unwrap() - 5_000;
    let hi = stamps.iter().max().unwrap() + 5_000;
    // Сдвиг лога (UTC) к местному времени кадров: по последнему кадру со строками.
    let mut offset = 0i64;
    for i in (0..saved.len()).rev() {
        let Some(st) = lines_of(&saved[i]).iter().filter_map(|l| stamp_ms(l)).last() else { continue };
        let t = &files[i];
        let local = ((t[0..2].parse::<i64>().unwrap_or(0) * 60 + t[2..4].parse::<i64>().unwrap_or(0)) * 60
            + t[4..6].parse::<i64>().unwrap_or(0))
            * 1000;
        let utc = st % 86_400_000;
        offset = ((local - utc) as f64 / 900_000.0).round() as i64 * 900_000;
        break;
    }
    let hits: Vec<i64> = log.iter().filter(|l| is_net_line(l)).filter_map(|l| stamp_ms(l)).filter(|&at| at >= lo && at <= hi).collect();
    if hits.is_empty() {
        return vec![];
    }
    let t = (hits[0] + offset).rem_euclid(86_400_000) / 1000;
    vec![Finding {
        kind: "связь",
        time: format!("{:02}:{:02}:{:02}", t / 3600, t / 60 % 60, t % 60),
        text: format!("обрыв связи с игрой (строк в логе: {}); проверь память после переподключения", hits.len()),
        count: None,
    }]
}

fn report_md(day: &str, start: &str, recs: &[FrameRec], found: &[Finding], files: &[String]) -> String {
    let s = summary(recs);
    let decided: Vec<&FrameRec> = recs.iter().filter(|r| !r.live.is_empty() && !r.mine.is_empty()).collect();
    let same = decided.iter().filter(|r| move_key(&r.live) == move_key(&r.mine)).count();
    let mut lines = vec![
        format!("# Забег {day} {}", hhmmss(start)),
        String::new(),
        format!("- Итог: {} призм. в хранилище, дней {}{}.", s.prisms, s.days, if s.theme != "none" { format!(", тема {}", s.theme) } else { String::new() }),
        format!("- Ожидание на первом обходе: {}.", s.start.map(fmt2).unwrap_or_else(|| "—".into())),
        format!("- Орудия: {}.", if s.tools.is_empty() { "—".to_string() } else { s.tools.join(", ") }),
        format!("- Живые подсказки совпали с повтором: {same} из {}.", decided.len()),
        format!("- Кадров: {} ({} … {}).", files.len(), files[0], files[files.len() - 1]),
        String::new(),
        format!("## Находки ({})", found.len()),
        String::new(),
    ];
    if found.is_empty() {
        lines.push("Нет.".into());
    }
    let mut kinds: Vec<&str> = vec![];
    for f in found {
        if !kinds.contains(&f.kind) {
            kinds.push(f.kind);
        }
    }
    for k in kinds {
        let list: Vec<&Finding> = found.iter().filter(|f| f.kind == k).collect();
        for f in list.iter().take(8) {
            lines.push(format!(
                "- {} · {k} · {}{}",
                f.time,
                f.text,
                f.count.map(|c| format!(" (кадров подряд: {c})")).unwrap_or_default()
            ));
        }
        if list.len() > 8 {
            lines.push(format!("- … ещё {k}: {}", list.len() - 8));
        }
    }
    lines.join("\n") + "\n"
}

pub struct AuditOpts {
    pub only_new: bool,
    pub days: usize,
    pub budget: f64,
    pub quiet_min: f64,
    pub only_run: Option<String>,
    /// Кадры Горна (по умолчанию «Изображения/Горн — тюрьма»).
    pub shots: PathBuf,
    /// Куда отчёты и index.json (по умолчанию gorn-prison-data/audits).
    pub out: PathBuf,
    /// Копировать кадры забега в папку данных.
    pub keep: bool,
}

pub fn default_shots() -> PathBuf {
    let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).unwrap_or_default();
    Path::new(&home).join("Pictures").join("Горн — тюрьма")
}

pub fn run(o: &AuditOpts) -> Result<(), String> {
    let data = data_dir();
    std::fs::create_dir_all(&o.out).map_err(|e| e.to_string())?;
    let index_path = o.out.join("index.json");
    let mut index: serde_json::Map<String, Value> = std::fs::read_to_string(&index_path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let mut log: Option<Vec<String>> = None;
    let mut day_dirs: Vec<String> = std::fs::read_dir(&o.shots)
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|d| d.len() == 10 && d.as_bytes()[4] == b'-' && d.as_bytes()[7] == b'-' && d.chars().filter(|c| c.is_ascii_digit()).count() == 8)
                .collect()
        })
        .unwrap_or_default();
    day_dirs.sort();
    let skip = day_dirs.len().saturating_sub(o.days);
    let mut done = 0;
    for day in &day_dirs[skip..] {
        let dir = o.shots.join(day);
        let mut files: Vec<String> = std::fs::read_dir(&dir)
            .map_err(|e| e.to_string())?
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|f| f.ends_with(".json"))
            .collect();
        files.sort();
        let heads: Vec<FrameHead> = files
            .iter()
            .map(|f| {
                let s: Value = std::fs::read_to_string(dir.join(f)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
                FrameHead { file: f.clone(), kind: s["kind"].as_str().unwrap_or("").into(), fresh: s["fresh"].as_bool().unwrap_or(false) }
            })
            .collect();
        let runs = split_runs(&heads);
        for (k, &(from, to, start)) in runs.iter().enumerate() {
            if !start {
                continue;
            }
            let run_files = &files[from..=to];
            let st = run_files[0][..6].to_string();
            let run_id = format!("{day}/{st}");
            if o.only_run.as_deref().is_some_and(|r| r != st) {
                continue;
            }
            if o.only_new && index.contains_key(&run_id) {
                continue;
            }
            let last_mtime = std::fs::metadata(dir.join(&run_files[run_files.len() - 1])).and_then(|m| m.modified()).ok();
            let quiet = last_mtime.and_then(|t| t.elapsed().ok()).is_some_and(|e| e.as_secs_f64() > o.quiet_min * 60.0);
            if !(k < runs.len() - 1 || quiet) {
                println!("забег {run_id}: ещё идёт — разберу после");
                continue;
            }
            let (recs, saved) = replay(&dir, &data, run_files, o.budget, None)?;
            let mut found = findings(&recs);
            // Картинки без описания (JSON не записан: расчёт прервал следующий кадр).
            let (lo, hi) = (run_files[0][..6].to_string(), run_files[run_files.len() - 1][..6].to_string());
            let in_run = |p: &str| p.len() >= 6 && p[..6] >= *lo && p[..6] <= *hi;
            let mut lost: Vec<String> = std::fs::read_dir(&dir)
                .map_err(|e| e.to_string())?
                .flatten()
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|p| p.ends_with(".png") && in_run(p) && !dir.join(p.replace(".png", ".json")).exists())
                .collect();
            lost.sort();
            if !lost.is_empty() {
                found.push(Finding {
                    kind: "кадры",
                    time: hhmmss(&lost[0][..6]),
                    text: format!("картинок без JSON: {} ({})", lost.len(), lost.iter().take(3).cloned().collect::<Vec<_>>().join(", ")),
                    count: None,
                });
            }
            let log = log.get_or_insert_with(|| {
                find_log_directory()
                    .and_then(|d| std::fs::read(d.join("UE_game.log")).ok())
                    .map(|b| String::from_utf8_lossy(&b).lines().map(String::from).collect())
                    .unwrap_or_default()
            });
            found.extend(net_events(&saved, run_files, log));
            // Кадры — в папку данных, пока Горн их не удалил.
            if o.keep {
                let keep = data.join("runs").join(day);
                std::fs::create_dir_all(&keep).map_err(|e| e.to_string())?;
                for p in std::fs::read_dir(&dir).map_err(|e| e.to_string())?.flatten() {
                    let name = p.file_name().to_string_lossy().to_string();
                    if in_run(&name) && !keep.join(&name).exists() {
                        let _ = std::fs::copy(p.path(), keep.join(&name));
                    }
                }
            }
            let out = o.out.join(day);
            std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
            let path = out.join(format!("{st}.md"));
            std::fs::write(&path, report_md(day, &st, &recs, &found, run_files)).map_err(|e| e.to_string())?;
            let s = summary(&recs);
            index.insert(
                run_id.clone(),
                json!({ "at": now_iso(), "findings": found.len(), "prisms": s.prisms, "report": path.to_string_lossy() }),
            );
            std::fs::write(&index_path, serde_json::to_string_pretty(&Value::Object(index.clone())).unwrap()).map_err(|e| e.to_string())?;
            println!("забег {run_id}: {} призм., находок {} → {}", s.prisms, found.len(), path.display());
            done += 1;
        }
    }
    if done == 0 {
        println!("{}", if o.only_new { "новых завершённых забегов нет" } else { "забегов нет" });
    }
    Ok(())
}

fn now_iso() -> String {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let secs = t.as_secs() as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // гражданская дата из дней от эпохи (Хиннант)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z", rem / 3600, rem / 60 % 60, rem % 60, t.subsec_millis())
}

// ── JSON для сверки с эталоном ──────────────────────────────────────────────

pub fn heads_of(dir: &Path) -> Result<(Vec<String>, Vec<FrameHead>), String> {
    let mut files: Vec<String> = std::fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|f| f.ends_with(".json"))
        .collect();
    files.sort();
    let heads = files
        .iter()
        .map(|f| {
            let s: Value = std::fs::read_to_string(dir.join(f)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
            FrameHead { file: f.clone(), kind: s["kind"].as_str().unwrap_or("").into(), fresh: s["fresh"].as_bool().unwrap_or(false) }
        })
        .collect();
    Ok((files, heads))
}

pub fn heads_json(h: &[FrameHead]) -> Value {
    Value::Array(h.iter().map(|h| json!({ "file": h.file, "kind": h.kind, "fresh": h.fresh })).collect())
}

pub fn recs_json(recs: &[FrameRec]) -> Value {
    Value::Array(
        recs.iter()
            .map(|r| {
                let mut m = serde_json::Map::new();
                m.insert("file".into(), json!(r.file));
                m.insert("time".into(), json!(r.time));
                m.insert("kind".into(), json!(r.kind));
                m.insert("hasPng".into(), json!(r.has_png));
                m.insert("logDay".into(), r.log_day.map(|(c, x)| json!({ "cur": c, "max": x })).unwrap_or(Value::Null));
                m.insert("live".into(), json!(r.live));
                m.insert("answered".into(), json!(r.answered));
                m.insert("screen".into(), json!(r.screen));
                m.insert("actionable".into(), json!(r.actionable));
                m.insert("pickUnknown".into(), json!(r.pick_unknown));
                m.insert("mine".into(), json!(r.mine));
                m.insert("best".into(), json!(r.best));
                m.insert("liveGap".into(), json!(r.live_gap));
                m.insert("event".into(), json!(r.event));
                m.insert(
                    "mem".into(),
                    json!({ "day": r.mem.day, "dayLimit": r.mem.day_limit, "storage": r.mem.storage, "tools": r.mem.tools,
                            "assumed": r.mem.assumed, "step": r.mem.step, "start": r.mem.start }),
                );
                Value::Object(m)
            })
            .collect(),
    )
}

/// То, что считает audit-core эталона, — по нашим функциям.
pub fn core_json(heads: &[FrameHead], recs: &[FrameRec]) -> Value {
    let runs: Vec<Value> = split_runs(heads).iter().map(|&(f, t, s)| json!({ "from": f, "to": t, "start": s })).collect();
    let found: Vec<Value> = findings(recs)
        .iter()
        .map(|f| {
            let mut m = serde_json::Map::new();
            m.insert("kind".into(), json!(f.kind));
            m.insert("time".into(), json!(f.time));
            m.insert("text".into(), json!(f.text));
            if let Some(c) = f.count {
                m.insert("count".into(), json!(c));
            }
            Value::Object(m)
        })
        .collect();
    let s = summary(recs);
    json!({
        "runs": runs,
        "findings": found,
        "summary": { "start": s.start, "prisms": s.prisms, "days": s.days, "tools": s.tools, "theme": s.theme },
    })
}
