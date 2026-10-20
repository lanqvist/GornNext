//! `gorn-cli verify`: сверка с эталоном на TS.
//!
//! 1. Веса мгновенной стратегии — до бита.
//! 2. ГСЧ — первые числа и все виды бросков для набора сидов.
//! 3. Забеги (мгновенная стратегия и случайные ходы) — потоковый хеш
//!    (код хода и состояние после каждого хода) и итог на всех сидах.
//! 4. Те же забеги целиком — состояние после каждого хода, поле в поле.
//!
//! Эталон считается в нескольких процессах node параллельно, Rust — в потоке
//! чтения каждого из них.

use std::io::{BufRead, BufReader, Read};
use std::time::Instant as Clock;

use gorn_prison::actions::describe;
use gorn_prison::policy::default_weights;
use gorn_prison::rng::Rng;
use gorn_prison::runs::{Mode, Runner};
use gorn_prison::state::{field_name, State, SIZE};
use gorn_prison::DEFAULT_PARAMS;

use crate::reference::Reference;

pub struct Options {
    pub tli: Option<String>,
    pub seeds: u32,
    pub random: u32,
    pub full: u32,
    pub full_random: u32,
    pub rng: u32,
    pub jobs: u32,
    pub from: u32,
    /// Позиций решателя (из 580 аудита) и потолок раундов.
    pub solve: u32,
    pub solve_rounds: u32,
    /// Потоков пула для проверки «пул = один поток» (0 — не проверять).
    pub solve_pool: u32,
    /// Забегов самоигры с решателем без срока и потолок раундов.
    pub play: u32,
    pub play_rounds: u32,
    /// Забегов настройки (prison-tune) на каждый вид стратегии.
    pub tune: u32,
    /// Кадры распознавания: папка («data» — папка данных тестов gorn-tli), None — не сверять.
    pub frames: Option<String>,
    pub frames_max: u32,
}

/// Расхождений показываем не больше стольких на проверку.
const SHOW: usize = 5;

pub fn run(o: &Options) -> Result<bool, String> {
    let rf = Reference::locate(o.tli.as_deref())?;
    println!("эталон: {}", rf.tli.display());
    let mut ok = true;
    ok &= check_weights(&rf, "default")?;
    ok &= check_weights(&rf, "hand")?;
    if o.rng > 0 {
        ok &= check_rng(&rf, o.from, o.rng, o.jobs)?;
    }
    for (mode, n) in [(Mode::Instant, o.seeds), (Mode::Random, o.random)] {
        if n > 0 {
            ok &= check_hash(&rf, mode, o.from, n, o.jobs)?;
        }
    }
    for (mode, n) in [(Mode::Instant, o.full), (Mode::Random, o.full_random)] {
        if n > 0 {
            ok &= check_full(&rf, mode, o.from, n, o.jobs)?;
        }
    }
    if let Some(dir) = &o.frames {
        let dir = if dir == "data" { crate::frames::data_dir().join("images") } else { std::path::PathBuf::from(dir) };
        let mut list = crate::frames::pngs(&dir);
        list.truncate(o.frames_max as usize);
        for mode in ["fresh", "chain", "chainre"] {
            ok &= check_frames(&rf, mode, &list, o.jobs)?;
        }
    }
    if o.tune > 0 {
        ok &= check_tune(&rf, o.tune)?;
    }
    if o.play > 0 {
        ok &= check_play(&rf, o.play, o.play_rounds, o.jobs)?;
    }
    if o.solve > 0 {
        for rule in ["default", "free"] {
            ok &= check_solve(&rf, rule, o.solve, o.solve_rounds, o.solve_pool, o.jobs)?;
        }
    }
    println!("{}", if ok { "СВЕРКА: совпадает" } else { "СВЕРКА: ЕСТЬ РАСХОЖДЕНИЯ" });
    Ok(ok)
}

fn shards(from: u32, n: u32, jobs: u32) -> Vec<(u32, u32)> {
    let jobs = jobs.clamp(1, n.max(1));
    let per = n.div_ceil(jobs);
    (0..jobs).map(|j| (from + j * per, per.min(n.saturating_sub(j * per)))).filter(|&(_, k)| k > 0).collect()
}

fn report(what: &str, n: u64, bad: u64, shown: &[String], t: Clock) -> bool {
    for s in shown.iter().take(SHOW) {
        println!("  ✗ {s}");
    }
    println!(
        "{what}: {n} — {} ({:.1} с)",
        if bad == 0 { "без расхождений".to_string() } else { format!("РАСХОЖДЕНИЙ {bad}") },
        t.elapsed().as_secs_f64()
    );
    bad == 0
}

// ── веса ────────────────────────────────────────────────────────────────────

fn check_weights(rf: &Reference, which: &str) -> Result<bool, String> {
    let t = Clock::now();
    let out = String::from_utf8(rf.output(&["weights".into(), which.into()])?).map_err(|e| e.to_string())?;
    let hand = gorn_prison::policy::weights::hand_weights();
    let w = if which == "hand" { &hand } else { default_weights() };
    let mut bad = 0;
    let mut shown = vec![];
    let mut n = 0;
    for line in out.lines() {
        let mut it = line.split_whitespace();
        let i: usize = it.next().and_then(|x| x.parse().ok()).ok_or("веса: строка")?;
        let bits = u64::from_str_radix(it.next().ok_or("веса: биты")?, 16).map_err(|e| e.to_string())?;
        n += 1;
        if i >= w.len() || w[i].to_bits() != bits {
            bad += 1;
            shown.push(format!("вес {i}: TS {} Rust {:?}", f64::from_bits(bits), w.get(i)));
        }
    }
    if n != w.len() {
        bad += 1;
        shown.push(format!("весов: TS {n}, Rust {}", w.len()));
    }
    Ok(report(&format!("веса ({which})"), n as u64, bad, &shown, t))
}

// ── ГСЧ ─────────────────────────────────────────────────────────────────────

#[derive(Debug)]
enum Tok {
    Int(i64),
    Float(f64),
}

const RNG_COUNT: usize = 16;

/// То же, что rng() в reference.mts.
fn rng_tokens(g: u32) -> Vec<Tok> {
    let mut out = vec![Tok::Int(g as i64)];
    let mut r = Rng::new(g);
    for _ in 0..RNG_COUNT {
        out.push(Tok::Int(r.next() as i64));
    }
    let mut r = Rng::new(g);
    for _ in 0..8 {
        out.push(Tok::Int(r.bits30() as i64));
    }
    for _ in 0..4 {
        out.push(Tok::Float(r.float()));
    }
    for k in [1, 2, 3, 4, 5, 16, 31, 1000, 0x4000_0000] {
        out.push(Tok::Int(r.int(k) as i64));
    }
    for q in [0.0, 0.25, 0.35, 0.5, 0.8, 1.0] {
        out.push(Tok::Int(r.chance(q) as i64));
    }
    out.push(Tok::Int(r.s() as i64));
    for label in [gorn_prison::rng::Label::Str("doors"), "событие".into(), (g & 0xffff).into()] {
        let mut t = r.stream(label);
        out.push(Tok::Int(t.next() as i64));
        out.push(Tok::Int(t.next() as i64));
    }
    out
}

fn same_tok(a: &str, b: &Tok) -> bool {
    match b {
        Tok::Int(x) => a.parse::<i64>().ok() == Some(*x),
        Tok::Float(x) => a.parse::<f64>().map(|y| y.to_bits() == x.to_bits()).unwrap_or(false),
    }
}

fn check_rng(rf: &Reference, from: u32, n: u32, jobs: u32) -> Result<bool, String> {
    let t = Clock::now();
    let parts = shards(from, n, jobs);
    let results: Vec<Result<(u64, u64, Vec<String>), String>> = std::thread::scope(|sc| {
        let hs: Vec<_> = parts
            .iter()
            .map(|&(a, k)| {
                sc.spawn(move || -> Result<(u64, u64, Vec<String>), String> {
                    let mut child = rf.spawn(&["rng".into(), a.to_string(), k.to_string(), RNG_COUNT.to_string()])?;
                    let rd = BufReader::new(child.stdout.take().unwrap());
                    let (mut cnt, mut bad, mut shown) = (0u64, 0u64, vec![]);
                    for (i, line) in rd.lines().enumerate() {
                        let line = line.map_err(|e| e.to_string())?;
                        let g = a + i as u32;
                        let ts: Vec<&str> = line.split_whitespace().collect();
                        let rs = rng_tokens(g);
                        cnt += 1;
                        if ts.len() != rs.len() || ts.iter().zip(&rs).any(|(x, y)| !same_tok(x, y)) {
                            bad += 1;
                            if shown.len() < SHOW {
                                shown.push(format!("ГСЧ сид {g}: TS [{line}] Rust {rs:?}"));
                            }
                        }
                    }
                    child.wait().map_err(|e| e.to_string())?;
                    if cnt != k as u64 {
                        return Err(format!("ГСЧ: эталон дал {cnt} строк из {k}"));
                    }
                    Ok((cnt, bad, shown))
                })
            })
            .collect();
        hs.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let (mut cnt, mut bad, mut shown) = (0, 0, vec![]);
    for r in results {
        let (c, b, s) = r?;
        cnt += c;
        bad += b;
        shown.extend(s);
    }
    Ok(report("ГСЧ, сидов", cnt, bad, &shown, t))
}

// ── забеги по хешу ──────────────────────────────────────────────────────────

fn check_hash(rf: &Reference, mode: Mode, from: u32, n: u32, jobs: u32) -> Result<bool, String> {
    let t = Clock::now();
    let parts = shards(from, n, jobs);
    let p = &DEFAULT_PARAMS;
    let w = default_weights();
    type Part = (u64, u64, Vec<String>, i64, u64);
    let results: Vec<Result<Part, String>> = std::thread::scope(|sc| {
        let hs: Vec<_> = parts
            .iter()
            .map(|&(a, k)| {
                sc.spawn(move || -> Result<Part, String> {
                    let mut child = rf.spawn(&[
                        "runs".into(),
                        mode.name().into(),
                        a.to_string(),
                        k.to_string(),
                        "hash".into(),
                    ])?;
                    let rd = BufReader::new(child.stdout.take().unwrap());
                    let mut runner = Runner::new(p, w);
                    let (mut cnt, mut bad, mut shown, mut score, mut steps) = (0u64, 0u64, vec![], 0i64, 0u64);
                    for line in rd.lines() {
                        let line = line.map_err(|e| e.to_string())?;
                        let f: Vec<u64> = line.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                        if f.len() != 5 {
                            return Err(format!("{}: строка эталона «{line}»", mode.name()));
                        }
                        let g = f[0] as u32;
                        let s = runner.run_hash(mode, g);
                        cnt += 1;
                        score += s.score as i64;
                        steps += s.steps as u64;
                        let ts = (f[1] as u32, f[2] as i32, f[3] as u32, f[4] as u32);
                        if ts != (s.steps, s.score, s.h1, s.h2) {
                            bad += 1;
                            if shown.len() < SHOW {
                                shown.push(format!(
                                    "{} сид {g}: TS ходов {} счёт {} хеш {:08x}{:08x}; Rust ходов {} счёт {} хеш {:08x}{:08x}",
                                    mode.name(),
                                    ts.0,
                                    ts.1,
                                    ts.2,
                                    ts.3,
                                    s.steps,
                                    s.score,
                                    s.h1,
                                    s.h2
                                ));
                            }
                        }
                    }
                    child.wait().map_err(|e| e.to_string())?;
                    if cnt != k as u64 {
                        return Err(format!("{}: эталон дал {cnt} забегов из {k}", mode.name()));
                    }
                    Ok((cnt, bad, shown, score, steps))
                })
            })
            .collect();
        hs.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let (mut cnt, mut bad, mut shown, mut score, mut steps) = (0, 0, vec![], 0i64, 0u64);
    for r in results {
        let (c, b, s, sc, st) = r?;
        cnt += c;
        bad += b;
        shown.extend(s);
        score += sc;
        steps += st;
    }
    println!(
        "  {}: {:.4} призм. в среднем, {:.1} записей на забег",
        mode.name(),
        score as f64 / cnt.max(1) as f64,
        steps as f64 / cnt.max(1) as f64
    );
    Ok(report(&format!("забеги {} по хешу, сидов", mode.name()), cnt, bad, &shown, t))
}

// ── забеги целиком ──────────────────────────────────────────────────────────

fn read_i32s(rd: &mut impl Read, out: &mut [i32]) -> std::io::Result<bool> {
    let mut b = vec![0u8; out.len() * 4];
    let mut got = 0;
    while got < b.len() {
        let k = rd.read(&mut b[got..])?;
        if k == 0 {
            if got == 0 {
                return Ok(false);
            }
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "обрыв записи"));
        }
        got += k;
    }
    for (i, v) in out.iter_mut().enumerate() {
        *v = i32::from_le_bytes([b[4 * i], b[4 * i + 1], b[4 * i + 2], b[4 * i + 3]]);
    }
    Ok(true)
}

fn diff_fields(a: &[i32], b: &[i32]) -> String {
    let d: Vec<String> = (0..SIZE)
        .filter(|&i| a[i] != b[i])
        .take(12)
        .map(|i| format!("{} TS {} Rust {}", field_name(i), a[i], b[i]))
        .collect();
    d.join(", ")
}

fn check_full(rf: &Reference, mode: Mode, from: u32, n: u32, jobs: u32) -> Result<bool, String> {
    let t = Clock::now();
    let parts = shards(from, n, jobs);
    let p = &DEFAULT_PARAMS;
    let w = default_weights();
    type Part = (u64, u64, u64, Vec<String>);
    let results: Vec<Result<Part, String>> = std::thread::scope(|sc| {
        let hs: Vec<_> = parts
            .iter()
            .map(|&(a, k)| {
                sc.spawn(move || -> Result<Part, String> {
                    let mut child = rf.spawn(&[
                        "runs".into(),
                        mode.name().into(),
                        a.to_string(),
                        k.to_string(),
                        "full".into(),
                    ])?;
                    let mut rd = BufReader::with_capacity(1 << 20, child.stdout.take().unwrap());
                    let mut runner = Runner::new(p, w);
                    let (mut cnt, mut recs, mut bad, mut shown) = (0u64, 0u64, 0u64, vec![]);
                    let mut head = [0i32; 2];
                    let mut rec = vec![0i32; 1 + SIZE];
                    let mut mine: Vec<(i32, State)> = Vec::with_capacity(512);
                    while read_i32s(&mut rd, &mut head).map_err(|e| e.to_string())? {
                        let (g, kk) = (head[0] as u32, head[1] as usize);
                        mine.clear();
                        runner.run_with(mode, g, |c, s| mine.push((c, *s)));
                        cnt += 1;
                        let mut first: Option<String> = None;
                        for i in 0..kk {
                            if !read_i32s(&mut rd, &mut rec).map_err(|e| e.to_string())? {
                                return Err("обрыв потока эталона".into());
                            }
                            recs += 1;
                            if first.is_some() {
                                continue;
                            }
                            match mine.get(i) {
                                None => first = Some(format!("запись {i}: у Rust забег уже кончился")),
                                Some((c, s)) => {
                                    if *c != rec[0] || s.m[..] != rec[1..] {
                                        first = Some(format!(
                                            "запись {i}: ход TS {} Rust {}; {}",
                                            describe(rec[0]),
                                            describe(*c),
                                            diff_fields(&rec[1..], &s.m)
                                        ));
                                    }
                                }
                            }
                        }
                        if first.is_none() && mine.len() != kk {
                            first = Some(format!("записей TS {kk}, Rust {}", mine.len()));
                        }
                        if let Some(f) = first {
                            bad += 1;
                            if shown.len() < SHOW {
                                shown.push(format!("{} сид {g}: {f}", mode.name()));
                            }
                        }
                    }
                    child.wait().map_err(|e| e.to_string())?;
                    if cnt != k as u64 {
                        return Err(format!("{}: эталон дал {cnt} забегов из {k}", mode.name()));
                    }
                    Ok((cnt, recs, bad, shown))
                })
            })
            .collect();
        hs.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let (mut cnt, mut recs, mut bad, mut shown) = (0, 0, 0, vec![]);
    for r in results {
        let (c, k, b, s) = r?;
        cnt += c;
        recs += k;
        bad += b;
        shown.extend(s);
    }
    println!("  {}: сверено состояний {recs}", mode.name());
    Ok(report(&format!("забеги {} целиком, сидов", mode.name()), cnt, bad, &shown, t))
}

// ── решатель ────────────────────────────────────────────────────────────────

fn hex(x: f64) -> String {
    format!("{:x}", x.to_bits())
}

/// Строка как у `solve` в reference.mts.
fn solve_line(i: usize, r: &gorn_prison::solver::SolveStep) -> String {
    let opts: Vec<String> = r
        .options
        .iter()
        .map(|o| format!("{}:{}:{}:{}:{}", o.code, hex(o.mean), hex(o.gap_se), o.rounds, o.pruned as i32))
        .collect();
    format!("{i} {} {} {} {} {}", r.rounds, r.done as i32, r.best, opts.len(), opts.join(" "))
}

/// Решатель при фиксированном числе раундов: те же оценки, что у эталона,
/// и в пуле rayon — те же, что в одном потоке.
fn check_solve(rf: &Reference, rule: &str, n: u32, rounds: u32, pool_threads: u32, jobs: u32) -> Result<bool, String> {
    use gorn_prison::solver::{Search, SolveOptions, STOP_DEFAULTS, STOP_FREE};
    let t = Clock::now();
    let p = &DEFAULT_PARAMS;
    let w = default_weights();
    let pos = gorn_prison::runs::solve_positions(p, w, 18, 2);
    let n = n.min(pos.len() as u32);
    let stop = if rule == "free" { STOP_FREE } else { STOP_DEFAULTS };
    let stop = gorn_prison::solver::StopRule { max_rounds: rounds as u64, ..stop };
    let pool = if pool_threads > 1 {
        Some(rayon::ThreadPoolBuilder::new().num_threads(pool_threads as usize).build().map_err(|e| e.to_string())?)
    } else {
        None
    };
    let parts = shards(0, n, jobs);
    type Part = (u64, u64, Vec<String>);
    let results: Vec<Result<Part, String>> = std::thread::scope(|sc| {
        let hs: Vec<_> = parts
            .iter()
            .map(|&(a, k)| {
                let (pos, pool) = (&pos, pool.as_ref());
                sc.spawn(move || -> Result<Part, String> {
                    let mut child = rf.spawn(&[
                        "solve".into(),
                        rule.into(),
                        a.to_string(),
                        k.to_string(),
                        rounds.to_string(),
                    ])?;
                    let rd = BufReader::new(child.stdout.take().unwrap());
                    let (mut cnt, mut bad, mut shown) = (0u64, 0u64, vec![]);
                    for line in rd.lines() {
                        let line = line.map_err(|e| e.to_string())?;
                        let i: usize = line.split_whitespace().next().and_then(|x| x.parse().ok()).ok_or("solve: строка")?;
                        let opts = SolveOptions { seed: 1000 + i as u32, stop, known_events: false };
                        let one = Search::new(&pos[i], opts, p, w).step(f64::INFINITY, None);
                        let par = match pool {
                            Some(pool) => Search::new(&pos[i], opts, p, w).step(f64::INFINITY, Some(pool)),
                            None => one.clone(),
                        };
                        let mine = solve_line(i, &one);
                        cnt += 1;
                        if mine != line || one != par {
                            bad += 1;
                            if shown.len() < SHOW {
                                shown.push(if mine != line {
                                    format!("решатель {rule}, позиция {i}:
    TS   {line}
    Rust {mine}")
                                } else {
                                    format!("решатель {rule}, позиция {i}: пул ≠ один поток
    {}", solve_line(i, &par))
                                });
                            }
                        }
                    }
                    child.wait().map_err(|e| e.to_string())?;
                    if cnt != k as u64 {
                        return Err(format!("решатель: эталон дал {cnt} позиций из {k}"));
                    }
                    Ok((cnt, bad, shown))
                })
            })
            .collect();
        hs.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let (mut cnt, mut bad, mut shown) = (0, 0, vec![]);
    for r in results {
        let (c, b, s) = r?;
        cnt += c;
        bad += b;
        shown.extend(s);
    }
    Ok(report(&format!("решатель ({rule}, {rounds} раундов), позиций"), cnt, bad, &shown, t))
}

/// Самоигра с решателем без срока: те же ходы и счёт, что у эталона.
fn check_play(rf: &Reference, n: u32, rounds: u32, jobs: u32) -> Result<bool, String> {
    use crate::solverbench::{play_game, PlayMode};
    let t = Clock::now();
    let p = &DEFAULT_PARAMS;
    let parts = shards(0, n, jobs);
    type Part = (u64, u64, Vec<String>);
    let results: Vec<Result<Part, String>> = std::thread::scope(|sc| {
        let hs: Vec<_> = parts
            .iter()
            .map(|&(a, k)| {
                sc.spawn(move || -> Result<Part, String> {
                    let mut child = rf.spawn(&["play".into(), a.to_string(), k.to_string(), rounds.to_string()])?;
                    let rd = BufReader::new(child.stdout.take().unwrap());
                    let (mut cnt, mut bad, mut shown) = (0u64, 0u64, vec![]);
                    for line in rd.lines() {
                        let line = line.map_err(|e| e.to_string())?;
                        let f: Vec<u64> = line.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                        if f.len() != 5 {
                            return Err(format!("play: строка эталона «{line}»"));
                        }
                        let g = play_game(f[0] as u32, PlayMode::Fixed(rounds as u64), p, None);
                        let mine = (g.instant as u64, g.solver as u64, g.hash >> 32, g.hash & 0xffff_ffff);
                        cnt += 1;
                        if mine != (f[1], f[2], f[3], f[4]) {
                            bad += 1;
                            if shown.len() < SHOW {
                                shown.push(format!("самоигра {}: TS «{line}», Rust {mine:?}", f[0]));
                            }
                        }
                    }
                    child.wait().map_err(|e| e.to_string())?;
                    if cnt != k as u64 {
                        return Err(format!("play: эталон дал {cnt} забегов из {k}"));
                    }
                    Ok((cnt, bad, shown))
                })
            })
            .collect();
        hs.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let (mut cnt, mut bad, mut shown) = (0, 0, vec![]);
    for r in results {
        let (c, b, s) = r?;
        cnt += c;
        bad += b;
        shown.extend(s);
    }
    Ok(report(&format!("самоигра с решателем ({rounds} раундов), забегов"), cnt, bad, &shown, t))
}

/// Забеги настройки (сценарий «все темы по кругу»): счёт каждого — как у эталона.
fn check_tune(rf: &Reference, n: u32) -> Result<bool, String> {
    use crate::tune::{scores, Kind, MIX};
    use gorn_prison::policy::weights::hand_weights;
    let t = Clock::now();
    let seed = 0x51ed;
    let (mut bad, mut shown, mut cnt) = (0u64, vec![], 0u64);
    for (name, kind) in [("random", Kind::Random), ("greedy", Kind::Greedy), ("policy", Kind::Policy)] {
        let out = String::from_utf8(rf.output(&["scores".into(), name.into(), seed.to_string(), n.to_string()])?)
            .map_err(|e| e.to_string())?;
        let w = if kind == Kind::Policy { *default_weights() } else { hand_weights() };
        // один поток: короткая сверка не должна занимать все ядра
        let pool = rayon::ThreadPoolBuilder::new().num_threads(1).build().map_err(|e| e.to_string())?;
        let xs = pool.install(|| scores(kind, &w, &MIX, seed, n));
        let mut h = gorn_prison::StreamHash::new();
        for &x in &xs {
            h.push(x as i32);
        }
        let mine = format!("{n} {} {} {}", xs.iter().sum::<f64>() / n as f64, h.h1, h.h2);
        let ts = out.trim();
        cnt += n as u64;
        // среднее печатают по-разному (JS и Rust); сверяем число и хеш
        let a: Vec<&str> = ts.split_whitespace().collect();
        let b: Vec<&str> = mine.split_whitespace().collect();
        let same = a.len() == 4
            && a[0] == b[0]
            && a[2] == b[2]
            && a[3] == b[3]
            && a[1].parse::<f64>().ok().map(f64::to_bits) == b[1].parse::<f64>().ok().map(f64::to_bits);
        if !same {
            bad += 1;
            shown.push(format!("настройка {name}: TS «{ts}», Rust «{mine}»"));
        }
    }
    Ok(report("забеги настройки (random, greedy, policy), забегов", cnt, bad, &shown, t))
}

// ── распознавание ───────────────────────────────────────────────────────────

/// Сравнить JSON: числа — по значению f64 до бита, порядок ключей не важен.
fn json_diff(a: &serde_json::Value, b: &serde_json::Value, path: &str, out: &mut Vec<String>) {
    use serde_json::Value::*;
    if out.len() >= 6 {
        return;
    }
    match (a, b) {
        (Number(x), Number(y)) => {
            if x.as_f64().map(f64::to_bits) != y.as_f64().map(f64::to_bits) {
                out.push(format!("{path}: TS {x} Rust {y}"));
            }
        }
        (Object(x), Object(y)) => {
            for (k, v) in x {
                match y.get(k) {
                    Some(w) => json_diff(v, w, &format!("{path}.{k}"), out),
                    None => out.push(format!("{path}.{k}: у Rust нет (TS {v})")),
                }
            }
            for (k, w) in y {
                if !x.contains_key(k) {
                    out.push(format!("{path}.{k}: у TS нет (Rust {w})"));
                }
            }
        }
        (Array(x), Array(y)) => {
            if x.len() != y.len() {
                out.push(format!("{path}: длина TS {} Rust {}", x.len(), y.len()));
                return;
            }
            for (i, (v, w)) in x.iter().zip(y).enumerate() {
                json_diff(v, w, &format!("{path}[{i}]"), out);
            }
        }
        _ => {
            if a != b {
                out.push(format!("{path}: TS {a} Rust {b}"));
            }
        }
    }
}

fn geo_json(g: &Option<gorn_prison::recognize::Geometry>) -> serde_json::Value {
    match g {
        None => serde_json::Value::Null,
        Some(g) => serde_json::json!({
            "width": g.width, "height": g.height, "s": g.s, "tx": g.tx, "ty": g.ty, "score": g.score,
        }),
    }
}

/// Кадры: тот же вид экрана и та же геометрия, что у эталона.
fn check_frames(rf: &Reference, mode: &str, list: &[std::path::PathBuf], jobs: u32) -> Result<bool, String> {
    use gorn_prison::recognize::{read, Geometry, ReadOptions};
    let t = Clock::now();
    let n = list.len() as u32;
    // chain — геометрия от кадра к кадру: каждый процесс берёт свой отрезок списка подряд.
    let parts = shards(0, n, jobs);
    let tmp = std::env::temp_dir();
    type Part = (u64, u64, Vec<String>);
    let results: Vec<Result<Part, String>> = std::thread::scope(|sc| {
        let hs: Vec<_> = parts
            .iter()
            .enumerate()
            .map(|(j, &(a, k))| {
                let part = &list[a as usize..(a + k) as usize];
                let listfile = tmp.join(format!("gorn-verify-frames-{}-{j}.txt", std::process::id()));
                sc.spawn(move || -> Result<Part, String> {
                    let text: String = part.iter().map(|p| format!("{}\n", p.display())).collect();
                    std::fs::write(&listfile, text).map_err(|e| e.to_string())?;
                    let mut child = rf.spawn(&["read".into(), mode.into(), listfile.to_string_lossy().to_string()])?;
                    let rd = BufReader::new(child.stdout.take().unwrap());
                    let (mut cnt, mut bad, mut shown) = (0u64, 0u64, vec![]);
                    let mut geo: Option<Geometry> = None;
                    for (i, line) in rd.lines().enumerate() {
                        let line = line.map_err(|e| e.to_string())?;
                        let ts: serde_json::Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
                        let path = &part[i];
                        let img = crate::frames::decode_png(path)?;
                        let prev = if mode == "fresh" { None } else { geo };
                        let (view, g) = read(&img.frame(), prev.as_ref(), ReadOptions { recalibrate: mode == "chainre" });
                        geo = g;
                        let mine = serde_json::json!({ "view": view.to_json(), "geo": geo_json(&g) });
                        let mut d = vec![];
                        json_diff(&ts["view"], &mine["view"], "view", &mut d);
                        json_diff(&ts["geo"], &mine["geo"], "geo", &mut d);
                        cnt += 1;
                        if !d.is_empty() {
                            bad += 1;
                            if shown.len() < SHOW {
                                shown.push(format!("кадр {} ({mode}):\n    {}", path.display(), d.join("\n    ")));
                            }
                        }
                    }
                    child.wait().map_err(|e| e.to_string())?;
                    let _ = std::fs::remove_file(&listfile);
                    if cnt != k as u64 {
                        return Err(format!("кадры: эталон дал {cnt} из {k}"));
                    }
                    Ok((cnt, bad, shown))
                })
            })
            .collect();
        hs.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let (mut cnt, mut bad, mut shown) = (0, 0, vec![]);
    for r in results {
        let (c, b, s) = r?;
        cnt += c;
        bad += b;
        shown.extend(s);
    }
    Ok(report(&format!("распознавание ({mode}), кадров"), cnt, bad, &shown, t))
}
