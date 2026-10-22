//! `gorn-cli fit`: подгонка шансов движка по живым забегам (этап 8, шаг 2).
//!
//! Пара соседних фактов — позиция до и что вышло. Каждый ход из позиции
//! «до» разыгрывается движком (скрытое — заново), броски записываются
//! (`gorn_prison::trace`); остаются розыгрыши, давшие ровно наблюдённое, —
//! по ним видно, какие броски привели к исходу. Их доли — апостериорные
//! счёты: редкость за открытой дверью и на замене, сработало ли орудие.
//! Новые шансы — счёты плюс слабый априор (прежние значения), и всё ещё раз
//! (EM: розыгрыши следующего круга — по уточнённым шансам).
//!
//! Броски, не влияющие на наблюдаемое, апостериорно равны априору и тянут к
//! прежнему значению; редкость считается только без нижней границы (без Крюка,
//! Петли и событий «не ниже Легендарной»).

use std::collections::BTreeMap;
use std::path::PathBuf;

use gorn_prison::actions::{apply_code, legal_codes, start_run, CodeBuf, RunOptions};
use gorn_prison::model::TierOdds;
use gorn_prison::policy::{default_weights, Instant};
use gorn_prison::rng::{mix_pair, Rng};
use gorn_prison::solver::redeterminize;
use gorn_prison::state::{State, LOCK, PHASE, P_END};
use gorn_prison::trace::{self, What};
use gorn_prison::{Params, DEFAULT_PARAMS};
use rayon::prelude::*;
use serde_json::{json, Map, Value};

use crate::learn::{diff, load, visible};

/// Сила априора: прежнее значение весит как столько наблюдений.
const PRIOR: f64 = 10.0;

/// Шансы, которые пишет движок (имена полей `Params`).
const CHANCES: [&str; 19] = [
    "appetite_chance",
    "cage_chance",
    "coin_chance",
    "collar_chance",
    "dogtag_chance",
    "door_event",
    "fire_chance",
    "forged_chance",
    "keys_chance",
    "one_less_door",
    "ravenous_chance",
    "relock_event",
    "revenge_chance",
    "saw_chance",
    "scales_chance",
    "shackles_chance",
    "shortchain_chance",
    "stake_chance",
    "whip_chance",
];

fn chance_mut<'a>(p: &'a mut Params, name: &str) -> &'a mut f64 {
    match name {
        "appetite_chance" => &mut p.appetite_chance,
        "cage_chance" => &mut p.cage_chance,
        "coin_chance" => &mut p.coin_chance,
        "collar_chance" => &mut p.collar_chance,
        "dogtag_chance" => &mut p.dogtag_chance,
        "door_event" => &mut p.door_event,
        "fire_chance" => &mut p.fire_chance,
        "forged_chance" => &mut p.forged_chance,
        "keys_chance" => &mut p.keys_chance,
        "one_less_door" => &mut p.one_less_door,
        "ravenous_chance" => &mut p.ravenous_chance,
        "relock_event" => &mut p.relock_event,
        "revenge_chance" => &mut p.revenge_chance,
        "saw_chance" => &mut p.saw_chance,
        "scales_chance" => &mut p.scales_chance,
        "shackles_chance" => &mut p.shackles_chance,
        "shortchain_chance" => &mut p.shortchain_chance,
        "stake_chance" => &mut p.stake_chance,
        "whip_chance" => &mut p.whip_chance,
        _ => panic!("fit: неизвестный шанс {name}"),
    }
}

fn chance_of(p: &Params, name: &str) -> f64 {
    *chance_mut(&mut p.clone(), name)
}

/// Переход: позиция до, что вышло, ходы-кандидаты; забег и время факта «до».
pub(crate) struct Pair {
    pub(crate) a: State,
    pub(crate) b: State,
    pub(crate) codes: Vec<i32>,
    pub(crate) run: String,
    pub(crate) ts: i64,
}

/// Апостериорные счёты.
#[derive(Default, Clone)]
struct Counts {
    door: [f64; 6],
    replace: [f64; 6],
    /// Шанс: (сработал, бросков).
    chance: BTreeMap<&'static str, (f64, f64)>,
    used: usize,
    unexplained: usize,
}

impl Counts {
    fn merge(mut self, o: Counts) -> Counts {
        for t in 0..6 {
            self.door[t] += o.door[t];
            self.replace[t] += o.replace[t];
        }
        for (k, (s, n)) in o.chance {
            let e = self.chance.entry(k).or_default();
            e.0 += s;
            e.1 += n;
        }
        self.used += o.used;
        self.unexplained += o.unexplained;
        self
    }
}

pub(crate) fn pairs_from_facts(dbs: &[PathBuf]) -> Result<Vec<Pair>, String> {
    let mut facts = vec![];
    for db in dbs.iter().filter(|d| d.exists()) {
        facts.extend(load(db)?);
    }
    facts.dedup_by(|b, a| a.run == b.run && a.key == b.key);
    let mut out = vec![];
    for w in facts.windows(2) {
        let (a, b) = (&w[0], &w[1]);
        if a.run != b.run || a.settled == Some(false) || b.settled == Some(false) {
            continue;
        }
        out.push(Pair { a: a.state, b: b.state, codes: a.options.iter().map(|o| o.0).collect(), run: a.run.clone(), ts: a.ts });
    }
    Ok(out)
}

/// Броски розыгрышей хода, давших ровно наблюдённое (лучший по совпадениям ход).
fn observe(pr: &Pair, p: &Params, samples: u32, seed: u32, fields: &[usize]) -> Counts {
    let mut best: Vec<Vec<trace::Draw>> = vec![];
    for (i, &code) in pr.codes.iter().enumerate() {
        let mut hits = vec![];
        for k in 0..samples {
            let mut r = Rng::new(mix_pair(k.wrapping_mul(7919).wrapping_add(1), (i as u32) << 16 ^ seed));
            let mut s = pr.a;
            trace::start();
            redeterminize(&mut s, &mut r, false, p);
            let ok = apply_code(&mut s, code, &mut r, p).is_ok();
            let draws = trace::take();
            if !ok {
                break;
            }
            if diff(&s, &pr.b, fields).is_empty() {
                hits.push(draws);
            }
        }
        if hits.len() > best.len() {
            best = hits;
        }
    }
    let mut c = Counts::default();
    if best.is_empty() {
        c.unexplained = 1;
        return c;
    }
    c.used = 1;
    let w = 1.0 / best.len() as f64;
    for draws in &best {
        // Дверь — последний бросок по камере, открытой в этом переходе.
        let mut door: BTreeMap<i32, (i32, i32)> = BTreeMap::new();
        for d in draws {
            match d.what {
                What::Door => {
                    door.insert(d.cell, (d.value, d.floor));
                }
                What::Replace if d.floor == 0 && (0..6).contains(&d.value) => c.replace[d.value as usize] += w,
                What::Replace => {}
                What::Chance(name) => {
                    let e = c.chance.entry(name).or_default();
                    e.0 += w * d.value as f64;
                    e.1 += w;
                }
            }
        }
        for (cell, (t, floor)) in door {
            let cu = cell as usize;
            if floor == 0 && (0..6).contains(&t) && pr.a.m[LOCK + cu] == 1 && pr.b.m[LOCK + cu] == 0 {
                c.door[t as usize] += w;
            }
        }
    }
    c
}

fn collect(pairs: &[Pair], p: &Params, samples: u32) -> Counts {
    let fields = visible();
    pairs
        .par_iter()
        .enumerate()
        .map(|(i, pr)| observe(pr, p, samples, i as u32, &fields))
        .reduce(Counts::default, Counts::merge)
}

/// Таблица с априором: (счёты + PRIOR · прежние доли) / (n + PRIOR).
fn table(counts: &[f64; 6], prior: &TierOdds) -> TierOdds {
    let n: f64 = counts.iter().sum();
    let mut t = [0.0; 6];
    for i in 0..6 {
        t[i] = (counts[i] + PRIOR * prior[i]) / (n + PRIOR);
    }
    t
}

fn update(c: &Counts, prior: &Params) -> Params {
    let mut p = prior.clone();
    p.door_odds = table(&c.door, &prior.door_odds);
    p.replace_odds = table(&c.replace, &prior.replace_odds);
    for (name, (s, n)) in &c.chance {
        let q = chance_of(prior, name);
        *chance_mut(&mut p, name) = (s + PRIOR * q) / (n + PRIOR);
    }
    p
}

/// Полуширина 95 % интервала доли m при n наблюдений (с априором).
fn ci(m: f64, n: f64) -> f64 {
    1.96 * (m * (1.0 - m) / (n + PRIOR + 1.0)).sqrt()
}

fn report(c: &Counts, base: &Params, fitted: &Params, truth: Option<&Params>) -> Value {
    let mut out = Map::new();
    let tiers = ["обычный", "необычный", "редкий", "эпический", "легендарный", "призматический"];
    for (name, counts, b, f, tr) in [
        ("door_odds", &c.door, &base.door_odds, &fitted.door_odds, truth.map(|t| t.door_odds)),
        ("replace_odds", &c.replace, &base.replace_odds, &fitted.replace_odds, truth.map(|t| t.replace_odds)),
    ] {
        let n: f64 = counts.iter().sum();
        println!("{name}: наблюдений {n:.0}");
        let mut rows = vec![];
        for i in 0..6 {
            let h = ci(f[i], n);
            let shift = if (f[i] - b[i]).abs() > h { "  ← сдвиг" } else { "" };
            let t = tr.map(|t| format!("  (истина {:.3})", t[i])).unwrap_or_default();
            println!("  {:<15} было {:.3}  стало {:.3} ± {:.3}{t}{shift}", tiers[i], b[i], f[i], h);
            rows.push(json!({ "tier": i, "was": b[i], "fit": f[i], "ci": h }));
        }
        out.insert(name.into(), json!({ "n": n, "values": f, "rows": rows }));
    }
    println!("Шансы (бросков — сколько раз разыгрывались в объяснённых переходах):");
    let mut ch = Map::new();
    for name in CHANCES {
        let Some(&(s, n)) = c.chance.get(name) else { continue };
        if n < 1.0 {
            continue;
        }
        let (b, f) = (chance_of(base, name), chance_of(fitted, name));
        let h = ci(f, n);
        let shift = if (f - b).abs() > h { "  ← сдвиг" } else { "" };
        let t = truth.map(|t| format!("  (истина {:.3})", chance_of(t, name))).unwrap_or_default();
        println!("  {name:<18} бросков {n:>6.1}, сработал {s:>6.1}: было {b:.3}  стало {f:.3} ± {h:.3}{t}{shift}");
        ch.insert(name.into(), json!({ "n": n, "was": b, "fit": f, "ci": h }));
    }
    out.insert("chances".into(), Value::Object(ch));
    Value::Object(out)
}

pub fn run(dbs: &[PathBuf], samples: u32, iters: u32, out: Option<&str>, synth: Option<u32>) -> Result<(), String> {
    let base = DEFAULT_PARAMS;
    let truth = synth.map(|_| synth_truth());
    let pairs = match synth {
        Some(n) => synth_pairs(n, truth.as_ref().unwrap()),
        None => pairs_from_facts(dbs)?,
    };
    println!("переходов {}, розыгрышей на ход {samples}, кругов {iters}", pairs.len());
    let mut p = base.clone();
    let mut last = Counts::default();
    for it in 0..iters.max(1) {
        let t0 = std::time::Instant::now();
        last = collect(&pairs, &p, samples);
        p = update(&last, &base);
        println!(
            "  круг {}: объяснено {} из {}, {:.1} с",
            it + 1,
            last.used,
            last.used + last.unexplained,
            t0.elapsed().as_secs_f64()
        );
    }
    println!();
    let v = report(&last, &base, &p, truth.as_ref());
    if let Some(path) = out {
        let doc = json!({
            "pairs": pairs.len(), "explained": last.used, "samples": samples, "iterations": iters,
            "prior": PRIOR, "params": v,
        });
        std::fs::write(path, serde_json::to_string_pretty(&doc).unwrap() + "\n").map_err(|e| format!("{path}: {e}"))?;
        println!("\nзаписано: {path}");
    }
    Ok(())
}

// ── синтетика: известные шансы, проверка, что fit их находит ────────────────

fn synth_truth() -> Params {
    let mut p = DEFAULT_PARAMS;
    p.door_odds = [0.52, 0.30, 0.09, 0.07, 0.02, 0.0];
    p.saw_chance = 0.5;
    p.keys_chance = 0.5;
    p.coin_chance = 0.3;
    p
}

/// Забеги мгновенной стратегией по истинным шансам; каждый шаг — «факт».
pub(crate) fn synth_pairs(n: u32, truth: &Params) -> Vec<Pair> {
    let w = default_weights();
    let mut inst = Instant::new();
    let mut buf: CodeBuf = [0; 32];
    let mut out = vec![];
    for g in 0..n {
        let mut s = State::new();
        let mut r = Rng::new(900_000 + g);
        start_run(&mut s, &mut r, truth, RunOptions { start: (g % 5) as i32, ..RunOptions::default() });
        while s.m[PHASE] != P_END {
            let k = legal_codes(&s, &mut buf);
            let a = s;
            let c = inst.code(&s, truth, w);
            if apply_code(&mut s, c, &mut r, truth).is_err() {
                break;
            }
            out.push(Pair { a, b: s, codes: buf[..k].to_vec(), run: format!("synth#{g}"), ts: out.len() as i64 });
        }
    }
    out
}
