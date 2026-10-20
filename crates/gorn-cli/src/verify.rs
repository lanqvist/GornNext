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
}

/// Расхождений показываем не больше стольких на проверку.
const SHOW: usize = 5;

pub fn run(o: &Options) -> Result<bool, String> {
    let rf = Reference::locate(o.tli.as_deref())?;
    println!("эталон: {}", rf.tli.display());
    let mut ok = true;
    ok &= check_weights(&rf)?;
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

fn check_weights(rf: &Reference) -> Result<bool, String> {
    let t = Clock::now();
    let out = String::from_utf8(rf.output(&["weights".into()])?).map_err(|e| e.to_string())?;
    let w = default_weights();
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
    Ok(report("веса", n as u64, bad, &shown, t))
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
