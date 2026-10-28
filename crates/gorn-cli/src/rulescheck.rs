//! `gorn-cli rules-check`: варианты «[догадок]» правил против живых забегов
//! (этап 8, шаг 3).
//!
//! Каждый вариант — переключатель в `Params`. Для пары соседних фактов
//! движок разыгрывает ходы (как `learn`), доля розыгрышей, давших ровно
//! наблюдённое, — правдоподобие перехода. Варианты разыгрываются на тех же
//! случайных числах, что нынешнее толкование: где вариант ни при чём,
//! розыгрыши совпадают в точности, разница — только там, где он что-то
//! меняет. Итог — сумма логарифмов отношения правдоподобий: больше 3 (≈ 20 : 1)
//! — данные за вариант, меньше −3 — за нынешнее толкование.

use std::path::PathBuf;

use gorn_prison::actions::apply_code;
use gorn_prison::rng::{mix_pair, Rng};
use gorn_prison::solver::redeterminize;
use gorn_prison::{Params, DEFAULT_PARAMS};
use rayon::prelude::*;

use crate::fit::{pairs_from_facts, synth_pairs, Pair};
use crate::learn::{diff, visible};

/// Вариант правила: имя, что меняет, как включить.
pub(crate) struct Variant {
    pub(crate) name: &'static str,
    pub(crate) about: &'static str,
    pub(crate) apply: fn(&mut Params),
}

/// Прежние толкования до правок 22.10 — «прежняя модель» для live-ab и play.
pub(crate) const LEGACY: &str = "swap_ends_day,blacklist_after_open,horn_check_on_turn";

/// Нынешние правила с вариантами по именам через запятую. Кроме вариантов
/// правил — `no_red_value`: итог без красных (только призмы, как до 22.10).
pub(crate) fn with_variants(names: &str) -> Result<Params, String> {
    let mut p = DEFAULT_PARAMS;
    for name in names.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        if name == "no_red_value" {
            p.red_value = 0.0;
            continue;
        }
        let v = VARIANTS.iter().find(|v| v.name == name).ok_or(format!("нет варианта {name}"))?;
        (v.apply)(&mut p);
    }
    Ok(p)
}

pub(crate) const VARIANTS: [Variant; 15] = [
    Variant { name: "swap_ends_day", about: "замена кончает день (до 22.10)", apply: |p| p.swap_then_devour = false },
    Variant { name: "blacklist_after_open", about: "«Чёрный список»: запертые после открытия (до 22.10)", apply: |p| p.blacklist_before_open = false },
    Variant { name: "reward_after_move", about: "цель засчитывается после хода надзирателя, «Волнения» — по «Принять» (до вечера 22.10)", apply: |p| p.reward_on_click = false },
    Variant { name: "horn_check_on_turn", about: "«Рог»: цель проверяется в свой ход, по номерам камер (до 22.10)", apply: |p| p.horn_eaters_snapshot = false },
    Variant {
        name: "horn_check_on_turn_random",
        about: "«Рог»: цель проверяется в свой ход, едоки в случайном порядке",
        apply: |p| {
            p.horn_eaters_snapshot = false;
            p.horn_random_order = true;
        },
    },
    Variant { name: "horn_random_order", about: "«Рог»: едоки (с целью до первого укуса) в случайном порядке", apply: |p| p.horn_random_order = true },
    Variant { name: "horn_no_devour_tools", about: "укусы «Рога» не запускают Плеть, Пилу, Деву", apply: |p| p.horn_devour_tools = false },
    Variant { name: "bestow_plus2", about: "«Наградить» — +2, не +1", apply: |p| p.bestow_gain = 2 },
    Variant { name: "devour_tools_first_bite", about: "Плеть, Пила, Дева — только после первого укуса", apply: |p| p.devour_tools_first_bite = true },
    Variant { name: "collar_first_bite", about: "«Ошейник» и на первом укусе", apply: |p| p.collar_first_bite = true },
    Variant { name: "cage_same_tier", about: "«Клетка»: новый узник той же редкости, что съеденный", apply: |p| p.cage_same_tier = true },
    Variant { name: "revenge_door_tier", about: "«Железная дева»: узник по шансам двери, не редкости едока", apply: |p| p.revenge_door_tier = true },
    Variant { name: "ring_hook", about: "«Трёхглавое кольцо» с «Крюком отбора» (до 24.10)", apply: |p| p.ring_hook = true },
    Variant { name: "maiden_new_tier", about: "«Рассечённый саркофаг»: Высшие и Низшие относительно нового узника (до 27.10)", apply: |p| p.maiden_new_tier = true },
    Variant {
        name: "odds_community",
        about: "редкость за дверью и на замену — таблицы сообщества, 7 191 дверь и 696 замен (до 28.10)",
        apply: |p| {
            p.door_odds = [0.418, 0.359, 0.114, 0.092, 0.018, 0.0];
            p.replace_odds = [0.422, 0.333, 0.145, 0.073, 0.023, 0.003];
        },
    },
];

/// Совпадений с наблюдённым у лучшего хода — по каждой паре.
fn hits(pairs: &[Pair], p: &Params, samples: u32) -> Vec<u32> {
    let fields = visible();
    pairs
        .par_iter()
        .enumerate()
        .map(|(n, pr)| {
            let mut best = 0;
            for (i, &code) in pr.codes.iter().enumerate() {
                let mut h = 0;
                for k in 0..samples {
                    let mut r = Rng::new(mix_pair(k.wrapping_mul(7919).wrapping_add(1), (i as u32) << 16 ^ n as u32));
                    let mut s = pr.a;
                    redeterminize(&mut s, &mut r, false, p);
                    if apply_code(&mut s, code, &mut r, p).is_err() {
                        break;
                    }
                    if diff(&s, &pr.b, &fields).is_empty() {
                        h += 1;
                    }
                }
                best = best.max(h);
            }
            best
        })
        .collect()
}

fn ln_p(h: u32, samples: u32) -> f64 {
    ((h as f64 + 0.5) / (samples as f64 + 1.0)).ln()
}

fn when(pr: &Pair) -> String {
    if pr.run.starts_with("synth") {
        return format!("{} шаг {}", pr.run, pr.ts);
    }
    use chrono::TimeZone;
    let t = chrono::Local.timestamp_millis_opt(pr.ts).single().map(|t| t.format("%d.%m %H:%M:%S").to_string()).unwrap_or_default();
    format!("{} {t}", pr.run)
}

pub fn run(dbs: &[PathBuf], samples: u32, synth: Option<u32>, truth: Option<&str>) -> Result<(), String> {
    let pairs = match synth {
        Some(n) => {
            let t = with_variants(truth.unwrap_or(""))?;
            println!("синтетика: {n} забегов, истина — {}", truth.unwrap_or("нынешние правила"));
            synth_pairs(n, &t)
        }
        None => pairs_from_facts(dbs)?,
    };
    let t0 = std::time::Instant::now();
    let base = hits(&pairs, &DEFAULT_PARAMS, samples);
    println!(
        "переходов {}, объяснено нынешними правилами {}, розыгрышей на ход {samples}",
        pairs.len(),
        base.iter().filter(|&&h| h > 0).count()
    );
    println!("Вариант: сумма ln(правдоподобие варианта / нынешнего) — > 3 за вариант, < −3 за нынешнее\n");
    let mut rows = vec![];
    for v in &VARIANTS {
        let mut p = DEFAULT_PARAMS;
        (v.apply)(&mut p);
        let h = hits(&pairs, &p, samples);
        let mut d: Vec<(f64, usize)> = vec![];
        let (mut gained, mut lost) = (0, 0);
        for (i, (&a, &b)) in base.iter().zip(&h).enumerate() {
            if a != b {
                d.push((ln_p(b, samples) - ln_p(a, samples), i));
                if a == 0 {
                    gained += 1;
                }
                if b == 0 {
                    lost += 1;
                }
            }
        }
        let score: f64 = d.iter().map(|x| x.0).sum();
        rows.push((score, v, d, gained, lost));
    }
    rows.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    for (score, v, d, gained, lost) in &rows {
        let verdict = if *score > 3.0 {
            "за вариант"
        } else if *score < -3.0 {
            "за нынешнее"
        } else {
            "не различаются"
        };
        println!(
            "{:<24} {score:+8.1}  {verdict:<15} переходов затронуто {:>4}, стало объяснимо {gained:>3}, стало необъяснимо {lost:>3} — {}",
            v.name,
            d.len(),
            v.about
        );
        let mut ex = d.clone();
        ex.sort_by(|a, b| b.0.abs().partial_cmp(&a.0.abs()).unwrap_or(std::cmp::Ordering::Equal));
        for (x, i) in ex.iter().take(3) {
            let pr = &pairs[*i];
            println!("      {x:+6.2}  {}", when(pr));
        }
    }
    println!("\n{:.1} с", t0.elapsed().as_secs_f64());
    Ok(())
}
