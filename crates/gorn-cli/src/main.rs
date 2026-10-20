//! gorn-cli — самоигра, замеры и сверка тюрьмы с эталоном на TS.
//!
//!   gorn-cli bench [--selfplay N] [--threads T] [--ts] [--tli ПУТЬ]
//!   gorn-cli selfplay [--seeds N] [--from K] [--mode instant|random] [--threads T]
//!   gorn-cli verify [--seeds N] [--random N] [--full N] [--full-random N] [--rng N]
//!                   [--from K] [--jobs J] [--quick] [--tli ПУТЬ]
//!
//! --threads: 1 — один поток (по умолчанию для selfplay), 0 — все ядра.

mod bench;
mod reference;
mod verify;

use std::collections::HashMap;
use std::process::ExitCode;

use gorn_prison::runs::Mode;

const USAGE: &str = "\
gorn-cli bench [--selfplay N=200000] [--threads T=0] [--ts] [--tli ../gorn-tli]
    замеры: случайные доигрывания, доигрывание стратегией, мгновенный совет,
    самоигра (один поток и T потоков, 0 — все ядра); --ts — затем то же в эталоне TS
gorn-cli selfplay [--seeds N=200000] [--from K=0] [--mode instant|random] [--threads T=1]
gorn-cli verify [--seeds N=1000000] [--random N=--seeds] [--full N=10000] [--full-random N=--full]
                [--rng N=100000] [--from K=0] [--jobs J=ядра] [--quick] [--tli ../gorn-tli]
    сверка с эталоном: веса, ГСЧ, забеги по хешу и целиком";

fn parse(args: &[String]) -> Result<HashMap<String, String>, String> {
    let mut m = HashMap::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        let key = a.strip_prefix("--").ok_or_else(|| format!("непонятный аргумент {a}"))?;
        let flag = matches!(key, "ts" | "quick");
        if flag {
            m.insert(key.to_string(), "1".to_string());
            i += 1;
        } else {
            let v = args.get(i + 1).ok_or_else(|| format!("--{key}: нет значения"))?;
            m.insert(key.to_string(), v.clone());
            i += 2;
        }
    }
    Ok(m)
}

fn num(m: &HashMap<String, String>, k: &str, d: u32) -> Result<u32, String> {
    match m.get(k) {
        None => Ok(d),
        Some(v) => v.replace('_', "").parse().map_err(|_| format!("--{k}: не число «{v}»")),
    }
}

fn cores() -> u32 {
    std::thread::available_parallelism().map(|n| n.get() as u32).unwrap_or(4)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = args.first() else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let res = (|| -> Result<bool, String> {
        let o = parse(&args[1..])?;
        match cmd.as_str() {
            "bench" => {
                let n = num(&o, "selfplay", 200_000)?;
                bench::bench(n, num(&o, "threads", 0)? as usize);
                if o.contains_key("ts") {
                    let rf = reference::Reference::locate(o.get("tli").map(String::as_str))?;
                    println!("— эталон TS ({}) —", rf.tli.display());
                    let mut child = rf.spawn(&["bench".into(), n.to_string()])?;
                    let mut out = child.stdout.take().unwrap();
                    std::io::copy(&mut out, &mut std::io::stdout()).map_err(|e| e.to_string())?;
                    child.wait().map_err(|e| e.to_string())?;
                }
                Ok(true)
            }
            "selfplay" => {
                let mode = match o.get("mode") {
                    None => Mode::Instant,
                    Some(m) => Mode::parse(m).ok_or_else(|| format!("--mode: {m}"))?,
                };
                bench::selfplay(mode, num(&o, "from", 0)?, num(&o, "seeds", 200_000)?, num(&o, "threads", 1)? as usize);
                Ok(true)
            }
            "verify" => {
                let quick = o.contains_key("quick");
                let seeds = num(&o, "seeds", if quick { 20_000 } else { 1_000_000 })?;
                let full = num(&o, "full", if quick { 1_000 } else { 10_000 })?;
                verify::run(&verify::Options {
                    tli: o.get("tli").cloned(),
                    seeds,
                    random: num(&o, "random", seeds)?,
                    full,
                    full_random: num(&o, "full-random", full)?,
                    rng: num(&o, "rng", if quick { 10_000 } else { 100_000 })?,
                    jobs: num(&o, "jobs", cores())?,
                    from: num(&o, "from", 0)?,
                })
            }
            "help" | "--help" | "-h" => {
                println!("{USAGE}");
                Ok(true)
            }
            _ => Err(format!("неизвестная команда {cmd}\n{USAGE}")),
        }
    })();
    match res {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("ошибка: {e}");
            ExitCode::from(2)
        }
    }
}
