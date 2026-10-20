//! `gorn-cli bench` и `gorn-cli selfplay`: замеры — те же, что `bench` в
//! tools/reference.mts (TS), на тех же сидах; средние призм. обязаны совпасть.

use std::hint::black_box;
use std::time::Instant as Clock;

use gorn_prison::actions::{random_playout, start_run, CodeBuf, RunOptions};
use gorn_prison::policy::{default_weights, Instant};
use gorn_prison::rng::Rng;
use gorn_prison::runs::{start_for, Mode, Runner, ALL_TOOLS, SIX_TOOLS};
use gorn_prison::state::{State, PHASE, P_END};
use gorn_prison::{apply_code, DEFAULT_PARAMS};
use rayon::prelude::*;

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

fn q(sorted: &[f64], f: f64) -> f64 {
    sorted[((f * sorted.len() as f64) as usize).min(sorted.len() - 1)]
}

pub fn bench(selfplay_seeds: u32, threads: usize) {
    let p = &DEFAULT_PARAMS;
    let w = default_weights();
    let mut buf: CodeBuf = [0; 32];
    let mut s;

    // 1. Случайные доигрывания — как prison-bench (медиана из 5 серий по 20 000).
    const N: usize = 20000;
    for (label, tools) in [("без орудий", 0), ("6 орудий", SIX_TOOLS), ("все 31 орудие", ALL_TOOLS)] {
        let mut r = Rng::new(1);
        let mut start = State::new();
        start_run(&mut start, &mut r, p, RunOptions { tools, ..RunOptions::default() });
        let mut total = 0i64;
        for _ in 0..5000 {
            s = start;
            total += random_playout(&mut s, &mut r, p, &mut buf) as i64;
        }
        let mut times = vec![];
        for _ in 0..5 {
            let t0 = Clock::now();
            for _ in 0..N {
                s = black_box(start);
                total += random_playout(&mut s, &mut r, p, &mut buf) as i64;
            }
            times.push(t0.elapsed().as_secs_f64() * 1e6 / N as f64);
        }
        println!(
            "random {label}: {:.2} мкс (медиана из 5), {:.3} призм.",
            median(times),
            total as f64 / (5000 + 5 * N) as f64
        );
    }

    // 2. Доигрывание мгновенной стратегией от старта instant-сидов 0…K−1.
    {
        const K: usize = 4000;
        let starts: Vec<State> = (0..K as u32)
            .map(|g| {
                let mut t = State::new();
                start_for(Mode::Instant, &mut t, &mut Rng::new(g), p, g);
                t
            })
            .collect();
        let mut inst = Instant::new();
        let mut r = Rng::new(12345);
        let mut total = 0i64;
        for st in &starts {
            s = *st;
            total += inst.playout(&mut s, &mut r, p, w) as i64;
        }
        let mut times = vec![];
        for _ in 0..5 {
            let t0 = Clock::now();
            for st in &starts {
                s = black_box(*st);
                total += inst.playout(&mut s, &mut r, p, w) as i64;
            }
            times.push(t0.elapsed().as_secs_f64() * 1e6 / K as f64);
        }
        println!("policy playout: {:.2} мкс (медиана из 5), {:.3} призм.", median(times), total as f64 / (6 * K) as f64);
    }

    // 3. Мгновенный совет: позиции из самоигры сидов 0…299.
    {
        let mut inst = Instant::new();
        let mut pos: Vec<State> = vec![];
        for g in 0..300u32 {
            let mut t = State::new();
            let mut r = Rng::new(g);
            start_for(Mode::Instant, &mut t, &mut r, p, g);
            while t.m[PHASE] != P_END {
                pos.push(t);
                let c = inst.code(&t, p, w);
                apply_code(&mut t, c, &mut r, p).unwrap();
            }
        }
        let mut sink = 0i64;
        for _ in 0..3 {
            for t in &pos {
                sink += inst.code(black_box(t), p, w) as i64;
            }
        }
        let mut batches = vec![];
        for _ in 0..5 {
            let t0 = Clock::now();
            for t in &pos {
                sink += inst.code(black_box(t), p, w) as i64;
            }
            batches.push(t0.elapsed().as_secs_f64() * 1e6 / pos.len() as f64);
        }
        let mut each: Vec<f64> = pos
            .iter()
            .map(|t| {
                let t0 = Clock::now();
                sink += inst.code(black_box(t), p, w) as i64;
                t0.elapsed().as_secs_f64() * 1e6
            })
            .collect();
        each.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!(
            "instant advice: {} позиций, среднее {:.3} мкс (медиана из 5 пакетов); по вызовам медиана {:.2}, p95 {:.2}, max {:.1} мкс [{}]",
            pos.len(),
            median(batches),
            q(&each, 0.5),
            q(&each, 0.95),
            q(&each, 1.0),
            sink & 1
        );
    }

    // 4. Самоигра мгновенной стратегией: один поток и все ядра.
    selfplay(Mode::Instant, 0, selfplay_seeds, 1);
    if threads != 1 {
        selfplay(Mode::Instant, 0, selfplay_seeds, threads);
    }
}

/// Самоигра сидов from…from+n−1; threads 0 — все ядра. Возвращает среднее призм.
pub fn selfplay(mode: Mode, from: u32, n: u32, threads: usize) -> f64 {
    let p = &DEFAULT_PARAMS;
    let w = default_weights();
    let t0 = Clock::now();
    let (total, moves, used) = if threads == 1 {
        let mut runner = Runner::new(p, w);
        let mut total = 0i64;
        let mut moves = 0u64;
        for g in from..from + n {
            total += runner.run_with(mode, g, |_, _| moves += 1) as i64;
        }
        (total, moves, 1)
    } else {
        let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build().expect("пул потоков");
        let used = pool.current_num_threads();
        let (total, moves) = pool.install(|| {
            (from..from + n)
                .into_par_iter()
                .with_min_len(64)
                .map_init(
                    || Runner::new(p, w),
                    |runner, g| {
                        let mut k = 0u64;
                        let sc = runner.run_with(mode, g, |_, _| k += 1);
                        (sc as i64, k)
                    },
                )
                .reduce(|| (0, 0), |a, b| (a.0 + b.0, a.1 + b.1))
        });
        (total, moves, used)
    };
    let sec = t0.elapsed().as_secs_f64();
    let mean = total as f64 / n as f64;
    println!(
        "selfplay {} {n}, потоков {used}: {sec:.2} с, {:.2} мкс на забег, {:.1} ходов, {mean:.4} призм. в среднем",
        mode.name(),
        sec * 1e6 / n as f64,
        (moves - n as u64) as f64 / n as f64,
    );
    mean
}
