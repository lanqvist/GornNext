//! `gorn-cli debug-trace <state.json> <seed> <rounds>`: раунды решателя ход за
//! ходом — для разбора расхождений с эталоном (тот же вывод, что у трассировки на TS).

use gorn_prison::actions::{apply_code, apply_trusted, legal_codes, CodeBuf};
use gorn_prison::policy::{default_weights, Instant};
use gorn_prison::rng::{mix_pair as mix, Rng};
use gorn_prison::solver::redeterminize;
use gorn_prison::state::{State, DAY, OBJ_DONE, OBJ_PROGRESS, PHASE, P_END, STORAGE};
use gorn_prison::DEFAULT_PARAMS;

pub fn run(file: &str, seed0: u32, rounds: u32) -> Result<(), String> {
    let text = std::fs::read_to_string(file).map_err(|e| e.to_string())?;
    let arr: Vec<i32> = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let mut s0 = State::new();
    s0.m.copy_from_slice(&arr);
    let p = &DEFAULT_PARAMS;
    let w = default_weights();
    let mut buf: CodeBuf = [0; 32];
    let k = legal_codes(&s0, &mut buf);
    let mut inst = Instant::new();
    for n in 0..rounds {
        let seed = mix(seed0, n);
        let mut xs = vec![];
        for &first in &buf[..k] {
            let mut wk = s0;
            let mut det = Rng::new(seed);
            redeterminize(&mut wk, &mut det, false, p);
            let mut rng = Rng::day(mix(seed, 0x5bd1_e995), seed);
            apply_code(&mut wk, first, &mut rng, p).map_err(|e| e.to_string())?;
            let (mut day, mut kk) = (-1, 0);
            let mut tr = vec![first.to_string()];
            while wk.m[PHASE] != P_END {
                if wk.m[DAY] != day {
                    day = wk.m[DAY];
                    kk = 0;
                }
                if let Ok(spec) = std::env::var("GORN_DUMP") {
                    // «раунд,ход,шаг»: позиция перед этим решением — в stderr
                    let v: Vec<usize> = spec.split(',').filter_map(|x| x.parse().ok()).collect();
                    let ci = buf[..k].iter().position(|&x| x == first).unwrap();
                    if v.len() == 3 && v[0] == n as usize && v[1] == ci && v[2] == tr.len() {
                        eprintln!("{:?}", &wk.m[..]);
                    }
                }
                let c = inst.code(&wk, p, w);
                rng.seed(mix(seed, ((day << 12) | (wk.m[PHASE] << 8) | kk) as u32));
                kk += 1;
                apply_trusted(&mut wk, c, &mut rng, p);
                tr.push(c.to_string());
            }
            xs.push(format!("{}/{}/{}:{}", wk.m[STORAGE], wk.m[OBJ_DONE], wk.m[OBJ_PROGRESS], tr.join(",")));
        }
        println!("{n} {}", xs.join(" "));
    }
    Ok(())
}

/// Оценки подарков и ход стратегии на позиции из файла.
pub fn gifts(file: &str) -> Result<(), String> {
    let text = std::fs::read_to_string(file).map_err(|e| e.to_string())?;
    let arr: Vec<i32> = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let mut s = State::new();
    s.m.copy_from_slice(&arr);
    let mut inst = Instant::new();
    for v in inst.gift_values(&s, &DEFAULT_PARAMS, default_weights()) {
        println!("{v} {:x}", v.to_bits());
    }
    println!("code {}", inst.code(&s, &DEFAULT_PARAMS, default_weights()));
    Ok(())
}

/// `gorn-cli debug-move <state.json> <код> <n> [камеры через запятую]`: ход из
/// позиции n раз (скрытое — заново, как у `learn`) и распределение исходов:
/// камеры (зверь/редкость/замок), фаза, день, призмы.
pub fn moves(file: &str, code: i32, n: u32, cells: &[usize]) -> Result<(), String> {
    use gorn_prison::state::{BEAST, LOCK, TIER};
    let text = std::fs::read_to_string(file).map_err(|e| e.to_string())?;
    let arr: Vec<i32> = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let mut s0 = State::new();
    s0.m.copy_from_slice(&arr);
    let p = &DEFAULT_PARAMS;
    let mut hist: std::collections::BTreeMap<String, u32> = Default::default();
    for k in 0..n {
        let mut s = s0;
        let mut r = Rng::new(mix(k * 7919 + 1, 0x51ed));
        redeterminize(&mut s, &mut r, false, p);
        apply_code(&mut s, code, &mut r, p).map_err(|e| e.to_string())?;
        let mut key = format!("ф{} д{} п{}", s.m[PHASE], s.m[DAY], s.m[STORAGE]);
        for &c in cells {
            key += &format!(" {c}:{}/{}/{}", s.m[BEAST + c], s.m[TIER + c], s.m[LOCK + c]);
        }
        *hist.entry(key).or_default() += 1;
    }
    let mut v: Vec<_> = hist.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    for (k, c) in v {
        println!("{:>7.3} %  {k}", c as f64 * 100.0 / n as f64);
    }
    Ok(())
}
