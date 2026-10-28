//! `gorn-cli exam`: экзамен советника на сохранённых кадрах — весь путь игрока
//! (кадр → распознавание → память забега → совет) по порядку, как вживую, и
//! проверка каждого шага:
//!
//! - **разметка** (если есть): что на кадре на самом деле — камеры, надзиратель,
//!   двери, хранилище, орудия; расхождение распознавания с ней — ошибка;
//! - **судьи без разметки**: день на экране против лога, память против
//!   экрана, мигание распознавания между кадрами без нажатий, совет про
//!   карточку, которой нет на экране, орудие без эталона панели, выбор
//!   орудия по логу, после которого орудий не прибавилось, и др.
//!
//! Строка на кадр — `exam.jsonl` (что распознано, что в памяти, какой совет и
//! какие проверки не прошли), сводка — `exam.md`. Камеры — короткой строкой из
//! 16 знаков (см. `cell_token`): так их удобно размечать и сравнивать.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use gorn_prison::adapter::{CellK, CellMemory, RunMemory};
use gorn_prison::advisor::{last_hint, OwnAdvisor};
use gorn_prison::model::{OFFICE, TOOL_IDS};
use gorn_prison::solver::{StopRule, STOP_DEFAULTS};
use gorn_prison::view::{BoardView, CellKind, CellView, Decision, PickView, ScreenView};
use serde_json::{json, Value};

/// Звери одной буквой: мамонт (слон), лев, волк, крыса.
const BEAST_CH: [char; 4] = ['m', 'l', 'w', 'r'];

/// Камера знаками: `o` — допросная, `.` — пустая, `#` — заперта (`#3` —
/// показана редкость 3), `m2` — слон редкости 2 (`m?` — редкость не видна);
/// следом метки: `!` событие, `n` гнездо, `p` добыча, `t` бивень.
pub fn cell_token(c: &CellView) -> String {
    let mut s = match c.kind {
        CellKind::Office => "o".to_string(),
        CellKind::Empty => ".".to_string(),
        CellKind::Locked => match c.shown_tier {
            Some(t) => format!("#{t}"),
            None => "#".to_string(),
        },
        CellKind::Inmate => {
            let b = c.beast.map(|b| BEAST_CH[b as usize]).unwrap_or('x');
            match (c.tier, c.tier_unknown) {
                (_, Some(true)) | (None, _) => format!("{b}?"),
                (Some(t), _) => format!("{b}{t}"),
            }
        }
    };
    if c.event == Some(true) {
        s.push('!');
    }
    if c.nest == Some(true) {
        s.push('n');
    }
    if c.prey == Some(true) {
        s.push('p');
    }
    if c.tusk == Some(true) {
        s.push('t');
    }
    s
}

/// То же для памяти адаптера.
pub fn mem_token(c: &CellMemory) -> String {
    let mut s = match c.k.unwrap_or(CellK::Locked) {
        CellK::Office => "o".to_string(),
        CellK::Empty => ".".to_string(),
        CellK::Locked => match c.shown {
            Some(t) => format!("#{t}"),
            None => "#".to_string(),
        },
        CellK::Inmate => {
            let b = c.b.map(|b| BEAST_CH[b as usize]).unwrap_or('x');
            match c.t {
                Some(t) => format!("{b}{t}"),
                None => format!("{b}?"),
            }
        }
    };
    if c.ev {
        s.push('!');
    }
    if c.nest {
        s.push('n');
    }
    if c.tusk {
        s.push('t');
    }
    s
}

/// Основа знака без меток (для сравнения камер, где метки не важны).
fn core(tok: &str) -> &str {
    tok.trim_end_matches(['!', 'n', 'p', 't'])
}

pub fn screen_name(v: &ScreenView) -> &'static str {
    match v {
        ScreenView::Board(b) => {
            if b.chain.is_some() {
                "chain"
            } else if let Some(d) = &b.decision {
                match d {
                    Decision::Manage { .. } => "manage",
                    Decision::Office => "office",
                    Decision::Ratway => "ratway",
                    Decision::Event => "event",
                }
            } else if !b.doors.is_empty() {
                "patrol"
            } else {
                "board"
            }
        }
        ScreenView::Pick(_) => "pick",
        ScreenView::Treasure(_) => "treasure",
        ScreenView::Other => "other",
    }
}

fn board_json(b: &BoardView) -> Value {
    json!({
        "cells": b.cells.iter().map(cell_token).collect::<Vec<_>>().join(" "),
        "warden": b.warden,
        "daysLeft": b.days_left,
        "doors": b.doors,
        "decision": b.decision.as_ref().map(Decision::to_json),
        "chain": b.chain,
        "tools": b.tools,
        "toolSlots": b.tool_slots,
        "chromatic": b.chromatic,
        "objective": b.objective,
        "at": b.at.cells.iter().map(|c| [c.x.round(), c.y.round(), c.r.round()]).collect::<Vec<_>>(),
        "doorCards": b.at.door_cards.iter().map(|x| x.map(f64::round)).collect::<Vec<_>>(),
    })
}

fn mem_json(m: &RunMemory) -> Value {
    json!({
        "cells": m.cells.iter().map(mem_token).collect::<Vec<_>>().join(" "),
        "warden": m.warden,
        "day": m.day,
        "dayLimit": m.day_limit,
        "storage": m.storage,
        "tools": m.tools.iter().map(|&t| TOOL_IDS[t as usize]).collect::<Vec<_>>(),
        "step": m.step.name(),
        "gifts": m.gifts,
        "objectiveDone": m.objective_done,
        "objectiveProgress": m.objective_progress,
    })
}

/// Разметка: «папка дня/кадр» → поля, которые на кадре на самом деле.
/// Сравниваются только заданные поля; `cells` — 16 знаков как у `cell_token`
/// (`*` — не размечено), `note` — заметка человека.
pub fn load_labels(path: &Path) -> Result<BTreeMap<String, Value>, String> {
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let v: Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(v.as_object().map(|o| o.iter().filter(|(k, _)| !k.starts_with('_')).map(|(k, v)| (k.clone(), v.clone())).collect()).unwrap_or_default())
}

/// Сверка распознанного с разметкой по полям: (поле, верно, «было → верно»).
/// Поля разметки: `screen`, `cells` (16 знаков, `*` — не размечено; камера —
/// своё поле «№N»), `warden`, `doors` (номера с единицы, допросная — 1),
/// `daysLeft`, `chromatic`, `tools`, `chain`, `decision` («manage c r e» /
/// «office» / …), `cards` (карты выбора орудий).
pub fn label_fields(screen: &str, b: Option<&BoardView>, pick: Option<&PickView>, label: &Value) -> Vec<(String, bool, String)> {
    let mut out = vec![];
    let mut add = |f: &str, ok: bool, d: String| out.push((f.to_string(), ok, d));
    if let Some(s) = label["screen"].as_str() {
        add("screen", s == screen, format!("{screen} → {s}"));
    }
    if let (Some(w), Some(p)) = (label.get("cards").filter(|c| c.is_array()), pick) {
        add("pick", json!(p.cards) == *w, format!("{} → {w}", json!(p.cards)));
    }
    let Some(b) = b else {
        return out;
    };
    if let Some(cells) = label["cells"].as_str() {
        let want: Vec<&str> = cells.split_whitespace().collect();
        for (n, c) in b.cells.iter().enumerate().skip(1) {
            let Some(&w) = want.get(n) else { break };
            let got = cell_token(c);
            if w != "*" {
                add("cell", w == got, format!("№{}: {got} → {w}", n + 1));
            }
        }
    }
    if let Some(w) = label.get("warden").filter(|w| !w.is_null()) {
        let got = json!(b.warden.map(|x| x + 1));
        add("warden", got == *w, format!("{got} → {w}"));
    }
    if let Some(w) = label.get("doors").filter(|d| d.is_array()) {
        let got = json!(b.doors.iter().map(|d| d + 1).collect::<Vec<_>>());
        add("doors", got == *w, format!("{got} → {w}"));
    }
    if let Some(w) = label["chromatic"].as_i64() {
        add("chromatic", b.chromatic as i64 == w, format!("{} → {w}", b.chromatic));
    }
    if let Some(w) = label["daysLeft"].as_i64() {
        add("days", b.days_left.map(i64::from) == Some(w), format!("{:?} → {w}", b.days_left));
    }
    if let Some(w) = label.get("tools").filter(|t| t.is_array()) {
        add("tools", json!(b.tools) == *w, format!("{:?} → {w}", b.tools));
    }
    if let Some(w) = label.get("chain").filter(|t| t.is_array()) {
        let got = json!(b.chain.iter().flatten().map(|d| d + 1).collect::<Vec<_>>());
        add("chain", got == *w, format!("{got} → {w}"));
    }
    if let Some(w) = label["decision"].as_str() {
        let got = match &b.decision {
            None => "none".to_string(),
            Some(Decision::Manage { consume, replace, end, hunt }) => {
                let mut s = String::from("manage");
                for (on, k) in [(*consume, " consume"), (*replace, " replace"), (*hunt, " hunt"), (*end, " end")] {
                    if on {
                        s += k;
                    }
                }
                s
            }
            Some(d) => d.kind().to_string(),
        };
        add("decision", got == w, format!("{got} → {w}"));
    }
    out
}

/// Номер камеры из совета «Иди в №7», «Съесть №5» и т. п. (с единицы).
fn hint_cell(text: &str) -> Option<usize> {
    let i = text.find('№')?;
    let digits: String = text[i + '№'.len_utf8()..].chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// Что-то произошло между кадрами: нажатие, звук, окно, новый день.
fn eventful(events: &[String]) -> bool {
    !events.is_empty()
}

struct Prev {
    board: Option<BoardView>,
    tools: usize,
    storage: i32,
    picks_pending: u32,
    day: i32,
    /// Совет и живой совет на прошлой доске.
    hint: String,
    live: String,
}

#[derive(Default)]
struct Tally {
    frames: usize,
    boards: usize,
    labeled: usize,
    label_ok: usize,
    checks: BTreeMap<String, Vec<String>>,
}

impl Tally {
    fn flag(&mut self, kind: &str, msg: String) {
        self.checks.entry(kind.to_string()).or_default().push(msg);
    }
}

/// Пройти папки дней по порядку и написать exam.jsonl / exam.md в out.
pub fn run(dirs: &[PathBuf], from: &str, to: &str, rounds: u64, labels: &Path, out: &Path, ocr: bool) -> Result<(), String> {
    let labels = load_labels(labels)?;
    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let mut jsonl = String::new();
    let mut t = Tally::default();
    for dir in dirs {
        let day_name = dir.file_name().and_then(|s| s.to_str()).unwrap_or("").to_string();
        // Как живой советник (app/src/assist.rs): цепочка подарков включена.
        let mut adv = OwnAdvisor::new();
        adv.chain_gifts = true;
        adv.init(None, None);
        // Текст на кадре — OCR Windows, как в приложении (assist.rs).
        unsafe {
            let _ = windows::Win32::System::WinRT::RoInitialize(windows::Win32::System::WinRT::RO_INIT_MULTITHREADED);
        }
        if let Some(ocr) = ocr.then(crate::ocr::Ocr::new).and_then(Result::ok) {
            adv.card_text = Some(Box::new(move |f: &gorn_prison::recognize::Frame, b| {
                ocr.read(&crate::ocr::light_text(f.data, f.width, f.height, b)).unwrap_or_default()
            }));
        }
        let stop = StopRule { max_rounds: rounds, ..STOP_DEFAULTS };
        let mut prev = Prev { board: None, tools: 0, storage: 0, picks_pending: 0, day: -1, hint: String::new(), live: String::new() };
        crate::replay::run(dir, from, to, 1000.0, stop, f64::INFINITY, &mut adv, |adv, o| {
            let Some(saved) = o.saved else { return };
            if o.png.is_none() {
                return;
            }
            t.frames += 1;
            let stem = o.file.trim_end_matches(".json").to_string();
            let id = format!("{day_name}/{stem}");
            let lines: Vec<String> =
                saved["lines"].as_array().into_iter().flatten().filter_map(|l| l.as_str().map(String::from)).collect();
            let events = gorn_prison::logevents::compact(&lines);
            let view = adv.last_view.clone().unwrap_or(ScreenView::Other);
            let screen = screen_name(&view);
            let mem = &adv.adapter.mem;
            let hint = last_hint(&o.replies);
            let settled = saved["settled"].as_bool();
            let mut fails: Vec<String> = vec![];

            // Выбор орудия по логу: после него до следующей доски с панелью орудий должно прибавиться.
            let confirms = events.iter().filter(|e| e.as_str() == "click:Btn_S16Board_RelicPop_Confirm").count() as u32;
            prev.picks_pending += confirms;
            if mem.day < prev.day && mem.day <= 1 {
                prev.tools = 0;
                prev.storage = 0;
                prev.picks_pending = confirms;
                prev.board = None;
            }
            prev.day = mem.day;

            let board = if let ScreenView::Board(b) = &view { Some(b) } else { None };
            if let Some(b) = board {
                t.boards += 1;
                let chain = b.chain.is_some();
                // День на экране против лога.
                if let (Some(left), Some(d)) = (b.days_left, saved["day"].as_object()) {
                    let (cur, max) = (d["cur"].as_i64().unwrap_or(0), d["max"].as_i64().unwrap_or(0));
                    if max > 0 && i64::from(left) != max - cur {
                        fails.push(format!("день: на экране осталось {left}, по логу {cur}/{max}"));
                        t.flag("день на экране ≠ лог", format!("{id}: {left} против {cur}/{max}"));
                    }
                }
                // Портреты без зверя или редкости.
                for (n, c) in b.cells.iter().enumerate() {
                    if c.kind == CellKind::Inmate && c.beast.is_none() {
                        fails.push(format!("№{}: узник без зверя", n + 1));
                        t.flag("узник без зверя", format!("{id} №{}", n + 1));
                    }
                }
                // Место панели без эталона.
                if let Some(slots) = &b.tool_slots {
                    if slots.iter().any(Option::is_none) {
                        fails.push("панель: орудие без эталона".into());
                        t.flag("орудие на панели не узнано", id.clone());
                    }
                }
                // Память против экрана — где адаптер доску принял: не цепочка (доска
                // затемнена), не переходный кадр без совета, не выбор добычи.
                let taken = !chain && screen != "board" && !(screen == "ratway" && mem.step == gorn_prison::adapter::Step::Hunt);
                if taken && settled != Some(false) {
                    let mut diff = vec![];
                    for (n, c) in b.cells.iter().enumerate() {
                        if n as i32 == OFFICE {
                            continue;
                        }
                        let (v, m) = (cell_token(c), mem_token(&mem.cells[n]));
                        if core(&v) != core(&m) && !(core(&v).ends_with('?') && core(&m).starts_with(&core(&v)[..1])) {
                            diff.push(format!("№{} экран {v} память {m}", n + 1));
                        }
                    }
                    if !diff.is_empty() {
                        fails.push(format!("память ≠ экран: {}", diff.join(", ")));
                        t.flag("память ≠ экран", format!("{id}: {}", diff.join(", ")));
                    }
                    if b.chromatic >= 0 && b.chromatic != mem.storage {
                        fails.push(format!("хранилище: экран {} память {}", b.chromatic, mem.storage));
                        t.flag("хранилище: память ≠ экран", format!("{id}: {} против {}", b.chromatic, mem.storage));
                    }
                }
                // Мигание: соседние кадры без событий лога, а распознано по-разному.
                if let Some(p) = &prev.board {
                    if !eventful(&events) && settled == Some(true) && !chain && p.chain.is_none() {
                        let mut diff = vec![];
                        for (n, (a, c)) in p.cells.iter().zip(&b.cells).enumerate() {
                            let (x, y) = (cell_token(a), cell_token(c));
                            if x != y {
                                diff.push(format!("№{} {x}→{y}", n + 1));
                            }
                        }
                        if !diff.is_empty() {
                            fails.push(format!("мигание без событий: {}", diff.join(", ")));
                            t.flag("мигание без событий", format!("{id}: {}", diff.join(", ")));
                        }
                    }
                    // Призм не бывает меньше, чем было (в одном забеге).
                    if b.chromatic >= 0 && p.chromatic > b.chromatic && mem.day > 0 {
                        fails.push(format!("хранилище убыло: {} → {}", p.chromatic, b.chromatic));
                        t.flag("хранилище убыло", format!("{id}: {} → {}", p.chromatic, b.chromatic));
                    }
                }
                // Совет про дверь, которой нет среди карточек.
                if screen == "patrol" && hint.starts_with("Иди") {
                    if let Some(n) = hint_cell(&hint) {
                        if !b.doors.iter().any(|&d| d + 1 == n) {
                            fails.push(format!("совет «{hint}», а двери {:?}", b.doors.iter().map(|d| d + 1).collect::<Vec<_>>()));
                            t.flag("совет — дверь не с экрана", format!("{id}: «{hint}»"));
                        }
                    }
                }
                // Совет на экране цепочки — про подсвеченную цель.
                if chain {
                    if let (Some(n), Some(tg)) = (hint_cell(&hint), &b.chain) {
                        if !tg.iter().any(|&d| d + 1 == n) {
                            fails.push(format!("совет «{hint}», а цели {:?}", tg.iter().map(|d| d + 1).collect::<Vec<_>>()));
                            t.flag("совет — цель не с экрана", format!("{id}: «{hint}»"));
                        }
                    }
                }
                // Нажали Пожирание по совету — на выборе первой цели совет не отступает
                // [кадры 27.10 11:15:08–10: «съесть №6», затем «Отменить»: дверь-цель не видели].
                let live = crate::replay::saved_last_hint(&saved["hints"]);
                let bite = events.iter().any(|e| e == "click:Btn_S16Board_ActionCell");
                let turned = |before: &str, now: &str| chain && bite && before.starts_with("Пожирание: съесть") && now.starts_with("Отменить");
                if turned(&prev.hint, &hint) {
                    fails.push(format!("совет развернулся: «{}» → «{hint}»", prev.hint));
                    t.flag("совет развернулся (повтор)", format!("{id}: «{}» → «{hint}»", prev.hint));
                }
                if turned(&prev.live, &live) {
                    t.flag("совет развернулся (вживую)", format!("{id}: «{}» → «{live}»", prev.live));
                }
                prev.hint = hint.clone();
                prev.live = live;
                // Взятое орудие не прибавилось.
                if b.tool_slots.is_some() {
                    if prev.picks_pending > 0 && mem.tools.len() < prev.tools + prev.picks_pending as usize && !chain {
                        fails.push(format!("выбор орудия по логу ({}), а в памяти {:?}", prev.picks_pending, mem.tools.iter().map(|&x| TOOL_IDS[x as usize]).collect::<Vec<_>>()));
                        t.flag("взятое орудие потеряно", id.clone());
                    }
                    prev.tools = mem.tools.len();
                    prev.picks_pending = 0;
                }
                prev.board = Some(b.clone());
            }
            if prev.storage > mem.storage && mem.day > 0 {
                t.flag("хранилище в памяти убыло", format!("{id}: {} → {}", prev.storage, mem.storage));
            }
            prev.storage = mem.storage;

            // Разметка.
            let mut label_diff = vec![];
            if let Some(l) = labels.get(&id) {
                t.labeled += 1;
                let pick = if let ScreenView::Pick(p) = &view { Some(p) } else { None };
                label_diff = label_fields(screen, board, pick, l).into_iter().filter(|f| !f.1).map(|f| format!("{}: {}", f.0, f.2)).collect();
                if label_diff.is_empty() {
                    t.label_ok += 1;
                } else {
                    t.flag("разметка", format!("{id}: {}", label_diff.join("; ")));
                }
            }

            let row = json!({
                "id": id,
                "png": o.png.as_ref().map(|p| p.display().to_string()),
                "kind": saved["kind"],
                "logDay": saved["day"],
                "settled": settled,
                "events": events,
                "screen": screen,
                "board": board.map(board_json),
                "pick": if let ScreenView::Pick(p) = &view { json!({ "cards": p.cards, "reroll": p.reroll, "reward": p.reward }) } else { Value::Null },
                "mem": mem_json(mem),
                "hint": hint,
                "liveHint": crate::replay::saved_last_hint(&saved["hints"]),
                "options": adv.last_options.iter().map(|(c, m)| json!([gorn_prison::advisor::describe_code(*c), (m * 100.0).round() / 100.0])).collect::<Vec<_>>(),
                "fails": fails,
                "labelDiff": label_diff,
            });
            jsonl.push_str(&row.to_string());
            jsonl.push('\n');
        })?;
    }
    std::fs::write(out.join("exam.jsonl"), &jsonl).map_err(|e| e.to_string())?;

    let mut md = vec![
        "# Экзамен советника по кадрам".to_string(),
        String::new(),
        format!("Кадров с картинкой: {}, досок: {}", t.frames, t.boards),
    ];
    if t.labeled > 0 {
        md.push(format!(
            "Размечено: {}, распознано без ошибок: {} ({:.1} %)",
            t.labeled,
            t.label_ok,
            100.0 * t.label_ok as f64 / t.labeled as f64
        ));
    }
    md.push(String::new());
    md.push("| Проверка | Кадров |".into());
    md.push("|---|---|".into());
    for (k, v) in &t.checks {
        md.push(format!("| {k} | {} |", v.len()));
    }
    for (k, v) in &t.checks {
        md.push(String::new());
        md.push(format!("## {k}"));
        md.push(String::new());
        for x in v {
            md.push(format!("- {x}"));
        }
    }
    std::fs::write(out.join("exam.md"), md.join("\n") + "\n").map_err(|e| e.to_string())?;
    println!("кадров {}, досок {}; отчёт — {}", t.frames, t.boards, out.join("exam.md").display());
    if t.labeled > 0 {
        println!("разметка: {} кадров, без ошибок {} ({:.1} %)", t.labeled, t.label_ok, 100.0 * t.label_ok as f64 / t.labeled as f64);
    }
    for (k, v) in &t.checks {
        println!("  {k}: {}", v.len());
    }
    Ok(())
}
