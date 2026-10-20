//! `gorn-cli tune`: настройка весов мгновенной стратегии самоигрой.
//! Перенос `src/cli/prison-tune.ts`: забеги и их кости те же, поэтому при
//! тех же ключах шаги и итоговые веса совпадают с эталоном.
//!
//! Все кандидаты играют одни и те же забеги (общие кости): разница двух
//! наборов весов меряется по парам забегов. Принимается шаг, который лучше
//! на 2 SE.

use gorn_prison::actions::{random_playout, start_run, CodeBuf, RunOptions};
use gorn_prison::model::{start_event, tool_index, START_EVENT_IDS};
use gorn_prison::policy::greedy::greedy_playout;
use gorn_prison::policy::weights::{hand_weights, to_json, weight_name, W_GIFT, W_SIZE, W_TOOL, W_VAL0};
use gorn_prison::policy::{default_weights, Instant, Weights};
use gorn_prison::rng::Rng;
use gorn_prison::state::State;
use gorn_prison::DEFAULT_PARAMS;
use rayon::prelude::*;

#[derive(Clone, Copy, Debug)]
pub struct Scenario {
    /// None — стартовые события по кругу.
    pub start: Option<i32>,
    pub tools: i32,
    pub start_pick: bool,
    pub no_objective: bool,
}

pub const MIX: Scenario = Scenario { start: None, tools: 0, start_pick: true, no_objective: false };

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Random,
    Greedy,
    Policy,
}

fn run_options(sc: &Scenario, i: u32) -> RunOptions {
    RunOptions {
        start: sc.start.unwrap_or((i % 5) as i32),
        objective: if sc.no_objective { Some(-1) } else { None },
        tools: sc.tools,
        start_pick: sc.start_pick,
    }
}

/// Счёт забегов 0…n−1; одинаковые seed и сценарий — одинаковые забеги.
pub fn scores(kind: Kind, w: &Weights, sc: &Scenario, seed: u32, n: u32) -> Vec<f64> {
    let p = &DEFAULT_PARAMS;
    (0..n)
        .into_par_iter()
        .with_min_len(32)
        .map_init(
            || (State::new(), Instant::new(), [0i32; 32] as CodeBuf, Rng::new(1)),
            |(s, inst, buf, r), i| {
                r.seed((seed ^ i).wrapping_mul(0x9e37_79b1) ^ i);
                start_run(s, r, p, run_options(sc, i));
                (match kind {
                    Kind::Random => random_playout(s, r, p, buf),
                    Kind::Greedy => greedy_playout(s, r, p),
                    Kind::Policy => inst.playout(s, r, p, w),
                }) as f64
            },
        )
        .collect()
}

fn mean(a: &[f64]) -> f64 {
    a.iter().sum::<f64>() / a.len() as f64
}

/// Среднее и SE разницы по парам забегов.
fn paired(a: &[f64], b: &[f64]) -> (f64, f64) {
    let (mut s, mut s2) = (0.0, 0.0);
    for (x, y) in a.iter().zip(b) {
        let d = x - y;
        s += d;
        s2 += d * d;
    }
    let n = a.len() as f64;
    let diff = s / n;
    let v = s2 / n - diff * diff;
    (diff, ((if v > 0.0 { v } else { 0.0 }) / n).sqrt())
}

/// Шаг по весу i: множитель для ценностей узников, сдвиг для прочих.
fn step(i: usize, w: &Weights, scale: f64) -> f64 {
    if i < W_GIFT {
        let a = w[i].abs() * 0.3;
        return (if a > 0.01 { a } else { 0.01 }) * scale;
    }
    if i >= W_TOOL {
        return 0.2 * scale;
    }
    const BASE: [f64; 12] = [0.1, 0.02, 0.03, 0.03, 0.02, 0.1, 0.03, 0.005, 0.03, 0.05, 0.05, 0.05];
    BASE[i - W_GIFT] * scale
}

pub fn tune(runs: u32, passes: u32, from_hand: bool, out: &str) -> Result<(), String> {
    let sc = MIX;
    let seed = 0x51ed;
    let mut w = if from_hand { hand_weights() } else { *default_weights() };
    let mut base = scores(Kind::Policy, &w, &sc, seed, runs);
    println!("старт: {:.3} призм. ({runs} забегов)", mean(&base));
    let mut scale = 1.0;
    for pass in 0..passes {
        let mut accepted = 0;
        for i in W_VAL0..W_SIZE {
            for dir in [1.0, -1.0] {
                let mut cand = w;
                cand[i] = w[i] + dir * step(i, &w, scale);
                // Призматический с подарком не может стоить меньше призматического: это сам счёт.
                if i == W_GIFT && cand[i] < 0.0 {
                    continue;
                }
                let sc2 = scores(Kind::Policy, &cand, &sc, seed, runs);
                let (diff, se) = paired(&sc2, &base);
                if diff > 2.0 * se && diff > 0.0 {
                    w = cand;
                    base = sc2;
                    accepted += 1;
                    println!("  {} → {:.4}: +{diff:.3} (SE {se:.3}), {:.3}", weight_name(i), w[i], mean(&base));
                    break;
                }
            }
        }
        println!("проход {}: принято {accepted}, {:.3} призм.", pass + 1, mean(&base));
        std::fs::write(out, to_json(&w)).map_err(|e| format!("{out}: {e}"))?;
        scale *= 0.6;
        if accepted == 0 && pass > 0 {
            break;
        }
    }
    println!("веса — {out} (в стратегию: скопировать в crates/gorn-prison/data/weights.json)");
    Ok(())
}

fn tools(ids: &[&str]) -> i32 {
    ids.iter().map(|id| 1 << tool_index(id).expect("орудие")).fold(0, |a, b| a | b)
}

pub fn report(runs: u32) {
    let seed = 0xbeef; // не те забеги, на которых настраивали
    let hand = hand_weights();
    let none = |t: &[&str]| Scenario { start: Some(start_event::NONE), tools: tools(t), start_pick: false, no_objective: false };
    let mut cases: Vec<(String, Scenario)> = vec![
        ("все стартовые события поровну, выбор орудий".into(), MIX),
        (
            "без орудий (без выбора и без целей)".into(),
            Scenario { start: Some(start_event::NONE), tools: 0, start_pick: false, no_objective: true },
        ),
        ("двери: связка, поддельный ключ, короткая цепь".into(), none(&["keys", "forged", "shortchain"])),
        ("пожирание: плеть, ошейник, пила, корона".into(), none(&["whip", "collar", "saw", "crown"])),
        ("замена: петля, столб, пест, секира".into(), none(&["rope", "stake", "pestle", "axe"])),
        ("утро: монета, удушающая цепь, капкан".into(), none(&["coin", "ravenous", "trap"])),
    ];
    for (i, id) in START_EVENT_IDS.iter().enumerate() {
        cases.push((format!("старт: {id}"), Scenario { start: Some(i as i32), ..MIX }));
    }
    println!("| Сценарий | Случайные | Жадная | Ручные веса | Настроенные |");
    println!("|---|---|---|---|---|");
    for (name, sc) in &cases {
        let r = mean(&scores(Kind::Random, &hand, sc, seed, runs));
        let g = mean(&scores(Kind::Greedy, &hand, sc, seed, runs));
        let h = mean(&scores(Kind::Policy, &hand, sc, seed, runs));
        let t = mean(&scores(Kind::Policy, default_weights(), sc, seed, runs));
        println!("| {name} | {r:.2} | {g:.2} | {h:.2} | {t:.2} |");
    }
}
