//! Дочитывание для сверки: сценарий шагов (запись порциями, ротация, старт с
//! разных позиций, файл ещё не создан) и проход своего дочитывателя по нему —
//! так же, как команда `tail` эталона.

use std::io::Write as _;
use std::path::Path;

use gorn_core::tailer::{TailEvent, Tailer, TailerOptions};
use serde_json::{json, Value};

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

/// Сценарий по зерну: шаги [op, arg].
pub fn script(seed: u64, items: &[i64], keys: &[i64]) -> Value {
    let mut x = seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1;
    let mut rnd = move |n: u64| {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x % n.max(1)
    };
    let log = crate::genlog::generate(seed, 1200, items, keys);
    let mut steps = vec![];
    let mut at = 0usize;
    let take = |n: usize, at: &mut usize| -> Vec<u8> {
        let end = (*at + n).min(log.len());
        let v = log[*at..end].to_vec();
        *at = end;
        v
    };
    if seed % 5 == 0 {
        steps.push(json!(["remove", null]));
    } else {
        let first = rnd(log.len() as u64 / 3) as usize;
        steps.push(json!(["truncate", hex(&take(first, &mut at))]));
    }
    let start = match seed % 4 {
        0 => json!({}),
        1 => json!({ "fromStart": true }),
        2 => json!({ "initialPosition": rnd(at as u64 + 1) }),
        _ => json!({}),
    };
    steps.push(json!(["start", start]));
    let mut written = at;
    while at < log.len() {
        let n = match rnd(6) {
            0 => 1 + rnd(3) as usize,
            1..=3 => 1 + rnd(3000) as usize,
            _ => 1 + rnd(20_000) as usize,
        };
        let part = take(n, &mut at);
        written += part.len();
        steps.push(json!(["append", hex(&part)]));
        match rnd(10) {
            0 => {}
            1 => {
                steps.push(json!(["pump", null]));
                steps.push(json!(["pump", null]));
            }
            2 if written > 2000 => {
                // Ротация: новый файл короче прочитанного.
                let k = rnd(1500) as usize;
                let part = take(k, &mut at);
                written = part.len();
                steps.push(json!(["truncate", hex(&part)]));
                steps.push(json!(["pump", null]));
            }
            _ => steps.push(json!(["pump", null])),
        }
    }
    // Порция больше предела одного прохода.
    if seed % 7 == 3 {
        let mut big = vec![];
        while big.len() < 4 * 1024 * 1024 + 12_345 {
            big.extend_from_slice(&log);
        }
        steps.push(json!(["append", hex(&big)]));
        for _ in 0..3 {
            steps.push(json!(["pump", null]));
        }
    }
    steps.push(json!(["pump", null]));
    Value::Array(steps)
}

/// Свой проход по сценарию: события как у эталона.
pub fn run(steps: &Value, file: &Path) -> Result<Value, String> {
    let mut events = vec![];
    let mut t: Option<Tailer> = None;
    let push = |events: &mut Vec<Value>, evs: Vec<TailEvent>| {
        for e in evs {
            events.push(match e {
                TailEvent::Lines(l) => json!(["lines", l]),
                TailEvent::Rotate => json!(["rotate"]),
                TailEvent::Error(_) => json!(["error"]),
            });
        }
    };
    for step in steps.as_array().ok_or("шаги")? {
        let (op, arg) = (step[0].as_str().unwrap_or(""), &step[1]);
        match op {
            "append" => {
                let mut f = std::fs::OpenOptions::new().create(true).append(true).open(file).map_err(|e| e.to_string())?;
                f.write_all(&unhex(arg.as_str().unwrap_or(""))).map_err(|e| e.to_string())?;
            }
            "truncate" => std::fs::write(file, unhex(arg.as_str().unwrap_or(""))).map_err(|e| e.to_string())?,
            "remove" => {
                let _ = std::fs::remove_file(file);
            }
            "start" => {
                let tl = Tailer::new(
                    file,
                    TailerOptions { from_start: arg["fromStart"].as_bool().unwrap_or(false), initial_position: arg["initialPosition"].as_u64() },
                );
                events.push(json!(["pos", tl.position()]));
                t = Some(tl);
            }
            "pump" => {
                let tl = t.as_mut().ok_or("pump до start")?;
                let evs = tl.pump();
                push(&mut events, evs);
                events.push(json!(["pos", tl.position()]));
            }
            _ => return Err(format!("шаг {op}")),
        }
    }
    Ok(Value::Array(events))
}
