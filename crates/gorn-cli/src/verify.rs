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
    /// Повтор кадров советником: папка дня или папка дней («data» — runs папки данных).
    pub replay: Option<String>,
    pub replay_rounds: u32,
    pub replay_from: String,
    pub replay_to: String,
    /// Ядро аудита: папка дней кадров («data» — runs папки данных) и сколько забегов на день.
    pub audit: Option<String>,
    pub audit_runs: u32,
    /// Логи игры для сверки трекера: папка или файл («game» — папка логов Steam).
    pub logs: Option<String>,
    /// Синтетических логов трекера.
    pub synth: u32,
    /// Ретро-скан и аналитика трекера: папка логов («game» — папка логов Steam).
    pub report: Option<String>,
    /// Синтетических папок логов для ретро-скана и аналитики.
    pub report_synth: u32,
    /// Хранилище: папка логов для сценария записи («game» — папка логов Steam).
    pub db: Option<String>,
    /// Синтетических папок (и старых баз для миграций) для хранилища.
    pub db_synth: u32,
    /// Готовая база (например, gorn.db трекера): читается с копий, оригинал не трогается.
    pub db_file: Option<String>,
    /// Сценариев дочитывания лога.
    pub tail: u32,
    /// Цены сообщества: 0 — нет, 1 — кэши на диске, 2 — ещё и обновление из сети.
    pub community: u32,
    /// Сценариев движка трекера.
    pub engine: u32,
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
    if let Some(dir) = &o.replay {
        let root = if dir == "data" { crate::frames::data_dir().join("runs") } else { std::path::PathBuf::from(dir) };
        // Папка дней — каждый день отдельно; папка дня — она сама.
        let mut days: Vec<std::path::PathBuf> =
            std::fs::read_dir(&root).map_err(|e| format!("{}: {e}", root.display()))?.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
        days.sort();
        if days.is_empty() {
            days.push(root);
        }
        ok &= check_replay(&rf, &days, o.replay_rounds, &o.replay_from, &o.replay_to)?;
    }
    if let Some(dir) = &o.audit {
        let root = if dir == "data" { crate::frames::data_dir().join("runs") } else { std::path::PathBuf::from(dir) };
        let mut days: Vec<std::path::PathBuf> =
            std::fs::read_dir(&root).map_err(|e| format!("{}: {e}", root.display()))?.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
        days.sort();
        if days.is_empty() {
            days.push(root);
        }
        ok &= check_audit(&rf, &days, o.audit_runs)?;
    }
    if let Some(l) = &o.logs {
        ok &= check_logs(&rf, l)?;
    }
    if o.synth > 0 {
        ok &= check_synth(&rf, o.synth, o.jobs)?;
    }
    if let Some(dir) = &o.report {
        ok &= check_report(&rf, dir)?;
    }
    if o.report_synth > 0 {
        ok &= check_report_synth(&rf, o.report_synth, o.jobs)?;
    }
    if let Some(dir) = &o.db {
        ok &= check_db(&rf, dir)?;
    }
    if o.db_synth > 0 {
        ok &= check_db_synth(&rf, o.db_synth, o.jobs)?;
    }
    if let Some(f) = &o.db_file {
        ok &= check_db_file(&rf, f)?;
    }
    if o.tail > 0 {
        ok &= check_tail(&rf, o.tail, o.jobs)?;
    }
    if o.community > 0 {
        ok &= check_community(&rf, o.community > 1)?;
    }
    if o.engine > 0 {
        ok &= check_engine(&rf, o.engine, o.jobs)?;
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

/// Советник на сохранённых кадрах без срока: те же ответы, память забега и
/// оценки ходов на каждом кадре, что у эталона. Дни — параллельно.
fn check_replay(rf: &Reference, days: &[std::path::PathBuf], rounds: u32, from: &str, to: &str) -> Result<bool, String> {
    use gorn_prison::advisor::OwnAdvisor;
    use gorn_prison::solver::{StopRule, STOP_DEFAULTS};
    let t = Clock::now();
    type Part = (u64, u64, Vec<String>);
    let results: Vec<Result<Part, String>> = std::thread::scope(|sc| {
        let hs: Vec<_> = days
            .iter()
            .map(|day| {
                sc.spawn(move || -> Result<Part, String> {
                    let mut child = rf.spawn(&[
                        "replay".into(),
                        day.to_string_lossy().to_string(),
                        rounds.to_string(),
                        from.into(),
                        to.into(),
                    ])?;
                    let mut ts = BufReader::new(child.stdout.take().unwrap()).lines();
                    let (mut cnt, mut bad, mut shown) = (0u64, 0u64, vec![]);
                    let mut err: Option<String> = None;
                    let mut adv = OwnAdvisor::new();
                    let stop = StopRule { max_rounds: rounds as u64, ..STOP_DEFAULTS };
                    crate::replay::run(day, from, to, 1000.0, stop, f64::INFINITY, &mut adv, |adv, o| {
                        if err.is_some() {
                            return;
                        }
                        let mine = crate::replay::frame_json(adv, &o);
                        let line = match ts.next() {
                            Some(Ok(l)) => l,
                            _ => {
                                err = Some(format!("{}: эталон оборвался на {}", day.display(), o.file));
                                return;
                            }
                        };
                        let theirs: serde_json::Value = match serde_json::from_str(&line) {
                            Ok(v) => v,
                            Err(e) => {
                                err = Some(e.to_string());
                                return;
                            }
                        };
                        let mut d = vec![];
                        json_diff(&theirs, &mine, "", &mut d);
                        cnt += 1;
                        if !d.is_empty() {
                            bad += 1;
                            if shown.len() < SHOW {
                                shown.push(format!("{} {}:\n    {}", day.display(), o.file, d.join("\n    ")));
                            }
                        }
                    })?;
                    if let Some(e) = err {
                        return Err(e);
                    }
                    if ts.next().is_some() {
                        return Err(format!("{}: у эталона кадров больше", day.display()));
                    }
                    child.wait().map_err(|e| e.to_string())?;
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
    Ok(report(&format!("советник на кадрах ({rounds} раундов), кадров"), cnt, bad, &shown, t))
}

/// Ядро аудита (разбивка на забеги, находки, сводка) на записях наших повторов — как у эталона.
fn check_audit(rf: &Reference, days: &[std::path::PathBuf], runs_per_day: u32) -> Result<bool, String> {
    use crate::audit::{core_json, heads_json, heads_of, recs_json, replay, split_runs};
    let t = Clock::now();
    let data = crate::frames::data_dir();
    let (mut cnt, mut bad, mut shown) = (0u64, 0u64, vec![]);
    for day in days {
        let (files, heads) = heads_of(day)?;
        let runs = split_runs(&heads);
        let picked: Vec<_> = runs.iter().filter(|r| r.2).take(runs_per_day as usize).collect();
        // Пустые записи — только разбивка дня на забеги.
        let mut inputs = vec![(heads_json(&heads), serde_json::Value::Array(vec![]), core_json(&heads, &[]), "разбивка".to_string())];
        for &&(from, to, _) in &picked {
            let (recs, _) = replay(day, &data, &files[from..=to], 600.0, Some(48))?;
            inputs.push((heads_json(&[]), recs_json(&recs), core_json(&[], &recs), files[from].clone()));
        }
        for (h, r, mine, what) in inputs {
            let tmp = std::env::temp_dir().join(format!("gorn-audit-{}.json", std::process::id()));
            std::fs::write(&tmp, serde_json::json!({ "heads": h, "recs": r }).to_string()).map_err(|e| e.to_string())?;
            let out = String::from_utf8(rf.output(&["audit-core".into(), tmp.to_string_lossy().to_string()])?).map_err(|e| e.to_string())?;
            let _ = std::fs::remove_file(&tmp);
            let theirs: serde_json::Value = serde_json::from_str(out.trim()).map_err(|e| e.to_string())?;
            let mut d = vec![];
            json_diff(&theirs, &mine, "", &mut d);
            cnt += 1;
            if !d.is_empty() {
                bad += 1;
                if shown.len() < SHOW {
                    shown.push(format!("{} {what}:\n    {}", day.display(), d.join("\n    ")));
                }
            }
        }
    }
    Ok(report("ядро аудита (разбивка дней, находки и сводка забегов), проверок", cnt, bad, &shown, t))
}

// ── трекер: разбор логов ────────────────────────────────────────────────────

/// Логи игры в папке, как listLogFiles эталона (от старых к новым, текущий последним).
fn log_files(dir: &std::path::Path) -> Vec<(std::path::PathBuf, bool)> {
    let mut v: Vec<(std::path::PathBuf, bool, std::time::SystemTime)> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    let md = e.metadata().ok()?;
                    (name.starts_with("UE_game") && name.ends_with(".log") && md.len() > 0)
                        .then(|| (e.path(), name == "UE_game.log", md.modified().unwrap_or(std::time::UNIX_EPOCH)))
                })
                .collect()
        })
        .unwrap_or_default();
    v.sort_by(|a, b| a.1.cmp(&b.1).then(a.2.cmp(&b.2)));
    v.into_iter().map(|(p, c, _)| (p, c)).collect()
}

fn steam_log_dir() -> Option<std::path::PathBuf> {
    let pf86 = std::env::var("ProgramFiles(x86)").unwrap_or_else(|_| r"C:\Program Files (x86)".into());
    let d = std::path::Path::new(&pf86).join(r"Steam\steamapps\common\Torchlight Infinite\UE_game\TorchLight\Saved\Logs");
    d.exists().then_some(d)
}

fn map_keys() -> Result<(Vec<i64>, Vec<i64>), String> {
    let items: serde_json::Value = serde_json::from_str(include_str!("../../gorn-core/data/items.json")).map_err(|e| e.to_string())?;
    let all: Vec<i64> = items.as_array().unwrap().iter().filter_map(|it| it["id"].as_i64()).collect();
    let keys = items
        .as_array()
        .unwrap()
        .iter()
        .filter(|it| gorn_core::patterns::is_map_key_type(it["typeCn"].as_str()))
        .filter_map(|it| it["id"].as_i64())
        .collect();
    Ok((all, keys))
}

/// Один файл лога: разбор эталона против своего (JSON); отличия — в out.
fn compare_log(rf: &Reference, src: &std::path::Path, current: bool, keys: &std::collections::HashSet<i64>, out: &mut Vec<String>) -> Result<serde_json::Value, String> {
    let mut args = vec!["analyze".to_string(), src.to_string_lossy().to_string()];
    if current {
        args.push("current".into());
    }
    let raw = rf.output(&args)?;
    let theirs: serde_json::Value = serde_json::from_slice(&raw).map_err(|e| format!("{}: {e}", src.display()))?;
    let is_key = |id: i64| keys.contains(&id);
    let mine = gorn_core::analyze_log_file(src, gorn_core::AnalyzeOptions { is_map_key: Some(&is_key), current }).map_err(|e| e.to_string())?;
    let mine = serde_json::to_value(&mine).map_err(|e| e.to_string())?;
    json_diff(&theirs, &mine, "", out);
    Ok(theirs)
}

/// Синтетические логи (gorn-cli gen-log): все виды событий и краевые случаи — разбор как у эталона.
fn check_synth(rf: &Reference, n: u32, jobs: u32) -> Result<bool, String> {
    let t = Clock::now();
    let (items, keys_v) = map_keys()?;
    let keys: std::collections::HashSet<i64> = keys_v.iter().copied().collect();
    let parts = shards(0, n, jobs);
    type Part = (u64, u64, Vec<String>, [usize; 4]);
    let results: Vec<Result<Part, String>> = std::thread::scope(|sc| {
        let hs: Vec<_> = parts
            .iter()
            .map(|&(a, k)| {
                let (items, keys_v, keys) = (&items, &keys_v, &keys);
                sc.spawn(move || -> Result<Part, String> {
                    let (mut cnt, mut bad, mut shown, mut cov) = (0u64, 0u64, vec![], [0usize; 4]);
                    for seed in a..a + k {
                        let bytes = crate::genlog::generate(seed as u64, 3000, items, keys_v);
                        let tmp = std::env::temp_dir().join(format!("gorn-synth-{}-{seed}.log", std::process::id()));
                        std::fs::write(&tmp, &bytes).map_err(|e| e.to_string())?;
                        for current in [false, true] {
                            let mut d = vec![];
                            let theirs = compare_log(rf, &tmp, current, keys, &mut d)?;
                            cov[0] += theirs["runs"].as_array().map(|a| a.len()).unwrap_or(0);
                            cov[1] += theirs["prices"].as_array().map(|a| a.len()).unwrap_or(0);
                            cov[2] += ["listings", "recalls", "receipts", "purchases"].iter().map(|k| theirs["trade"][k].as_array().map(|a| a.len()).unwrap_or(0)).sum::<usize>();
                            cov[3] += theirs["players"].as_array().map(|a| a.len()).unwrap_or(0);
                            cnt += 1;
                            if !d.is_empty() {
                                bad += 1;
                                if shown.len() < SHOW {
                                    shown.push(format!("синтетический лог {seed} ({}):
    {}", if current { "текущий" } else { "целиком" }, d.join("
    ")));
                                }
                            }
                        }
                        let _ = std::fs::remove_file(&tmp);
                    }
                    Ok((cnt, bad, shown, cov))
                })
            })
            .collect();
        hs.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let (mut cnt, mut bad, mut shown, mut cov) = (0, 0, vec![], [0usize; 4]);
    for r in results {
        let (c, b, s, v) = r?;
        cnt += c;
        bad += b;
        shown.extend(s);
        for i in 0..4 {
            cov[i] += v[i];
        }
    }
    println!("  покрытие: забегов {}, пробивок цен {}, операций биржи {}, профилей {}", cov[0], cov[1], cov[2], cov[3]);
    Ok(report("трекер: синтетические логи (целиком и как текущий), разборов", cnt, bad, &shown, t))
}

/// Разбор каждого лога (забеги, цены, биржа, сумка, слоты, профили, нарезка) — как у эталона.
fn check_logs(rf: &Reference, spec: &str) -> Result<bool, String> {
    let t = Clock::now();
    let path = if spec == "game" { steam_log_dir().ok_or("папка логов игры не найдена")? } else { std::path::PathBuf::from(spec) };
    let files = if path.is_dir() { log_files(&path) } else { vec![(path.clone(), false)] };
    let items: serde_json::Value = serde_json::from_str(include_str!("../../gorn-core/data/items.json")).map_err(|e| e.to_string())?;
    let keys: std::collections::HashSet<i64> = items
        .as_array()
        .unwrap()
        .iter()
        .filter(|it| gorn_core::patterns::is_map_key_type(it["typeCn"].as_str()))
        .filter_map(|it| it["id"].as_i64())
        .collect();
    let is_key = |id: i64| keys.contains(&id);
    let (mut cnt, mut bad, mut shown) = (0u64, 0u64, vec![]);
    for (f, current) in &files {
        // Текущий лог игра дописывает: копия, чтобы обе стороны читали одно и то же.
        let src = if *current {
            let tmp = std::env::temp_dir().join(format!("gorn-verify-current-{}.log", std::process::id()));
            std::fs::copy(f, &tmp).map_err(|e| e.to_string())?;
            tmp
        } else {
            f.clone()
        };
        let mut args = vec!["analyze".to_string(), src.to_string_lossy().to_string()];
        if *current {
            args.push("current".into());
        }
        let out = rf.output(&args)?;
        let theirs: serde_json::Value = serde_json::from_slice(&out).map_err(|e| format!("{}: {e}", f.display()))?;
        let mine = gorn_core::analyze_log_file(&src, gorn_core::AnalyzeOptions { is_map_key: Some(&is_key), current: *current })
            .map_err(|e| e.to_string())?;
        let mine = serde_json::to_value(&mine).map_err(|e| e.to_string())?;
        if *current {
            let _ = std::fs::remove_file(&src);
        }
        let mut d = vec![];
        json_diff(&theirs, &mine, "", &mut d);
        cnt += 1;
        println!(
            "  {}: забегов {}, строк {}",
            f.file_name().unwrap().to_string_lossy(),
            theirs["runs"].as_array().map(|a| a.len()).unwrap_or(0),
            theirs["diagnostics"]["linesTotal"]
        );
        if !d.is_empty() {
            bad += 1;
            if shown.len() < SHOW {
                shown.push(format!("{}:\n    {}", f.display(), d.join("\n    ")));
            }
        }
    }
    Ok(report("трекер: разбор логов, файлов", cnt, bad, &shown, t))
}

/// Одна папка логов: ретро-скан и аналитика эталона против своих (JSON); отличия — в out.
fn compare_report(rf: &Reference, dir: &std::path::Path, out: &mut Vec<String>) -> Result<serde_json::Value, String> {
    let raw = rf.output(&["report".to_string(), dir.to_string_lossy().to_string()])?;
    let theirs: serde_json::Value = serde_json::from_slice(&raw).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mine = crate::report::build(dir)?;
    json_diff(&theirs, &mine, "", out);
    Ok(theirs)
}

fn report_line(v: &serde_json::Value) -> String {
    let len = |p: &str| v.pointer(p).and_then(|a| a.as_array()).map(|a| a.len()).unwrap_or(0);
    format!(
        "файлов {}, забегов {}, цен {}, стратегий {}, карт {}, сессий {}, недостающих {}, находок {}, в сумке {}, биржа {}",
        v["scan"]["filesScanned"], len("/scan/runs"), len("/scan/prices"), len("/strategies"), len("/maps"), len("/sessions"), len("/missing"),
        len("/top"), len("/bag/0/top"), len("/scan/trade/listings") + len("/scan/trade/receipts") + len("/scan/trade/purchases") + len("/scan/trade/recalls")
    )
}

/// Ретро-скан папки логов (по умолчанию — логов игры) и вся аналитика — как у эталона.
fn check_report(rf: &Reference, spec: &str) -> Result<bool, String> {
    let t = Clock::now();
    let src = if spec == "game" { steam_log_dir().ok_or("папка логов игры не найдена")? } else { std::path::PathBuf::from(spec) };
    // Игра может дописывать текущий лог: копия папки, чтобы обе стороны читали одно и то же.
    let tmp = std::env::temp_dir().join(format!("gorn-report-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    for (f, _) in log_files(&src) {
        let to = tmp.join(f.file_name().unwrap());
        std::fs::copy(&f, &to).map_err(|e| e.to_string())?;
        let mtime = std::fs::metadata(&f).and_then(|m| m.modified()).map_err(|e| e.to_string())?;
        std::fs::File::options().write(true).open(&to).and_then(|h| h.set_modified(mtime)).map_err(|e| e.to_string())?;
    }
    let mut d = vec![];
    let theirs = compare_report(rf, &tmp, &mut d);
    let _ = std::fs::remove_dir_all(&tmp);
    let theirs = theirs?;
    println!("  {}: {}", src.display(), report_line(&theirs));
    let shown = if d.is_empty() { vec![] } else { vec![format!("{}:
    {}", src.display(), d.iter().take(40).cloned().collect::<Vec<_>>().join("
    "))] };
    Ok(report("трекер: ретро-скан и аналитика, папок", 1, (!d.is_empty()) as u64, &shown, t))
}

/// Синтетические папки: несколько логов (бэкапы и текущий), перекрытия забегов и биржи между файлами.
fn check_report_synth(rf: &Reference, n: u32, jobs: u32) -> Result<bool, String> {
    let t = Clock::now();
    let (items, keys_v) = map_keys()?;
    let parts = shards(0, n, jobs);
    type Part = (u64, u64, Vec<String>);
    let results: Vec<Result<Part, String>> = std::thread::scope(|sc| {
        let hs: Vec<_> = parts
            .iter()
            .map(|&(a, k)| {
                let (items, keys_v) = (&items, &keys_v);
                sc.spawn(move || -> Result<Part, String> {
                    let (mut cnt, mut bad, mut shown) = (0u64, 0u64, vec![]);
                    for seed in a..a + k {
                        let dir = std::env::temp_dir().join(format!("gorn-report-synth-{}-{seed}", std::process::id()));
                        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                        let files = 1 + seed % 4;
                        let base = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_790_000_000);
                        for i in 0..files {
                            let name = if i + 1 == files && seed % 5 != 0 { "UE_game.log".to_string() } else { format!("UE_game-backup-2026.10.{:02}-10.00.00.log", 10 + i) };
                            // Файлы парами: второй — оборванная копия первого (ротация при краше),
                            // забеги и биржа перекрываются, у копий забегов разная полнота.
                            let mut bytes = crate::genlog::generate(seed as u64 * 7 + i as u64 / 2, 1500, items, keys_v);
                            if i % 2 == 1 {
                                bytes.truncate(bytes.len() * (40 + (seed % 50) as usize) / 100);
                            }
                            let path = dir.join(&name);
                            std::fs::write(&path, &bytes).map_err(|e| e.to_string())?;
                            let at = base + std::time::Duration::from_secs(3600 * i as u64);
                            std::fs::File::options().write(true).open(&path).and_then(|h| h.set_modified(at)).map_err(|e| e.to_string())?;
                        }
                        let mut d = vec![];
                        let r = compare_report(rf, &dir, &mut d);
                        let _ = std::fs::remove_dir_all(&dir);
                        let theirs = r?;
                        if seed < 3 {
                            println!("  папка {seed}: {}", report_line(&theirs));
                        }
                        cnt += 1;
                        if !d.is_empty() {
                            bad += 1;
                            if shown.len() < SHOW {
                                shown.push(format!("синтетическая папка {seed}:
    {}", d.iter().take(30).cloned().collect::<Vec<_>>().join("
    ")));
                            }
                        }
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
    Ok(report("трекер: ретро-скан и аналитика синтетических папок, папок", cnt, bad, &shown, t))
}

fn remove_db(p: &std::path::Path) {
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", p.display()));
    }
}

/// Хранилище на одной папке логов: сценарий записи у эталона и у себя, затем
/// перекрёстное чтение баз друг друга. Отличия — в out с префиксом шага.
fn compare_db(rf: &Reference, logs: &std::path::Path, tag: &str, out: &mut Vec<String>) -> Result<serde_json::Value, String> {
    let theirs_db = std::env::temp_dir().join(format!("gorn-db-{}-{tag}-ref.db", std::process::id()));
    let mine_db = std::env::temp_dir().join(format!("gorn-db-{}-{tag}-own.db", std::process::id()));
    remove_db(&theirs_db);
    remove_db(&mine_db);
    let result = (|| {
        let raw = rf.output(&["db-write".into(), logs.to_string_lossy().to_string(), theirs_db.to_string_lossy().to_string()])?;
        let theirs: serde_json::Value = serde_json::from_slice(&raw).map_err(|e| format!("db-write: {e}"))?;
        let mine = crate::dbcheck::write(logs, &mine_db)?;
        let mut d = vec![];
        json_diff(&theirs, &mine, "запись", &mut d);
        let season = theirs["season"].as_i64();
        let mut args = vec!["db-read".to_string(), mine_db.to_string_lossy().to_string()];
        if let Some(s) = season {
            args.push(s.to_string());
        }
        let raw = rf.output(&args)?;
        let their_read: serde_json::Value = serde_json::from_slice(&raw).map_err(|e| format!("db-read: {e}"))?;
        json_diff(&theirs, &their_read, "эталон читает свою базу", &mut d);
        let my_read = crate::dbcheck::read(&theirs_db, season)?;
        json_diff(&theirs, &my_read, "своя читает базу эталона", &mut d);
        out.extend(d);
        Ok(theirs)
    })();
    remove_db(&theirs_db);
    remove_db(&mine_db);
    result
}

/// Старая база (user_version = 0): миграции и ремонт сезонов — как у эталона.
fn compare_old_db(rf: &Reference, seed: u64, out: &mut Vec<String>) -> Result<(), String> {
    let a = std::env::temp_dir().join(format!("gorn-olddb-{}-{seed}-ref.db", std::process::id()));
    let b = std::env::temp_dir().join(format!("gorn-olddb-{}-{seed}-own.db", std::process::id()));
    let result = (|| {
        crate::dbcheck::old_fixture(&a, seed)?;
        std::fs::copy(&a, &b).map_err(|e| e.to_string())?;
        let raw = rf.output(&["db-read".into(), a.to_string_lossy().to_string(), "1601".into()])?;
        let theirs: serde_json::Value = serde_json::from_slice(&raw).map_err(|e| format!("db-read: {e}"))?;
        let mine = crate::dbcheck::read(&b, Some(1601))?;
        json_diff(&theirs, &mine, "миграция", out);
        Ok(())
    })();
    remove_db(&a);
    remove_db(&b);
    result
}

fn db_line(v: &serde_json::Value) -> String {
    format!(
        "забегов {}, строк дропа {}, цен {}, лотов {}, выплат {}, сессий {}",
        v["counts"]["runs"],
        v["counts"]["drops"],
        v["counts"]["priceRows"],
        v["counts"]["listings"],
        v["counts"]["receipts"],
        v["sessions"].as_array().map(|a| a.len()).unwrap_or(0)
    )
}

/// Хранилище на папке логов (по умолчанию — логов игры, копией).
fn check_db(rf: &Reference, spec: &str) -> Result<bool, String> {
    let t = Clock::now();
    let src = if spec == "game" { steam_log_dir().ok_or("папка логов игры не найдена")? } else { std::path::PathBuf::from(spec) };
    let tmp = std::env::temp_dir().join(format!("gorn-dblogs-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    for (f, _) in log_files(&src) {
        let to = tmp.join(f.file_name().unwrap());
        std::fs::copy(&f, &to).map_err(|e| e.to_string())?;
        let mtime = std::fs::metadata(&f).and_then(|m| m.modified()).map_err(|e| e.to_string())?;
        std::fs::File::options().write(true).open(&to).and_then(|h| h.set_modified(mtime)).map_err(|e| e.to_string())?;
    }
    let mut d = vec![];
    let r = compare_db(rf, &tmp, "game", &mut d);
    let _ = std::fs::remove_dir_all(&tmp);
    println!("  {}: {}", src.display(), db_line(&r?));
    let shown = if d.is_empty() { vec![] } else { vec![d.iter().take(40).cloned().collect::<Vec<_>>().join("\n    ")] };
    Ok(report("трекер: хранилище на логах, папок", 1, (!d.is_empty()) as u64, &shown, t))
}

/// Хранилище на синтетических папках и старых базах.
fn check_db_synth(rf: &Reference, n: u32, jobs: u32) -> Result<bool, String> {
    let t = Clock::now();
    let (items, keys_v) = map_keys()?;
    let parts = shards(0, n, jobs);
    type Part = (u64, u64, Vec<String>);
    let results: Vec<Result<Part, String>> = std::thread::scope(|sc| {
        let hs: Vec<_> = parts
            .iter()
            .map(|&(a, k)| {
                let (items, keys_v) = (&items, &keys_v);
                sc.spawn(move || -> Result<Part, String> {
                    let (mut cnt, mut bad, mut shown) = (0u64, 0u64, vec![]);
                    for seed in a..a + k {
                        let dir = std::env::temp_dir().join(format!("gorn-db-synth-{}-{seed}", std::process::id()));
                        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                        let files = 1 + seed % 3;
                        let base = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_790_000_000);
                        for i in 0..files {
                            let name = if i + 1 == files {
                                "UE_game.log".to_string()
                            } else {
                                format!("UE_game-backup-2026.10.{:02}-10.00.00.log", 10 + i)
                            };
                            let mut bytes = crate::genlog::generate(seed as u64 * 5 + i as u64 / 2, 1500, items, keys_v);
                            if i % 2 == 1 {
                                bytes.truncate(bytes.len() * (40 + (seed % 50) as usize) / 100);
                            }
                            let path = dir.join(&name);
                            std::fs::write(&path, &bytes).map_err(|e| e.to_string())?;
                            let at = base + std::time::Duration::from_secs(3600 * i as u64);
                            std::fs::File::options().write(true).open(&path).and_then(|h| h.set_modified(at)).map_err(|e| e.to_string())?;
                        }
                        let mut d = vec![];
                        let r = compare_db(rf, &dir, &seed.to_string(), &mut d);
                        let _ = std::fs::remove_dir_all(&dir);
                        let theirs = r?;
                        if seed < 3 {
                            println!("  папка {seed}: {}", db_line(&theirs));
                        }
                        compare_old_db(rf, seed as u64, &mut d)?;
                        cnt += 1;
                        if !d.is_empty() {
                            bad += 1;
                            if shown.len() < SHOW {
                                shown.push(format!("синтетическая папка {seed}:\n    {}", d.iter().take(30).cloned().collect::<Vec<_>>().join("\n    ")));
                            }
                        }
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
    Ok(report("трекер: хранилище (запись, перекрёстное чтение, миграции) синтетических папок, папок", cnt, bad, &shown, t))
}

/// Готовая база: две копии (с журналом WAL), эталон и своя открывают каждая свою —
/// миграции, ремонт сезонов и все выборки должны совпасть. Оригинал не трогается.
fn check_db_file(rf: &Reference, src: &str) -> Result<bool, String> {
    let t = Clock::now();
    let src = std::path::PathBuf::from(src);
    let copy = |tag: &str| -> Result<std::path::PathBuf, String> {
        let to = std::env::temp_dir().join(format!("gorn-dbfile-{}-{tag}.db", std::process::id()));
        remove_db(&to);
        for suffix in ["", "-wal"] {
            let from = format!("{}{suffix}", src.display());
            if std::path::Path::new(&from).exists() {
                std::fs::copy(&from, format!("{}{suffix}", to.display())).map_err(|e| format!("{from}: {e}"))?;
            }
        }
        Ok(to)
    };
    let (a, b) = (copy("ref")?, copy("own")?);
    let result = (|| {
        let mut season = None;
        let mut d = vec![];
        let mut theirs = serde_json::Value::Null;
        // Сначала без сезона, затем — в сезоне последнего профиля.
        for pass in 0..2 {
            let mut args = vec!["db-read".to_string(), a.to_string_lossy().to_string()];
            if let Some(s) = season {
                args.push(format!("{s}"));
            }
            let raw = rf.output(&args)?;
            theirs = serde_json::from_slice(&raw).map_err(|e| format!("db-read: {e}"))?;
            let mine = crate::dbcheck::read(&b, season)?;
            json_diff(&theirs, &mine, if pass == 0 { "без сезона" } else { "в сезоне" }, &mut d);
            season = theirs["players"].get(0).and_then(|p| p["seasonId"].as_f64()).map(|s| s as i64);
        }
        Ok::<_, String>((theirs, d))
    })();
    remove_db(&a);
    remove_db(&b);
    let (theirs, d) = result?;
    println!("  {}: {}", src.display(), db_line(&theirs));
    let shown = if d.is_empty() { vec![] } else { vec![d.iter().take(40).cloned().collect::<Vec<_>>().join("\n    ")] };
    Ok(report("трекер: хранилище, готовая база", 1, (!d.is_empty()) as u64, &shown, t))
}

/// Дочитывание: сценарии записи порциями (посреди строки и символа), ротации,
/// старта с конца, с начала, с позиции и до появления файла — события как у эталона.
fn check_tail(rf: &Reference, n: u32, jobs: u32) -> Result<bool, String> {
    let t = Clock::now();
    let (items, keys_v) = map_keys()?;
    let parts = shards(0, n, jobs);
    type Part = (u64, u64, Vec<String>, usize);
    let results: Vec<Result<Part, String>> = std::thread::scope(|sc| {
        let hs: Vec<_> = parts
            .iter()
            .map(|&(a, k)| {
                let (items, keys_v) = (&items, &keys_v);
                sc.spawn(move || -> Result<Part, String> {
                    let (mut cnt, mut bad, mut shown, mut lines) = (0u64, 0u64, vec![], 0usize);
                    for seed in a..a + k {
                        let tmp = std::env::temp_dir();
                        let id = format!("{}-{seed}", std::process::id());
                        let steps_file = tmp.join(format!("gorn-tail-{id}.json"));
                        let (theirs_log, mine_log) = (tmp.join(format!("gorn-tail-{id}-ref.log")), tmp.join(format!("gorn-tail-{id}-own.log")));
                        let steps = crate::tailcheck::script(seed as u64, items, keys_v);
                        std::fs::write(&steps_file, steps.to_string()).map_err(|e| e.to_string())?;
                        let raw = rf.output(&["tail".into(), steps_file.to_string_lossy().to_string(), theirs_log.to_string_lossy().to_string()]);
                        let mine = crate::tailcheck::run(&steps, &mine_log);
                        for f in [&steps_file, &theirs_log, &mine_log] {
                            let _ = std::fs::remove_file(f);
                        }
                        let theirs: serde_json::Value = serde_json::from_slice(&raw?).map_err(|e| format!("tail: {e}"))?;
                        let mine = mine?;
                        lines += theirs.as_array().map(|a| a.iter().filter_map(|e| e[1].as_array()).map(|l| l.len()).sum::<usize>()).unwrap_or(0);
                        let mut d = vec![];
                        json_diff(&theirs, &mine, "", &mut d);
                        cnt += 1;
                        if !d.is_empty() {
                            bad += 1;
                            if shown.len() < SHOW {
                                shown.push(format!("сценарий {seed}:\n    {}", d.iter().take(20).cloned().collect::<Vec<_>>().join("\n    ")));
                            }
                        }
                    }
                    Ok((cnt, bad, shown, lines))
                })
            })
            .collect();
        hs.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let (mut cnt, mut bad, mut shown, mut lines) = (0, 0, vec![], 0);
    for r in results {
        let (c, b, s, l) = r?;
        cnt += c;
        bad += b;
        shown.extend(s);
        lines += l;
    }
    println!("  строк выдано: {lines}");
    Ok(report("трекер: дочитывание лога, сценариев", cnt, bad, &shown, t))
}

/// Лента в папке: кэш сезона и, с fetch, обновление из сети — тот же JSON, что у эталона.
fn community_dump(dir: &std::path::Path, file: &str, season: i64, fetch: bool) -> serde_json::Value {
    let feed = gorn_core::community::CommunityPriceFeed::new(dir, file);
    feed.switch_season(season);
    feed.load_cache(season);
    let loaded = serde_json::json!({ "size": feed.size(), "cursor": feed.cursor(), "fetchedAt": feed.fetched_at_ms() });
    let changed = fetch.then(|| feed.refresh());
    let mut all = feed.all();
    all.sort_by_key(|p| p.config_base_id);
    serde_json::json!({ "loaded": loaded, "changed": changed, "stale": feed.stale(), "cursor": feed.cursor(), "all": all })
}

/// Цены сообщества: кэши трекера (на копиях), с сетью — дельта от кэша, полная
/// выборка и чтение эталоном кэша, записанного своей лентой.
fn check_community(rf: &Reference, network: bool) -> Result<bool, String> {
    let t = Clock::now();
    let src = std::path::PathBuf::from(std::env::var("APPDATA").map_err(|e| e.to_string())?).join("gorn-tli");
    let mut files: Vec<String> = std::fs::read_dir(&src)
        .map_err(|e| format!("{}: {e}", src.display()))?
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.starts_with("community-prices") && n.ends_with(".json"))
        .collect();
    files.sort();
    let (mut cnt, mut bad, mut shown) = (0u64, 0u64, vec![]);
    let mut check = |what: String, theirs: &serde_json::Value, mine: &serde_json::Value, skip: &[&str]| {
        let mut d = vec![];
        let strip = |v: &serde_json::Value| {
            let mut v = v.clone();
            for k in skip {
                v["loaded"].as_object_mut().map(|o| o.remove(*k));
                v.as_object_mut().map(|o| o.remove(*k));
            }
            v
        };
        json_diff(&strip(theirs), &strip(mine), "", &mut d);
        println!("  {what}: цен {}", theirs["all"].as_array().map(|a| a.len()).unwrap_or(0));
        cnt += 1;
        if !d.is_empty() {
            bad += 1;
            if shown.len() < SHOW {
                shown.push(format!("{what}:\n    {}", d.iter().take(20).cloned().collect::<Vec<_>>().join("\n    ")));
            }
        }
    };
    let tmp = |tag: &str| std::env::temp_dir().join(format!("gorn-community-{}-{tag}", std::process::id()));
    for name in &files {
        let text = std::fs::read_to_string(src.join(name)).map_err(|e| e.to_string())?;
        let season = serde_json::from_str::<serde_json::Value>(&text).ok().and_then(|v| v["seasonId"].as_i64()).unwrap_or(0);
        for fetch in if network { vec![false, true] } else { vec![false] } {
            let (a, b) = (tmp("ref"), tmp("own"));
            for d in [&a, &b] {
                std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
                std::fs::write(d.join(name), &text).map_err(|e| e.to_string())?;
            }
            let mut args = vec!["community".to_string(), a.to_string_lossy().to_string(), name.clone(), season.to_string()];
            if fetch {
                args.push("fetch".into());
            }
            let theirs: serde_json::Value = serde_json::from_slice(&rf.output(&args)?).map_err(|e| e.to_string())?;
            let mine = community_dump(&b, name, season, fetch);
            check(format!("{name}{}", if fetch { ", дельта из сети" } else { "" }), &theirs, &mine, &[]);
            let _ = std::fs::remove_dir_all(&a);
            let _ = std::fs::remove_dir_all(&b);
        }
    }
    if network {
        // Полная выборка с пустого места, затем эталон читает кэш своей ленты.
        let season = 1601;
        let (a, b) = (tmp("full-ref"), tmp("full-own"));
        for d in [&a, &b] {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        let file = "community-prices.json".to_string();
        let theirs: serde_json::Value = serde_json::from_slice(&rf.output(&["community".into(), a.to_string_lossy().to_string(), file.clone(), season.to_string(), "fetch".into()])?)
            .map_err(|e| e.to_string())?;
        let mine = community_dump(&b, &file, season, true);
        check(format!("полная выборка сезона {season}"), &theirs, &mine, &[]);
        let their_read: serde_json::Value =
            serde_json::from_slice(&rf.output(&["community".into(), b.to_string_lossy().to_string(), file.clone(), season.to_string()])?).map_err(|e| e.to_string())?;
        let my_read = community_dump(&b, &file, season, false);
        check("эталон читает кэш своей ленты".into(), &their_read, &my_read, &["fetchedAt"]);
        let _ = std::fs::remove_dir_all(&a);
        let _ = std::fs::remove_dir_all(&b);
    }
    Ok(report("трекер: цены сообщества, проверок", cnt, bad, &shown, t))
}

/// Движок трекера: сценарии на синтетических логах (часть — поверх настоящего
/// gorn.db трекера, на копии), срезы и итоговая база — как у эталона.
fn check_engine(rf: &Reference, n: u32, jobs: u32) -> Result<bool, String> {
    let t = Clock::now();
    let (items, keys_v) = map_keys()?;
    let appdata = std::path::PathBuf::from(std::env::var("APPDATA").unwrap_or_default()).join("gorn-tli");
    let parts = shards(0, n, jobs);
    type Part = (u64, u64, Vec<String>, usize);
    let results: Vec<Result<Part, String>> = std::thread::scope(|sc| {
        let hs: Vec<_> = parts
            .iter()
            .map(|&(a, k)| {
                let (items, keys_v, appdata) = (&items, &keys_v, &appdata);
                sc.spawn(move || -> Result<Part, String> {
                    let (mut cnt, mut bad, mut shown, mut snaps) = (0u64, 0u64, vec![], 0usize);
                    for seed in a..a + k {
                        let root = std::env::temp_dir().join(format!("gorn-engine-{}-{seed}", std::process::id()));
                        let _ = std::fs::remove_dir_all(&root);
                        let (logs, da, db) = (root.join("logs"), root.join("ref"), root.join("own"));
                        for d in [&logs, &da, &db] {
                            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
                        }
                        // Бэкап прошлой сессии и текущий лог: начало — уже на диске, остальное — живьём.
                        let base = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_790_000_000);
                        if seed % 2 == 0 {
                            let p = logs.join("UE_game-backup-2026.10.10-10.00.00.log");
                            std::fs::write(&p, crate::genlog::generate(seed as u64 * 3 + 101, 1200, items, keys_v)).map_err(|e| e.to_string())?;
                            std::fs::File::options().write(true).open(&p).and_then(|h| h.set_modified(base)).map_err(|e| e.to_string())?;
                        }
                        let full = crate::genlog::generate(seed as u64 * 3 + 7, 1500, items, keys_v);
                        let cut = full[..full.len() * (20 + (seed % 60) as usize) / 100].iter().rposition(|&b| b == b'\n').map(|i| i + 1).unwrap_or(0);
                        let cur = logs.join("UE_game.log");
                        std::fs::write(&cur, &full[..cut]).map_err(|e| e.to_string())?;
                        std::fs::File::options().write(true).open(&cur).and_then(|h| h.set_modified(base + std::time::Duration::from_secs(3600))).map_err(|e| e.to_string())?;
                        let next = if seed % 4 == 1 { crate::genlog::generate(seed as u64 * 3 + 9, 600, items, keys_v) } else { vec![] };
                        // Данные: кэш цен сообщества и (часть сценариев) настоящая база трекера.
                        for d in [&da, &db] {
                            for name in ["community-prices.json", "community-prices-1501.json"] {
                                let from = appdata.join(name);
                                if from.exists() {
                                    std::fs::copy(&from, d.join(name)).map_err(|e| e.to_string())?;
                                }
                            }
                            if seed % 3 == 0 {
                                for suffix in ["", "-wal"] {
                                    let from = appdata.join(format!("gorn.db{suffix}"));
                                    if from.exists() {
                                        std::fs::copy(&from, d.join(format!("gorn.db{suffix}"))).map_err(|e| e.to_string())?;
                                    }
                                }
                            }
                        }
                        let players: Vec<String> = (0..3).map(|i| format!("95608272857761{i:04}")).chain(["0".to_string()]).collect();
                        let script = crate::enginecheck::script(seed as u64, &logs, &full[cut..], &next, &players);
                        let script_file = root.join("script.json");
                        std::fs::write(&script_file, script.to_string()).map_err(|e| e.to_string())?;
                        let raw = rf.output(&["engine".into(), script_file.to_string_lossy().to_string(), da.to_string_lossy().to_string()]);
                        let mine = crate::enginecheck::run(&script, &db);
                        let r = (|| {
                            let theirs: serde_json::Value = serde_json::from_slice(&raw?).map_err(|e| format!("engine: {e}"))?;
                            let mine = mine?;
                            let mut d = vec![];
                            json_diff(&theirs, &mine, "", &mut d);
                            let tdb = crate::dbcheck::read(&da.join("gorn.db"), Some(1601))?;
                            let mdb = crate::dbcheck::read(&db.join("gorn.db"), Some(1601))?;
                            json_diff(&tdb, &mdb, "база", &mut d);
                            if seed < 6 {
                                let snaps: Vec<&serde_json::Value> = theirs.as_array().into_iter().flatten().filter(|e| e[0] == "snapshot").map(|e| &e[1]).collect();
                                let count = |f: &dyn Fn(&serde_json::Value) -> bool| snaps.iter().filter(|s| f(s)).count();
                                let len = |v: &serde_json::Value| v.as_array().map(|a| a.len()).unwrap_or(0);
                                println!(
                                    "  сценарий {seed}: срезов {}, в карте {}, с сумкой {}, с находками {}, со стратегиями {}, с прошлыми сезонами {}, забегов в истории до {}",
                                    snaps.len(),
                                    count(&|s| s["status"] == "running"),
                                    count(&|s| !s["bag"].is_null()),
                                    count(&|s| len(&s["sessionTop"]) > 0),
                                    count(&|s| len(&s["strategies"]) > 0),
                                    count(&|s| s["otherSeasonRuns"].as_u64().unwrap_or(0) > 0),
                                    snaps.iter().map(|s| s["rate"]["sampleSize"].as_u64().unwrap_or(0)).max().unwrap_or(0)
                                );
                            }
                            Ok::<_, String>((theirs.as_array().map(|a| a.len()).unwrap_or(0), d))
                        })();
                        let _ = std::fs::remove_dir_all(&root);
                        let (k, d) = r?;
                        snaps += k;
                        cnt += 1;
                        if !d.is_empty() {
                            bad += 1;
                            if shown.len() < SHOW {
                                shown.push(format!("сценарий {seed} (отличий {}):\n    {}", d.len(), d.iter().take(25).cloned().collect::<Vec<_>>().join("\n    ")));
                            }
                        }
                    }
                    Ok((cnt, bad, shown, snaps))
                })
            })
            .collect();
        hs.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let (mut cnt, mut bad, mut shown, mut snaps) = (0, 0, vec![], 0);
    for r in results {
        let (c, b, s, k) = r?;
        cnt += c;
        bad += b;
        shown.extend(s);
        snaps += k;
    }
    println!("  срезов, поисков и выгрузок сверено: {snaps}");
    Ok(report("трекер: движок (срезы по сценарию и итоговая база), сценариев", cnt, bad, &shown, t))
}
