//! tests/data/rng.txt — снимок эталона на TS (gorn-tli, tools/reference.mts
//! `rng <сид> 1 16`); tests/data/runs.txt — снимок движка GornNext (`gorn-cli
//! snapshot-runs tests/data/runs.txt`): правила с 22.10 расходятся с эталоном,
//! который больше не меняется. Переснимать — только при намеренной правке
//! правил. Проходит без node и без соседнего репозитория.

use gorn_prison::rng::{Label, Rng};
use gorn_prison::runs::{Mode, Runner, Summary};
use gorn_prison::{default_weights, DEFAULT_PARAMS};

/// Токены строки `rng` в reference.mts: целые как строки, дробные — как f64.
fn rng_line(g: u32) -> Vec<String> {
    let mut out = vec![g.to_string()];
    let mut r = Rng::new(g);
    for _ in 0..16 {
        out.push(r.next().to_string());
    }
    let mut r = Rng::new(g);
    for _ in 0..8 {
        out.push(r.bits30().to_string());
    }
    for _ in 0..4 {
        out.push(format!("f{}", r.float().to_bits()));
    }
    for k in [1, 2, 3, 4, 5, 16, 31, 1000, 0x4000_0000] {
        out.push(r.int(k).to_string());
    }
    for q in [0.0, 0.25, 0.35, 0.5, 0.8, 1.0] {
        out.push((r.chance(q) as i32).to_string());
    }
    out.push(r.s().to_string());
    for label in [Label::Str("doors"), Label::Str("событие"), Label::Num(g & 0xffff)] {
        let mut t = r.stream(label);
        out.push(t.next().to_string());
        out.push(t.next().to_string());
    }
    out
}

#[test]
fn rng_matches_reference() {
    let data = include_str!("data/rng.txt");
    let mut n = 0;
    for line in data.lines().filter(|l| !l.trim().is_empty()) {
        let ts: Vec<String> = line
            .split_whitespace()
            .map(|t| if t.contains('.') || t.contains('e') { format!("f{}", t.parse::<f64>().unwrap().to_bits()) } else { t.to_string() })
            .collect();
        let g: u32 = ts[0].parse().unwrap();
        assert_eq!(ts, rng_line(g), "сид {g}");
        n += 1;
    }
    assert_eq!(n, 7);
}

#[test]
fn rng_known_values() {
    // mulberry32: первые числа для сида 1 (эталон, rng.ts)
    let mut r = Rng::new(1);
    assert_eq!(r.next(), 2693262067);
    assert_eq!(r.next(), 11749833);
    assert_eq!(r.s(), 1u32.wrapping_add(2 * 0x6d2b_79f5));
    // seed() начинает заново
    r.seed(1);
    assert_eq!(r.next(), 2693262067);
    // int(n) всегда в [0, n), chance(0) / chance(1) — крайние
    let mut r = Rng::new(42);
    for n in 1..200 {
        let x = r.int(n);
        assert!((0..n).contains(&x));
        assert!(!r.chance(0.0));
        assert!(r.chance(1.0));
    }
}

#[test]
fn runs_match_reference() {
    let p = &DEFAULT_PARAMS;
    let mut runner = Runner::new(p, default_weights());
    let data = include_str!("data/runs.txt");
    let mut n = 0;
    for line in data.lines().filter(|l| !l.trim().is_empty()) {
        let f: Vec<&str> = line.split_whitespace().collect();
        let mode = Mode::parse(f[0]).unwrap();
        let g: u32 = f[1].parse().unwrap();
        let want = Summary {
            steps: f[2].parse().unwrap(),
            score: f[3].parse().unwrap(),
            h1: f[4].parse().unwrap(),
            h2: f[5].parse().unwrap(),
        };
        assert_eq!(runner.run_hash(mode, g), want, "{line}");
        n += 1;
    }
    assert_eq!(n, 24);
}

#[test]
fn weights_parsed_exactly() {
    let w = default_weights();
    // те же литералы, что в weights.json: число из JSON — ближайший f64
    assert_eq!(w[gorn_prison::policy::weights::W_VAL0 + 3], 0.2214);
    assert_eq!(w[gorn_prison::policy::weights::W_DOOR], -0.0372);
    assert_eq!(w[gorn_prison::policy::weights::W_BRAND], 0.05);
}
