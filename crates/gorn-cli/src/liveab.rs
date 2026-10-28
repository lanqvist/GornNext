//! `gorn-cli live-ab`: A/B моделей правил на позициях живых забегов (журнал
//! фактов). Решатель прежней модели и решатель нынешней выбирают ход из одной
//! позиции; где ходы разошлись, глубокий решатель по нынешним правилам
//! оценивает оба. Средняя разница — сколько призм стоит прежняя модель на
//! реальных позициях; самоигра этого не видит, если упирается в хранилище.

use std::path::PathBuf;

use gorn_prison::actions::{legal_codes, CodeBuf};
use gorn_prison::policy::default_weights;
use gorn_prison::solver::{Search, SolveOptions, StopRule, STOP_DEFAULTS};
use gorn_prison::state::{State, SIZE};
use gorn_prison::{Params, DEFAULT_PARAMS};
use rayon::prelude::*;
use rusqlite::{Connection, OpenFlags};

fn positions(dbs: &[PathBuf]) -> Result<Vec<(String, State)>, String> {
    let mut out: Vec<(String, State)> = vec![];
    for db in dbs.iter().filter(|d| d.exists()) {
        let c = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|e| format!("{}: {e}", db.display()))?;
        let mut st = c.prepare("SELECT key, state FROM fact ORDER BY run_id, ts, id").map_err(|e| e.to_string())?;
        let rows = st.query_map([], |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, String>(1)?))).map_err(|e| e.to_string())?;
        for row in rows {
            let (key, state) = row.map_err(|e| e.to_string())?;
            let m: Vec<i32> = serde_json::from_str(&state).map_err(|e| e.to_string())?;
            if m.len() != SIZE {
                continue;
            }
            let mut s = State::new();
            s.m.copy_from_slice(&m);
            out.push((key.unwrap_or_default(), s));
        }
    }
    // Повторы одной позиции (пересъёмка кадра) — одна.
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out.dedup_by(|a, b| a.0 == b.0);
    let mut buf: CodeBuf = [0; 32];
    out.retain(|(_, s)| legal_codes(s, &mut buf) > 1);
    Ok(out)
}

fn seed_of(key: &str) -> u32 {
    key.bytes().fold(2_166_136_261u32, |h, b| (h ^ b as u32).wrapping_mul(16_777_619))
}

/// Ход решателя с потолком раундов (детерминированно).
fn choose(s: &State, seed: u32, rounds: u64, p: &Params) -> i32 {
    let stop = StopRule { max_rounds: rounds, ..STOP_DEFAULTS };
    Search::new(s, SolveOptions { seed, stop, known_events: false }, p, default_weights()).step(f64::INFINITY, None).best
}

/// Глубокая оценка всех ходов: ровно rounds раундов, без отсечения.
fn evaluate(s: &State, seed: u32, rounds: u64, p: &Params) -> Vec<(i32, f64)> {
    let stop = StopRule { min_rounds: rounds, max_rounds: rounds, z_prune: f64::INFINITY, ..STOP_DEFAULTS };
    let mut search = Search::new(s, SolveOptions { seed, stop, known_events: false }, p, default_weights());
    let mut res = search.step(f64::INFINITY, None);
    while !res.done {
        res = search.step(f64::INFINITY, None);
    }
    res.options.iter().map(|o| (o.code, o.mean)).collect()
}

/// old — варианты прежней модели (имена `rules-check` через запятую).
pub fn run(dbs: &[PathBuf], rounds: u64, ref_rounds: u64, threads: usize, old: &str) -> Result<(), String> {
    let pos = positions(dbs)?;
    let truth = DEFAULT_PARAMS;
    println!("прежняя модель — нынешние правила с вариантами: {old}");
    let old = crate::rulescheck::with_variants(old)?;
    let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build().map_err(|e| e.to_string())?;
    let t0 = std::time::Instant::now();
    // (ход прежней, ход нынешней, разница нынешняя − прежняя по глубокой оценке)
    let res: Vec<(String, i32, i32, f64)> = pool.install(|| {
        pos.par_iter()
            .map(|(key, s)| {
                let seed = seed_of(key);
                let a = choose(s, seed, rounds, &old);
                let b = choose(s, seed, rounds, &truth);
                if a == b {
                    return (key.clone(), a, b, 0.0);
                }
                let v = evaluate(s, seed ^ 0x9e37_79b9, ref_rounds, &truth);
                let mean = |c: i32| v.iter().find(|o| o.0 == c).map(|o| o.1).unwrap_or(0.0);
                (key.clone(), a, b, mean(b) - mean(a))
            })
            .collect()
    });
    let n = res.len() as f64;
    let d: Vec<f64> = res.iter().map(|r| r.3).collect();
    let md = d.iter().sum::<f64>() / n;
    let se = (d.iter().map(|x| (x - md) * (x - md)).sum::<f64>() / (n * (n - 1.0).max(1.0))).sqrt();
    let differ: Vec<&(String, i32, i32, f64)> = res.iter().filter(|r| r.1 != r.2).collect();
    println!(
        "Позиций {}: ходы разошлись в {}; нынешняя − прежняя модель: {:+.4} ± {:.4} призм на позицию ({:+.2} SE), сумма {:+.2}; {:.0} с",
        res.len(),
        differ.len(),
        md,
        se,
        if se > 0.0 { md / se } else { 0.0 },
        d.iter().sum::<f64>(),
        t0.elapsed().as_secs_f64()
    );
    let mut by_move: std::collections::BTreeMap<String, (usize, f64)> = Default::default();
    for r in &differ {
        let k = format!("{} → {}", gorn_prison::actions::describe(r.1), gorn_prison::actions::describe(r.2));
        let k = k.split_whitespace().filter(|w| w.parse::<i32>().is_err()).collect::<Vec<_>>().join(" ");
        let e = by_move.entry(k).or_default();
        e.0 += 1;
        e.1 += r.3;
    }
    let mut v: Vec<_> = by_move.into_iter().collect();
    v.sort_by(|a, b| b.1 .1.abs().partial_cmp(&a.1 .1.abs()).unwrap_or(std::cmp::Ordering::Equal));
    for (k, (c, s)) in v.iter().take(12) {
        println!("  {k}: {c} раз, {s:+.3} призм");
    }
    Ok(())
}

/// Калибровка обеих моделей: ожидание лучшего хода (глубокая оценка, каждая
/// по своим правилам) против итога доигранного забега, по дням. Начало
/// забега занижает и доигрывание мгновенной стратегией (слабее живого
/// решателя), поздние дни — честнее про сами правила.
pub fn calib(dbs: &[PathBuf], ref_rounds: u64, threads: usize, old: &str) -> Result<(), String> {
    let mut facts = vec![];
    for db in dbs.iter().filter(|d| d.exists()) {
        facts.extend(crate::learn::load(db)?);
    }
    let mut pos: Vec<(i64, f64, String, State)> = vec![];
    for run in facts.chunk_by(|a, b| a.run == b.run) {
        let last = run.last().unwrap();
        if last.day < last.state.m[gorn_prison::state::DAY_LIMIT] as i64 - 1 {
            continue;
        }
        let mut buf: CodeBuf = [0; 32];
        for f in run.iter().filter(|f| legal_codes(&f.state, &mut buf) > 1) {
            pos.push((f.day, last.storage as f64, f.key.clone(), f.state));
        }
    }
    pos.sort_by(|a, b| a.2.cmp(&b.2));
    pos.dedup_by(|a, b| a.2 == b.2);
    println!("прежняя модель — нынешние правила с вариантами: {old}; позиций {}, раундов {ref_rounds}", pos.len());
    let old = crate::rulescheck::with_variants(old)?;
    let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build().map_err(|e| e.to_string())?;
    let t0 = std::time::Instant::now();
    let best = |v: Vec<(i32, f64)>| v.iter().map(|o| o.1).fold(f64::MIN, f64::max);
    let res: Vec<(i64, f64, f64, f64)> = pool.install(|| {
        pos.par_iter()
            .map(|(day, y, key, s)| {
                let seed = seed_of(key);
                (*day, best(evaluate(s, seed, ref_rounds, &old)), best(evaluate(s, seed, ref_rounds, &DEFAULT_PARAMS)), *y)
            })
            .collect()
    });
    println!("дни      позиций   прежняя → итог   нынешняя → итог");
    let mut by_day: std::collections::BTreeMap<i64, (usize, f64, f64, f64)> = Default::default();
    for (d, a, b, y) in &res {
        let e = by_day.entry(d / 5 * 5).or_default();
        *e = (e.0 + 1, e.1 + a, e.2 + b, e.3 + y);
    }
    for (d, (n, a, b, y)) in &by_day {
        let n = *n as f64;
        println!("{:>2}–{:<2}   {:>7}   {:>6.2} → {:.2}    {:>6.2} → {:.2}", d, d + 4, n, a / n, y / n, b / n, y / n);
    }
    println!("{:.0} с", t0.elapsed().as_secs_f64());
    Ok(())
}
