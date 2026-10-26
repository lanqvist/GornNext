//! Распознавание против ручной разметки папки данных (`gorn-prison-data/labels`):
//! перенос `test/prison/recognize.test.ts` эталона и своя разметка кадров
//! Горна Next (`exam.json`). Счёт по полям: где распознавание ошибается и на
//! каких кадрах — для `gorn-cli exam --data`.
//!
//! Наборы: `boards-ru*.json` (доски целиком, ru — 2000 px, ru4k — 3840×2160,
//! честный набор), `picks-panels-digits.json` (карты выбора, панели, цифры),
//! `screens-ru.json` (тип экрана и фаза), кадры Горна 1920×1080
//! (`gorn-screens.json`, `gorn-new-screens.json`, `gorn-cells.json`,
//! `gorn-picks.json`), кадры Горна Next (`exam.json`, картинки в
//! `images/next/<дата>/`).

use std::collections::BTreeMap;
use std::path::Path;

use gorn_prison::model::BEASTS;
use gorn_prison::recognize::{read, Geometry, ReadOptions};
use gorn_prison::view::{BoardView, CellKind, Decision, ScreenView};
use serde_json::{json, Value};

use crate::frames::{data_dir, decode_png};

/// Счёт по полям: поле → (верно, всего), ошибки — строками.
#[derive(Default)]
pub struct Score {
    pub fields: BTreeMap<String, (u32, u32)>,
    pub errors: Vec<String>,
}

impl Score {
    pub fn add(&mut self, field: &str, ok: bool, where_: &str, detail: impl FnOnce() -> String) {
        let f = self.fields.entry(field.to_string()).or_default();
        f.1 += 1;
        if ok {
            f.0 += 1;
        } else {
            self.errors.push(format!("{where_} {field}: {}", detail()));
        }
    }

    pub fn table(&self) -> String {
        self.fields
            .iter()
            .map(|(k, (ok, n))| format!("  {k:<10} {ok}/{n} {:.1} %", 100.0 * *ok as f64 / (*n).max(1) as f64))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn load(name: &str) -> BTreeMap<String, Value> {
    let path = data_dir().join("labels").join(name);
    let Ok(text) = std::fs::read_to_string(&path) else { return BTreeMap::new() };
    let Ok(Value::Object(o)) = serde_json::from_str::<Value>(&text) else { return BTreeMap::new() };
    o.into_iter().filter(|(k, _)| !k.starts_with('_')).collect()
}

fn view_of(path: &Path, geo: Option<&Geometry>) -> Result<(ScreenView, Option<Geometry>), String> {
    let img = decode_png(path)?;
    Ok(read(&img.frame(), geo, ReadOptions::default()))
}

fn s(v: &Value) -> String {
    v.to_string()
}

fn phase_of(b: &BoardView) -> &'static str {
    if b.chain.is_some() {
        "chain"
    } else if let Some(d) = &b.decision {
        d.kind()
    } else if !b.doors.is_empty() {
        "patrol"
    } else {
        "none"
    }
}

/// Доска против разметки `boards-ru*.json` (`compareBoard` эталона).
fn compare_board(score: &mut Score, at: &str, b: &BoardView, l: &Value) {
    let nums = |k: &str| -> Vec<i64> { l[k].as_array().into_iter().flatten().filter_map(Value::as_i64).collect() };
    let (locked, events, nests, jaws) = (nums("locked"), nums("events"), nums("nests"), nums("jaws"));
    for n in 2..=16i64 {
        let c = &b.cells[n as usize - 1];
        let exp = match l["inmates"][n.to_string()].as_str() {
            Some(x) => format!("inmate {x}"),
            None if locked.contains(&n) => "locked".into(),
            None => "empty".into(),
        };
        let got = match c.kind {
            CellKind::Inmate => format!("inmate {}/{}", c.beast.map(|b| BEASTS[b as usize]).unwrap_or("?"), c.tier.map(|t| t.to_string()).unwrap_or("?".into())),
            k => k.name().to_string(),
        };
        score.add("cell", exp == got, &format!("{at} №{n}"), || format!("{exp} → {got}"));
        if locked.contains(&n) {
            score.add("event", (c.event == Some(true)) == events.contains(&n), &format!("{at} №{n}"), || format!("ждали {}", events.contains(&n)));
            score.add("nest", (c.nest == Some(true)) == nests.contains(&n), &format!("{at} №{n}"), || format!("ждали {}", nests.contains(&n)));
        }
        score.add("jaws", (c.jaws == Some(true)) == jaws.contains(&n), &format!("{at} №{n}"), || format!("ждали {}", jaws.contains(&n)));
    }
    let warden = l["warden"].as_i64().unwrap_or(0);
    score.add("warden", b.warden.map(|w| w as i64) == Some(warden - 1), at, || format!("{warden} → {:?}", b.warden.map(|w| w + 1)));
    let phase = if l["phase"] == "patrol" {
        "patrol"
    } else if warden == 1 {
        "office"
    } else {
        "manage"
    };
    let got_phase = match &b.decision {
        None if !b.doors.is_empty() => "patrol",
        None => "none",
        Some(d) => d.kind(),
    };
    score.add("phase", phase == got_phase, at, || format!("{phase} → {got_phase}"));
    let mut doors: Vec<i64> = b.doors.iter().filter(|&&d| d != 0).map(|&d| d as i64 + 1).collect();
    doors.extend(b.doors.iter().filter(|&&d| d == 0).map(|_| 1));
    score.add("doors", json!(doors) == l["doors"], at, || format!("{} → {doors:?}", s(&l["doors"])));
    score.add("days", b.days_left.map(i64::from) == l["days"].as_i64(), at, || format!("{} → {:?}", s(&l["days"]), b.days_left));
    let tools: Vec<String> = l["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(|t| if t == "casket" { "revenge".to_string() } else { t.to_string() })
        .collect();
    score.add("tools", tools == b.tools, at, || format!("{tools:?} → {:?}", b.tools));
    score.add("chromatic", Some(b.chromatic as i64) == l["chromatic"].as_i64(), at, || format!("{} → {}", s(&l["chromatic"]), b.chromatic));
}

fn boards(score: &mut Score, set: &str, dir: &str) -> Result<(), String> {
    for (name, l) in load(set) {
        let (v, _) = view_of(&data_dir().join("images").join(dir).join(&name), None)?;
        let ScreenView::Board(b) = v else {
            score.add("screen", false, &name, || "не доска".into());
            continue;
        };
        score.add("screen", true, &name, String::new);
        compare_board(score, &name, &b, &l);
    }
    Ok(())
}

/// `picks-panels-digits.json`: карты выбора, панели орудий, дни и двери.
fn picks_panels_digits(score: &mut Score) -> Result<(), String> {
    let all = load("picks-panels-digits.json");
    let img = |n: &str| data_dir().join("images").join(n);
    for (name, exp) in all.get("picks").and_then(Value::as_object).into_iter().flatten() {
        let (v, _) = view_of(&img(name), None)?;
        let got = match &v {
            ScreenView::Pick(p) => json!(p.cards),
            v => json!(v.to_json()["screen"]),
        };
        score.add("pick", got == *exp, name, || format!("{} → {got}", s(exp)));
    }
    for (name, exp) in all.get("panels").and_then(Value::as_object).into_iter().flatten() {
        let (v, _) = view_of(&img(name), None)?;
        let got = if let ScreenView::Board(b) = &v { json!(b.tools) } else { json!("не доска") };
        score.add("tools", got == *exp, name, || format!("{} → {got}", s(exp)));
    }
    for (name, exp) in all.get("digits").and_then(Value::as_object).into_iter().flatten() {
        let (v, _) = view_of(&img(name), None)?;
        let ScreenView::Board(b) = &v else {
            score.add("screen", false, name, || "не доска".into());
            continue;
        };
        let doors: Vec<i64> = b.doors.iter().filter(|&&d| d != 0).map(|&d| d as i64 + 1).collect();
        score.add("days", b.days_left.map(i64::from) == exp["days"].as_i64(), name, || format!("{} → {:?}", s(&exp["days"]), b.days_left));
        score.add("doors", json!(doors) == exp["doors"], name, || format!("{} → {doors:?}", s(&exp["doors"])));
    }
    Ok(())
}

/// `screens-ru.json`: тип экрана, фаза доски, цели цепочки, флаги выбора.
fn screens_ru(score: &mut Score) -> Result<(), String> {
    for (name, l) in load("screens-ru.json") {
        let (v, _) = view_of(&data_dir().join("images").join(&name), None)?;
        let screen = v.to_json()["screen"].as_str().unwrap_or("").to_string();
        let want = l["screen"].as_str().unwrap_or("");
        score.add("screen", screen == want, &name, || format!("{want} → {screen}"));
        if screen != want {
            continue;
        }
        match &v {
            ScreenView::Pick(p) => {
                if let Some(r) = l["reward"].as_bool() {
                    score.add("reward", p.reward == r, &name, || format!("{r} → {}", p.reward));
                }
                if let Some(r) = l["reroll"].as_bool() {
                    score.add("reroll", p.reroll == r, &name, || format!("{r} → {}", p.reroll));
                }
            }
            ScreenView::Treasure(g) => score.add("gifts", g.len() == 4, &name, || format!("подарков {}", g.len())),
            ScreenView::Board(b) => {
                if let Some(ph) = l["phase"].as_str() {
                    let got = phase_of(b);
                    score.add("phase", got == ph, &name, || format!("{ph} → {got}"));
                    if let Some(t) = l.get("targets").filter(|t| !t.is_null()) {
                        let got: Option<Vec<usize>> = b.chain.as_ref().map(|c| c.iter().map(|x| x + 1).collect());
                        score.add("targets", json!(got) == *t, &name, || format!("{} → {got:?}", s(t)));
                    }
                }
            }
            ScreenView::Other => {}
        }
    }
    Ok(())
}

/// Кадры Горна 1920×1080: экраны, камеры, выборы (`все размеченные поля` эталона).
fn gorn(score: &mut Score) -> Result<(), String> {
    let mut screens = load("gorn-screens.json");
    screens.extend(load("gorn-new-screens.json"));
    let cells = load("gorn-cells.json");
    let picks = load("gorn-picks.json");
    let mut keys: Vec<&String> = screens.keys().chain(cells.keys()).chain(picks.keys()).collect();
    keys.sort();
    keys.dedup();
    let mut geo: Option<Geometry> = None;
    for key in keys {
        let path = if key.starts_with("gorn-new/") { key.clone() } else { format!("gorn/{key}") };
        let (v, g) = view_of(&data_dir().join("images").join(&path), geo.as_ref())?;
        geo = g.or(geo);
        let sc = screens.get(key);
        let p = picks.get(key).or(sc.filter(|x| x.get("cards").is_some() && path.contains("instruments")));
        if let Some(p) = p {
            let ok = matches!(&v, ScreenView::Pick(pv) if json!(pv.cards) == p["cards"] && Some(pv.reward) == p["reward"].as_bool());
            score.add("pick", ok, key, || match &v {
                ScreenView::Pick(pv) => format!("{} → {}", s(&p["cards"]), json!(pv.cards)),
                v => format!("экран {}", v.to_json()["screen"]),
            });
            continue;
        }
        if let Some(want) = sc.and_then(|x| x["screen"].as_str()) {
            let got = v.to_json()["screen"].as_str().unwrap_or("").to_string();
            score.add("screen", got == want, key, || format!("{want} → {got}"));
            continue;
        }
        let ScreenView::Board(b) = &v else {
            score.add("screen", false, key, || format!("{} вместо доски", v.to_json()["screen"]));
            continue;
        };
        for (n, cls) in cells.get(key).and_then(Value::as_object).into_iter().flatten() {
            let c = &b.cells[n.parse::<usize>().unwrap_or(1) - 1];
            let got = match c.kind {
                CellKind::Inmate => c.beast.map(|b| BEASTS[b as usize]).unwrap_or("?").to_string(),
                CellKind::Locked if c.shown_tier.is_some() => "unknown".into(),
                k => k.name().to_string(),
            };
            let want = cls.as_str().unwrap_or("");
            score.add("cell", got == want, &format!("{key} №{n}"), || format!("{want} → {got}"));
        }
        let Some(sc) = sc else { continue };
        if let Some(d) = sc["days"].as_i64() {
            score.add("days", b.days_left.map(i64::from) == Some(d), key, || format!("{d} → {:?}", b.days_left));
        }
        if let Some(h) = sc["header"].as_str() {
            let got = match &b.decision {
                Some(Decision::Office) => "manage",
                Some(d) => d.kind(),
                None if !b.doors.is_empty() => "patrol",
                None => "none",
            };
            score.add("header", got == h, key, || format!("{h} → {got}"));
        }
        if let Some(cards) = sc["cards"].as_array() {
            let has = |c: &str| cards.iter().any(|x| x == c);
            let want = if has("bestow") { "office".to_string() } else { format!("manage {} {} {}", has("consume"), has("replace"), has("end")) };
            let got = match &b.decision {
                None => "null".to_string(),
                Some(Decision::Manage { consume, replace, end, .. }) => format!("manage {consume} {replace} {end}"),
                Some(d) => d.kind().to_string(),
            };
            score.add("cards", got == want, key, || format!("{want} → {got}"));
        }
        if let Some(t) = sc.get("tools").filter(|t| !t.is_null()) {
            score.add("tools", json!(b.tools) == *t, key, || format!("{} → {:?}", s(t), b.tools));
        }
        if let Some(c) = sc["chromatic"].as_i64() {
            score.add("chromatic", b.chromatic as i64 == c, key, || format!("{c} → {}", b.chromatic));
        }
        if let Some(o) = sc["objective"].as_str() {
            score.add("objective", b.objective.as_deref() == Some(o), key, || format!("{o} → {:?}", b.objective));
        }
    }
    Ok(())
}

/// Своя разметка кадров Горна Next (`exam.json`): «дата/кадр» → поля, как в
/// `exam::against_labels`; картинка — `images/next/<дата>/<кадр>.png` или ещё
/// не убранная из «Изображения/Горн Next — тюрьма».
fn next(score: &mut Score) -> Result<(), String> {
    let pictures = std::path::PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join("Pictures").join("Горн Next — тюрьма");
    for (key, l) in load("exam.json") {
        let Some((day, stem)) = key.split_once('/') else { continue };
        let png = [data_dir().join("images").join("next").join(day), pictures.join(day)]
            .into_iter()
            .map(|d| d.join(format!("{stem}.png")))
            .find(|p| p.exists());
        let Some(png) = png else {
            score.add("кадр", false, &key, || "картинки нет".into());
            continue;
        };
        let (v, _) = view_of(&png, None)?;
        let screen = crate::exam::screen_name(&v);
        let board = if let ScreenView::Board(b) = &v { Some(b) } else { None };
        let pick = if let ScreenView::Pick(p) = &v { Some(p) } else { None };
        for (field, ok, detail) in crate::exam::label_fields(screen, board, pick, &l) {
            score.add(&field, ok, &key, || detail);
        }
    }
    Ok(())
}

/// Все наборы: (название, счёт).
pub fn run_all() -> Result<Vec<(&'static str, Score)>, String> {
    let mut out = vec![];
    let mut sc = Score::default();
    boards(&mut sc, "boards-ru4k.json", "ru4k")?;
    out.push(("boards-ru4k (честный, 3840×2160)", sc));
    let mut sc = Score::default();
    boards(&mut sc, "boards-ru.json", "ru")?;
    out.push(("boards-ru (2000 px)", sc));
    let mut sc = Score::default();
    picks_panels_digits(&mut sc)?;
    out.push(("picks-panels-digits", sc));
    let mut sc = Score::default();
    screens_ru(&mut sc)?;
    out.push(("screens-ru", sc));
    let mut sc = Score::default();
    gorn(&mut sc)?;
    out.push(("кадры Горна 1920×1080", sc));
    let mut sc = Score::default();
    next(&mut sc)?;
    out.push(("кадры Горна Next (exam.json)", sc));
    Ok(out)
}
