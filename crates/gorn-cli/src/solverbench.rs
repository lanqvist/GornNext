//! Решатель в CLI: самоигра с проверкой вариантами (`play`, аналог
//! `prison-solve-bench --play`) и замер советов на позициях (`solve-bench`,
//! аналог `prison-solve-bench` с `--rate` и `--regret`).

use std::collections::HashMap;
use std::time::Instant as Clock;

use gorn_prison::actions::{apply_code, legal_codes, start_run, CodeBuf, RunOptions};
use gorn_prison::policy::{default_weights, Instant};
use gorn_prison::rng::Rng;
use gorn_prison::runs::solve_positions;
use gorn_prison::solver::{Search, SolveOptions, StopRule, STOP_DEFAULTS, STOP_FREE, STOP_OLD};
use gorn_prison::state::{State, OFFER_KIND, OFFER_TOOL, PHASE, PHASE_NAMES, P_END, P_PICK, STORAGE};
use gorn_prison::{Params, StreamHash, DEFAULT_PARAMS};
use rayon::prelude::*;
use rayon::ThreadPool;

/// Показ первого совета (мс) — как QUICK_MS в ownAdvisor.ts.
pub const QUICK_MS: f64 = 300.0;
/// Досчёт обычного хода до (мс).
pub const BUDGET_MS: f64 = 1500.0;
/// Выбор орудий: досчёт до PICK_BUDGET_MS и не меньше PICK_ROUNDS раундов до остановки.
pub const PICK_BUDGET_MS: f64 = 4000.0;
pub const PICK_ROUNDS: u64 = 1500;

fn tool_pick(s: &State) -> bool {
    s.m[PHASE] == P_PICK && s.m[OFFER_KIND] == OFFER_TOOL
}

/// Совет как в потоке подсказок: показ через QUICK_MS, досчёт до 1,5 с
/// (выбор орудий — до 4 с и ≥ 1500 раундов), смена — если новый заметно лучше.
/// Возвращает (ход, мс до первого совета).
pub fn advise_like(s: &State, seed: u32, p: &Params, pool: Option<&ThreadPool>) -> (i32, f64) {
    let tp = tool_pick(s);
    let t0 = Clock::now();
    let stop = if tp { StopRule { min_rounds: PICK_ROUNDS, ..STOP_DEFAULTS } } else { STOP_DEFAULTS };
    let mut search = Search::new(s, SolveOptions { seed, stop, known_events: false }, p, default_weights());
    let res = search.step(QUICK_MS, pool);
    let first = t0.elapsed().as_secs_f64() * 1000.0;
    let shown = res.best;
    if res.done {
        return (shown, first);
    }
    let res = search.step((if tp { PICK_BUDGET_MS } else { BUDGET_MS }) - QUICK_MS, pool);
    let best = res.best;
    (if best != shown && search.clearly_better(best, shown) { best } else { shown }, first)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PlayMode {
    /// Как в потоке подсказок (`--mode new`).
    New,
    /// То же, но выбор орудий как любой ход.
    Flat,
    /// Прежнее правило (64 раунда, 3 SE, ε 0,1), ход окончательный.
    Old,
    /// Без срока: правило по умолчанию и потолок раундов — детерминированно,
    /// ход в ход как `play` в tools/reference.mts.
    Fixed(u64),
}

/// Ход решателя в самоигре.
fn choose(s: &State, seed: u32, mode: PlayMode, p: &Params, pool: Option<&ThreadPool>) -> i32 {
    let w = default_weights();
    match mode {
        PlayMode::Old => Search::new(s, SolveOptions { seed, stop: STOP_OLD, known_events: false }, p, w).step(1500.0, pool).best,
        PlayMode::New => advise_like(s, seed, p, pool).0,
        PlayMode::Flat => {
            let mut search = Search::new(s, SolveOptions::new(seed), p, w);
            let res = search.step(QUICK_MS, pool);
            let shown = res.best;
            if res.done {
                return shown;
            }
            let best = search.step(BUDGET_MS - QUICK_MS, pool).best;
            if best != shown && search.clearly_better(best, shown) {
                best
            } else {
                shown
            }
        }
        PlayMode::Fixed(r) => {
            let stop = StopRule { max_rounds: r, ..STOP_DEFAULTS };
            Search::new(s, SolveOptions { seed, stop, known_events: false }, p, w).step(f64::INFINITY, pool).best
        }
    }
}

/// Итог забега самоигры: счёт мгновенной, счёт с решателем, хеш ходов решателя.
pub struct Game {
    pub g: u32,
    pub instant: i32,
    pub solver: i32,
    pub hash: u64,
    /// Итог с красными (`final_value` по правилам игры p).
    pub instant_v: f64,
    pub solver_v: f64,
}

/// Забег g (сид 777000+g, тема g % 5): мгновенной стратегией и с решателем.
/// Игра идёт по p, решатель считает по model (A/B модели правил на тех же сидах).
pub fn play_game(g: u32, mode: PlayMode, p: &Params, pool: Option<&ThreadPool>) -> Game {
    play_game_model(g, mode, p, p, pool)
}

pub fn play_game_model(g: u32, mode: PlayMode, p: &Params, model: &Params, pool: Option<&ThreadPool>) -> Game {
    let w = default_weights();
    let mut inst = Instant::new();
    let mut buf: CodeBuf = [0; 32];
    let mut scores = [0; 2];
    let mut values = [0.0; 2];
    let mut h = StreamHash::new();
    for (k, use_solver) in [false, true].into_iter().enumerate() {
        let mut s = State::new();
        let mut r = Rng::new(777_000 + g);
        start_run(&mut s, &mut r, p, RunOptions { start: (g % 5) as i32, ..RunOptions::default() });
        let mut mv: u32 = 0;
        while s.m[PHASE] != P_END {
            let c = if use_solver && legal_codes(&s, &mut buf) > 1 {
                choose(&s, g.wrapping_mul(1000).wrapping_add(mv), mode, model, pool)
            } else {
                inst.code(&s, p, w)
            };
            if use_solver {
                h.push(c);
            }
            apply_code(&mut s, c, &mut r, p).unwrap_or_else(|e| panic!("забег {g}: {e}"));
            mv += 1;
        }
        scores[k] = s.m[STORAGE];
        values[k] = gorn_prison::rules::final_value(&s, p);
    }
    Game { g, instant: scores[0], solver: scores[1], hash: h.finish(), instant_v: values[0], solver_v: values[1] }
}

/// `gorn-cli play`: самоигра N забегов; threads — забегов параллельно, solver_threads — пул на поиск.
/// model — по чему считает решатель (прежняя модель — варианты rules-check), игра — по нынешним правилам.
pub fn play(from: u32, n: u32, mode: PlayMode, threads: usize, solver_threads: usize, quiet: bool, model: &Params) {
    let p = &DEFAULT_PARAMS;
    let solver_pool =
        (solver_threads > 1).then(|| rayon::ThreadPoolBuilder::new().num_threads(solver_threads).build().unwrap());
    let t0 = Clock::now();
    let games: Vec<Game> = if threads == 1 {
        (from..from + n)
            .map(|g| {
                let r = play_game_model(g, mode, p, model, solver_pool.as_ref());
                if !quiet {
                    println!("{} {:.2} {:.2} {}", r.g, r.instant_v, r.solver_v, r.solver);
                }
                r
            })
            .collect()
    } else {
        let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build().unwrap();
        let mut v: Vec<Game> = pool.install(|| {
            (from..from + n).into_par_iter().with_max_len(1).map(|g| play_game_model(g, mode, p, model, solver_pool.as_ref())).collect()
        });
        v.sort_by_key(|r| r.g);
        if !quiet {
            for r in &v {
                println!("{} {:.2} {:.2} {}", r.g, r.instant_v, r.solver_v, r.solver);
            }
        }
        v
    };
    let sec = t0.elapsed().as_secs_f64();
    // Итог с красными (призмы + red_value за каждого Величайшего на доске).
    let si: f64 = games.iter().map(|r| r.instant_v).sum();
    let ss: f64 = games.iter().map(|r| r.solver_v).sum();
    let d: Vec<f64> = games.iter().map(|r| r.solver_v - r.instant_v).collect();
    let md = d.iter().sum::<f64>() / d.len() as f64;
    let se = (d.iter().map(|x| (x - md) * (x - md)).sum::<f64>() / (d.len() as f64 * (d.len() as f64 - 1.0).max(1.0))).sqrt();
    let mut h = StreamHash::new();
    for r in &games {
        h.push(r.solver);
        h.push((r.hash >> 32) as i32);
        h.push(r.hash as i32);
    }
    println!(
        "# {mode:?}: забегов {n}, мгновенная {:.3}, с проверкой {:.3} (+{md:.3} ± {se:.3}), {sec:.1} с, {:.2} с на забег (потоков {threads}×{solver_threads}), хеш {:016x}",
        si / n as f64,
        ss / n as f64,
        sec / n as f64,
        h.finish()
    );
}

// ── solve-bench ─────────────────────────────────────────────────────────────

fn q(a: &[f64], f: f64) -> f64 {
    let mut b = a.to_vec();
    b.sort_by(|x, y| x.partial_cmp(y).unwrap());
    b[((b.len() as f64 * f) as usize).min(b.len() - 1)]
}

fn avg(a: &[f64]) -> f64 {
    a.iter().sum::<f64>() / a.len() as f64
}

fn fmt_ms(x: f64) -> String {
    if x < 1.0 {
        format!("{x:.3}")
    } else {
        format!("{x:.1}")
    }
}

pub struct BenchOpts {
    pub games: u32,
    pub every: u32,
    pub positions: usize,
    pub min_rounds: u64,
    pub max_rounds: u64,
    pub cap_ms: f64,
    pub threads: usize,
    pub rate: Option<usize>,
    pub regret: Option<RegretOpts>,
}

pub struct RegretOpts {
    pub file: String,
    pub rounds: u64,
    pub threads: usize,
}

pub fn solve_bench(o: &BenchOpts) -> Result<(), String> {
    let p = &DEFAULT_PARAMS;
    let w = default_weights();
    let mut pos = solve_positions(p, w, o.games, o.every);
    pos.truncate(o.positions);
    let pool = (o.threads > 1).then(|| rayon::ThreadPoolBuilder::new().num_threads(o.threads).build().unwrap());
    let pool = pool.as_ref();

    if let Some(stride) = o.rate {
        let mut at300 = vec![];
        let mut at1500 = vec![];
        for i in (0..pos.len()).step_by(stride.max(1)) {
            let mut search = Search::new(&pos[i], SolveOptions { seed: 1000 + i as u32, stop: STOP_FREE, known_events: false }, p, w);
            at300.push(search.step(300.0, pool).rounds as f64);
            at1500.push(search.step(1200.0, pool).rounds as f64);
        }
        println!("потоков {}, позиций {} (каждая {}-я из {})", o.threads, at300.len(), stride, pos.len());
        println!("раундов за 300 мс: медиана {}, среднее {:.0}, p10 {}", q(&at300, 0.5), avg(&at300), q(&at300, 0.1));
        println!("раундов за 1,5 с: медиана {}, среднее {:.0}, p10 {}", q(&at1500, 0.5), avg(&at1500), q(&at1500, 0.1));
        return Ok(());
    }

    if let Some(ro) = &o.regret {
        let refm = regret_ref(ro, &pos, o.games, o.every, p)?;
        let regret_of = |i: usize, code: i32| -> f64 {
            let m = &refm[i];
            let best = m.values().cloned().fold(f64::NEG_INFINITY, f64::max);
            best - m.get(&code).copied().unwrap_or(f64::NAN)
        };
        let mut inst = Instant::new();
        let (mut first, mut reg, mut reg_i) = (vec![], vec![], vec![]);
        let mut by_ph: Vec<(String, Vec<f64>)> = vec![];
        for (i, s) in pos.iter().enumerate() {
            let (code, ms) = advise_like(s, 1000 + i as u32, p, pool);
            first.push(ms);
            let r = regret_of(i, code);
            reg.push(r);
            reg_i.push(regret_of(i, inst.code(s, p, w)));
            let ph = if tool_pick(s) { "выбор орудий".to_string() } else { PHASE_NAMES[s.m[PHASE] as usize].to_string() };
            match by_ph.iter_mut().find(|(k, _)| *k == ph) {
                Some((_, v)) => v.push(r),
                None => by_ph.push((ph, vec![r])),
            }
        }
        println!(
            "потоков {}, позиций {} (забегов {}, каждое {}-е), эталон {} раундов",
            o.threads,
            pos.len(),
            o.games,
            o.every,
            ro.rounds
        );
        println!("regret решателя {:.4}, мгновенной {:.4}", avg(&reg), avg(&reg_i));
        for (ph, a) in &by_ph {
            println!("  {ph}: {:.4} ({})", avg(a), a.len());
        }
        println!("первый совет: медиана {} мс, p95 {} мс", fmt_ms(q(&first, 0.5)), fmt_ms(q(&first, 0.95)));
        return Ok(());
    }

    let mut inst = Instant::new();
    let (mut instant_ms, mut solve_ms, mut rounds) = (vec![], vec![], vec![]);
    let (mut changed, mut capped) = (0, 0);
    let mut by_phase: Vec<(&str, u32, u32)> = vec![];
    for (i, s) in pos.iter().enumerate() {
        let t0 = Clock::now();
        let ic = inst.code(s, p, w);
        instant_ms.push(t0.elapsed().as_secs_f64() * 1000.0);
        let t0 = Clock::now();
        let stop = StopRule { min_rounds: o.min_rounds, max_rounds: o.max_rounds, ..STOP_DEFAULTS };
        let res = Search::new(s, SolveOptions { seed: 1000 + i as u32, stop, known_events: false }, p, w).step(o.cap_ms, pool);
        solve_ms.push(t0.elapsed().as_secs_f64() * 1000.0);
        rounds.push(res.rounds as f64);
        if res.rounds >= o.max_rounds {
            capped += 1;
        }
        let ph = PHASE_NAMES[s.m[PHASE] as usize];
        let idx = match by_phase.iter().position(|(k, _, _)| *k == ph) {
            Some(i) => i,
            None => {
                by_phase.push((ph, 0, 0));
                by_phase.len() - 1
            }
        };
        by_phase[idx].1 += 1;
        if res.best != ic {
            changed += 1;
            by_phase[idx].2 += 1;
        }
    }
    let n = pos.len();
    let maxs = if o.max_rounds == u64::MAX { "∞".to_string() } else { o.max_rounds.to_string() };
    println!("позиций: {n} (забегов {}), раунды {}…{maxs}, потоков {}", o.games, o.min_rounds, o.threads);
    println!(
        "мгновенный совет: медиана {} мс, p95 {} мс, max {} мс",
        fmt_ms(q(&instant_ms, 0.5)),
        fmt_ms(q(&instant_ms, 0.95)),
        fmt_ms(q(&instant_ms, 1.0))
    );
    println!(
        "проверка вариантами до остановки: медиана {} мс, p95 {} мс, max {} мс",
        fmt_ms(q(&solve_ms, 0.5)),
        fmt_ms(q(&solve_ms, 0.95)),
        fmt_ms(q(&solve_ms, 1.0))
    );
    println!(
        "раундов: медиана {}, p95 {}; упёрлись в потолок: {capped} ({:.1} %)",
        q(&rounds, 0.5),
        q(&rounds, 0.95),
        100.0 * capped as f64 / n as f64
    );
    println!("проверка сменила мгновенный совет: {changed} из {n} ({:.1} %)", 100.0 * changed as f64 / n as f64);
    for (ph, k, c) in by_phase {
        println!("  {ph}: {c} из {k}");
    }
    Ok(())
}

/// Эталон regret: средние ходов на ref-rounds раундах (зерно 900000+i). Файл —
/// в формате prison-solve-bench (`{rounds, games, every, means: [{код: среднее}]}`),
/// его можно брать и из gorn-tli: оценки решателей совпадают до бита.
fn regret_ref(ro: &RegretOpts, pos: &[State], games: u32, every: u32, p: &Params) -> Result<Vec<HashMap<i32, f64>>, String> {
    if let Ok(text) = std::fs::read_to_string(&ro.file) {
        let j: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", ro.file))?;
        let ok = j["rounds"].as_u64() == Some(ro.rounds)
            && j["games"].as_u64() == Some(games as u64)
            && j["every"].as_u64() == Some(every as u64)
            && j["means"].as_array().map(|a| a.len()) == Some(pos.len());
        if !ok {
            return Err(format!("эталон {} — для других позиций или раундов", ro.file));
        }
        let means = j["means"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| {
                m.as_object()
                    .unwrap()
                    .iter()
                    .map(|(k, v)| (k.parse::<i32>().unwrap(), v.as_f64().unwrap()))
                    .collect::<HashMap<_, _>>()
            })
            .collect();
        println!("эталон regret: {}", ro.file);
        return Ok(means);
    }
    let pool = rayon::ThreadPoolBuilder::new().num_threads(ro.threads.max(1)).build().unwrap();
    let w = default_weights();
    let t0 = Clock::now();
    let stop = StopRule { min_rounds: ro.rounds, max_rounds: ro.rounds, z_stop: f64::INFINITY, eps: -1.0, ..STOP_DEFAULTS };
    let mut means = vec![];
    for (i, s) in pos.iter().enumerate() {
        let res = Search::new(s, SolveOptions { seed: 900_000 + i as u32, stop, known_events: false }, p, w)
            .step(f64::INFINITY, Some(&pool));
        means.push(res.options.iter().map(|o| (o.code, o.mean)).collect::<HashMap<_, _>>());
        if i % 50 == 49 {
            println!("эталон: {} из {}, {:.0} с", i + 1, pos.len(), t0.elapsed().as_secs_f64());
        }
    }
    let mut out = serde_json::Map::new();
    out.insert("rounds".into(), ro.rounds.into());
    out.insert("games".into(), games.into());
    out.insert("every".into(), every.into());
    out.insert(
        "means".into(),
        serde_json::Value::Array(
            means
                .iter()
                .map(|m| {
                    let mut o = serde_json::Map::new();
                    let mut keys: Vec<_> = m.keys().collect();
                    keys.sort();
                    for k in keys {
                        o.insert(k.to_string(), serde_json::json!(m[k]));
                    }
                    serde_json::Value::Object(o)
                })
                .collect(),
        ),
    );
    std::fs::write(&ro.file, serde_json::to_string(&serde_json::Value::Object(out)).unwrap())
        .map_err(|e| format!("{}: {e}", ro.file))?;
    Ok(means)
}
