//! `gorn-cli learn`: отчёт «ожидали → получили» по журналу фактов тюрьмы
//! (этап 7, шаг 2).
//!
//! Пара соседних решений забега — позиция до и позиция после. Каждый ход
//! из позиции «до» проигрывается движком много раз (скрытое — узники за
//! дверями, виды событий — разыгрывается заново, как у решателя), и
//! результаты сравниваются с позицией «после» по видимому: звери, редкости
//! и замки камер, надзиратель, день, фаза, призмы. Отсюда:
//!
//! - какой ход сделан и мог ли движок дать ровно такой исход («объяснён»);
//! - появление узников (из двери и на замену): наблюдённые редкости против
//!   ожидаемых движком при тех же орудиях и том же ходе;
//! - калибровка: сколько призм советник ждал и сколько вышло, по дням;
//! - поля, в которых движок чаще всего расходится с игрой.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use gorn_prison::actions::apply_code;
use gorn_prison::model::{CELLS, DEFAULT_PARAMS};
use gorn_prison::rng::Rng;
use gorn_prison::solver::redeterminize;
use gorn_prison::state::*;
use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Value};

/// Розыгрышей на ход.
const SAMPLES: u32 = 300;

const TIERS: [&str; 6] = ["обычный", "необычный", "редкий", "эпический", "легендарный", "мифический"];

struct Fact {
    run: String,
    ts: i64,
    key: String,
    day: i64,
    state: State,
    advised: i32,
    /// Ходы решателя: код, среднее призм к концу.
    options: Vec<(i32, f64)>,
    storage: i64,
}

fn load(db: &Path) -> Result<Vec<Fact>, String> {
    let c = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|e| format!("{}: {e}", db.display()))?;
    let mut st = c
        .prepare("SELECT run_id, ts, key, day, state, advised, options, storage FROM fact ORDER BY run_id, ts, id")
        .map_err(|e| e.to_string())?;
    let tag = db.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let rows = st
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<i64>>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, Option<i64>>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, Option<i64>>(7)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut out = vec![];
    for row in rows {
        let (run, ts, key, day, state, advised, options, storage) = row.map_err(|e| e.to_string())?;
        let m: Vec<i32> = serde_json::from_str(&state).map_err(|e| e.to_string())?;
        if m.len() != SIZE {
            continue; // позиция другой версии раскладки
        }
        let mut s = State::new();
        s.m.copy_from_slice(&m);
        let opts: Value = serde_json::from_str(&options).map_err(|e| e.to_string())?;
        out.push(Fact {
            run: format!("{tag}#{run}"),
            ts,
            key: key.unwrap_or_default(),
            day: day.unwrap_or(-1),
            state: s,
            advised: advised.unwrap_or(-1) as i32,
            options: opts
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|o| Some((o["code"].as_i64()? as i32, o["mean"].as_f64()?)))
                .collect(),
            storage: storage.unwrap_or(0),
        });
    }
    Ok(out)
}

/// Видимые поля позиции: камеры (зверь, редкость, замок) и скаляры.
fn visible() -> Vec<usize> {
    let mut v = vec![];
    for c in 0..CELLS {
        v.extend([BEAST + c, TIER + c, LOCK + c]);
    }
    v.extend([WARDEN, DAY, PHASE, STORAGE]);
    v
}

fn diff(a: &State, b: &State, fields: &[usize]) -> Vec<usize> {
    fields.iter().copied().filter(|&i| a.m[i] != b.m[i]).collect()
}

/// Новый узник в камере c: из двери (была заперта) или на замену.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Arrival {
    Door,
    Replace,
}

/// Кто появился в камерах между позициями: (камера, откуда, редкость).
fn arrivals(before: &State, after: &State) -> Vec<(usize, Arrival, i32)> {
    let mut v = vec![];
    for c in 1..CELLS {
        let t = after.m[TIER + c];
        if t < 0 || after.m[LOCK + c] == 1 {
            continue;
        }
        if before.m[LOCK + c] == 1 {
            // Редкость за дверью была видна — не случайность двери.
            if before.m[FLAGS + c] & F_VIS_TIER == 0 && before.m[PRE_TIER + c] < 0 {
                v.push((c, Arrival::Door, t));
            }
        } else if before.m[TIER + c] != t || before.m[BEAST + c] != after.m[BEAST + c] {
            if before.m[TIER + c] < 0 || before.m[BEAST + c] != after.m[BEAST + c] {
                v.push((c, Arrival::Replace, t));
            }
        }
    }
    v
}

#[derive(Default)]
struct Pair {
    /// Ход, лучше всех объясняющий переход; совпал ли он с советом.
    code: i32,
    followed: bool,
    /// Доля розыгрышей этого хода, давших ровно наблюдённое.
    exact: f64,
    /// Наименьшее число несовпавших полей у розыгрышей этого хода.
    min_diff: usize,
    /// Поля, не совпавшие у ближайшего розыгрыша, и что он дал.
    fields: Vec<usize>,
    closest: State,
}

pub fn run(dbs: &[PathBuf], json_out: bool) -> Result<(), String> {
    let p = &DEFAULT_PARAMS;
    let fields = visible();
    let mut facts = vec![];
    for db in dbs {
        if db.exists() {
            facts.extend(load(db)?);
        }
    }
    // Повторы одной позиции (пересъёмка кадра) — одно решение.
    facts.dedup_by(|b, a| a.run == b.run && a.key == b.key);
    let mut by_run: BTreeMap<String, Vec<Fact>> = BTreeMap::new();
    for f in facts {
        by_run.entry(f.run.clone()).or_default().push(f);
    }

    let (mut pairs, mut exact, mut near, mut far, mut followed, mut gaps) = (0usize, 0usize, 0usize, 0usize, 0usize, 0usize);
    let mut field_miss: BTreeMap<String, usize> = BTreeMap::new();
    // Появление узников: наблюдено и ожидалось движком, по (откуда, редкость).
    let mut seen: BTreeMap<(Arrival, i32), f64> = BTreeMap::new();
    let mut expect: BTreeMap<(Arrival, i32), f64> = BTreeMap::new();
    let mut loglik = 0.0;
    let mut unexplained: Vec<Value> = vec![];
    // Калибровка: (день, ожидание, итог забега).
    let mut calib: Vec<(i64, f64, f64)> = vec![];
    let mut runs_done = 0usize;

    for (run, fs) in &by_run {
        let last = fs.last().map(|f| f.storage as f64).unwrap_or(0.0);
        let limit = fs.first().map(|f| f.state.m[DAY_LIMIT] as i64).unwrap_or(20);
        // Итог известен только у доигранного забега.
        let complete = fs.last().is_some_and(|f| f.day >= limit - 1);
        if complete {
            runs_done += 1;
            for f in fs {
                if let Some(best) = f.options.iter().find(|o| o.0 == f.advised).or(f.options.first()) {
                    calib.push((f.day, best.1, last));
                }
            }
        }
        for w in fs.windows(2) {
            let (a, b) = (&w[0], &w[1]);
            pairs += 1;
            let mut best: Option<Pair> = None;
            let mut sim_arrivals: Vec<(Arrival, i32)> = vec![];
            for (i, &(code, _)) in a.options.iter().enumerate() {
                let mut hits = 0u32;
                let mut min_diff = usize::MAX;
                let mut miss: Vec<usize> = vec![];
                let mut closest = a.state;
                let mut arr: Vec<(Arrival, i32)> = vec![];
                let mut legal = true;
                for k in 0..SAMPLES {
                    let mut r = Rng::new(gorn_prison::rng::mix_pair(k * 7919 + 1, (i as u32) << 16 ^ pairs as u32));
                    let mut s = a.state;
                    redeterminize(&mut s, &mut r, false, p);
                    if apply_code(&mut s, code, &mut r, p).is_err() {
                        legal = false;
                        break;
                    }
                    let d = diff(&s, &b.state, &fields);
                    if d.is_empty() {
                        hits += 1;
                    }
                    if d.len() < min_diff {
                        min_diff = d.len();
                        miss = d;
                        closest = s;
                    }
                    arr.extend(arrivals(&a.state, &s).into_iter().map(|(_, k, t)| (k, t)));
                }
                if !legal {
                    continue;
                }
                let cand = Pair {
                    code,
                    followed: code == a.advised,
                    exact: hits as f64 / SAMPLES as f64,
                    min_diff,
                    fields: miss,
                    closest,
                };
                let better = match &best {
                    None => true,
                    Some(o) => cand.exact > o.exact || (cand.exact == o.exact && cand.min_diff < o.min_diff),
                };
                if better {
                    best = Some(cand);
                    sim_arrivals = arr;
                }
            }
            let Some(pr) = best else { continue };
            if pr.followed {
                followed += 1;
            }
            // Следующий факт — дальше, чем доходит один ход: между ними были
            // решения без кадра (пропуск), это не ошибка модели правил.
            if pr.exact == 0.0 && (b.state.m[DAY] > pr.closest.m[DAY] || b.state.m[DAY] < a.state.m[DAY]) {
                gaps += 1;
                continue;
            }
            if pr.exact > 0.0 {
                exact += 1;
                loglik += pr.exact.ln();
                // Появление узников — только по объяснённым переходам: ход известен.
                for (_, k, t) in arrivals(&a.state, &b.state) {
                    *seen.entry((k, t)).or_default() += 1.0;
                }
                for (k, t) in &sim_arrivals {
                    *expect.entry((*k, *t)).or_default() += 1.0 / SAMPLES as f64;
                }
            } else if pr.min_diff <= 3 {
                near += 1;
            } else {
                far += 1;
            }
            if pr.exact == 0.0 {
                for &f in &pr.fields {
                    *field_miss.entry(field_name(f)).or_default() += 1;
                }
                if unexplained.len() < 40 {
                    unexplained.push(json!({
                        "run": run, "ts": a.ts, "day": a.day, "code": pr.code, "move": gorn_prison::actions::describe(pr.code),
                        "fields": pr.fields.iter().map(|&f| json!([field_name(f), pr.closest.m[f], b.state.m[f]])).collect::<Vec<_>>(),
                    }));
                }
            }
        }
    }

    // Калибровка по дням: среднее (ожидание − итог).
    let mut by_day: BTreeMap<i64, (f64, f64, usize)> = BTreeMap::new();
    for (d, e, y) in &calib {
        let b = by_day.entry((*d / 5) * 5).or_default();
        b.0 += e;
        b.1 += y;
        b.2 += 1;
    }

    if json_out {
        let v = json!({
            "runs": by_run.len(), "complete": runs_done, "pairs": pairs, "exact": exact, "near": near, "far": far, "gaps": gaps,
            "followed": followed, "loglik": loglik,
            "arrivals": seen.keys().chain(expect.keys()).collect::<std::collections::BTreeSet<_>>().into_iter()
                .map(|k| json!({ "from": if k.0 == Arrival::Door { "door" } else { "replace" }, "tier": k.1,
                                 "seen": seen.get(k).copied().unwrap_or(0.0), "expected": expect.get(k).copied().unwrap_or(0.0) }))
                .collect::<Vec<_>>(),
            "calibration": by_day.iter().map(|(d, (e, y, n))| json!({ "days": d, "expected": e / *n as f64, "actual": y / *n as f64, "n": n })).collect::<Vec<_>>(),
            "fields": field_miss,
            "unexplained": unexplained,
        });
        println!("{}", serde_json::to_string_pretty(&v).unwrap());
        return Ok(());
    }

    let pct = |a: usize, b: usize| if b == 0 { 0.0 } else { 100.0 * a as f64 / b as f64 };
    println!("Журнал: забегов {}, доиграно {runs_done}, переходов между решениями {pairs}", by_run.len());
    println!();
    println!("Переходы (ход — тот, что лучше всех объясняет следующий кадр):");
    println!("  пропуск решений между     {gaps} ({:.0} %) — не в счёт ниже", pct(gaps, pairs));
    let pairs = pairs - gaps;
    println!("  объяснены движком точно   {exact} ({:.0} %)", pct(exact, pairs));
    println!("  близко (≤ 3 поля мимо)    {near} ({:.0} %)", pct(near, pairs));
    println!("  не объяснены              {far} ({:.0} %)", pct(far, pairs));
    println!("  ход совпал с советом      {followed} ({:.0} %)", pct(followed, pairs));
    println!("  лог-правдоподобие точных  {loglik:.1} (больше — модель правил ближе к игре)");
    println!();
    println!("Новые узники (по точно объяснённым переходам): наблюдено / ожидалось движком");
    for (from, name) in [(Arrival::Door, "из двери"), (Arrival::Replace, "на замену")] {
        let n_seen: f64 = seen.iter().filter(|(k, _)| k.0 == from).map(|(_, v)| v).sum();
        let n_exp: f64 = expect.iter().filter(|(k, _)| k.0 == from).map(|(_, v)| v).sum();
        if n_seen == 0.0 && n_exp == 0.0 {
            continue;
        }
        println!("  {name}: всего {n_seen:.0} / {n_exp:.1}");
        for t in 0..6 {
            let s = seen.get(&(from, t)).copied().unwrap_or(0.0);
            let e = expect.get(&(from, t)).copied().unwrap_or(0.0);
            if s == 0.0 && e < 0.05 {
                continue;
            }
            let share = |x: f64, n: f64| if n > 0.0 { 100.0 * x / n } else { 0.0 };
            // Отклонение в стандартных ошибках (биномиальная модель доли).
            let q = if n_exp > 0.0 { e / n_exp } else { 0.0 };
            let sd = (n_seen * q * (1.0 - q)).sqrt();
            let z = if sd > 0.0 { (s - n_seen * q) / sd } else { 0.0 };
            let mark = if z.abs() >= 2.0 { "  ← заметно" } else { "" };
            println!(
                "    {:<12} {s:>4.0} ({:>4.1} %)   ожидалось {e:>5.1} ({:>4.1} %)   {z:+.1} SE{mark}",
                TIERS[t as usize],
                share(s, n_seen),
                share(e, n_exp)
            );
        }
    }
    println!();
    let (se, sy) = calib.iter().fold((0.0, 0.0), |a, c| (a.0 + c.1, a.1 + c.2));
    let n = calib.len().max(1) as f64;
    println!("Калибровка (доигранные забеги): ожидал призм к концу → вышло; в среднем {:.2} → {:.2}", se / n, sy / n);
    for (d, (e, y, n)) in &by_day {
        println!("  дни {:>2}–{:<2}  {:>5.2} → {:>5.2}  (решений {n})", d, d + 4, e / *n as f64, y / *n as f64);
    }
    if !field_miss.is_empty() {
        println!();
        println!("Поля, где движок не дал наблюдённого (у ближайших розыгрышей):");
        let mut v: Vec<_> = field_miss.into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1));
        for (f, n) in v.iter().take(15) {
            println!("  {f:<20} {n}");
        }
    }
    if !unexplained.is_empty() {
        println!();
        println!("Не объяснённые переходы (первые {}): забег, день, ход — поле (движок дал → вышло)", unexplained.len().min(10));
        for u in unexplained.iter().take(10) {
            let fs: Vec<String> = u["fields"].as_array().unwrap().iter().take(4).map(|f| format!("{} {}→{}", f[0].as_str().unwrap_or(""), f[1], f[2])).collect();
            println!("  {} д{} {} — {}", u["run"].as_str().unwrap_or(""), u["day"], u["move"].as_str().unwrap_or(""), fs.join(", "));
        }
    }
    Ok(())
}
