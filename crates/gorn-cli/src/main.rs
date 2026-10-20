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
mod solverbench;
mod tune;
mod verify;

use std::collections::HashMap;
use std::process::ExitCode;

use gorn_prison::runs::Mode;

const USAGE: &str = "\
gorn-cli bench [--selfplay N=200000] [--threads T=0] [--ts] [--tli ../gorn-tli]
    замеры: случайные доигрывания, доигрывание стратегией, мгновенный совет,
    самоигра (один поток и T потоков, 0 — все ядра); --ts — затем то же в эталоне TS
gorn-cli selfplay [--seeds N=200000] [--from K=0] [--mode instant|random] [--threads T=1]
gorn-cli play [--seeds N=800] [--from K=0] [--mode new|flat|old|fixed] [--rounds R=400]
              [--threads T=1] [--solver-threads S=1] [--quiet]
    самоигра с решателем (сиды 777000+g), как prison-solve-bench --play; fixed — без срока
gorn-cli solve-bench [--games 12] [--every 2] [--positions N] [--min 256] [--max R] [--cap 300]
              [--threads S=1] [--rate [--stride 5]] [--regret [--ref файл] [--ref-rounds 20000] [--ref-threads 14]]
    советы на позициях самоигры, как prison-solve-bench; 580 позиций аудита: --games 18 --positions 580
gorn-cli tune [--runs 6000] [--passes 4] [--from hand] [--out weights-tuned.json] [--threads T=0]
gorn-cli tune --report [--runs 20000] [--threads T=0]
    настройка весов мгновенной стратегии самоигрой, как prison-tune.ts
gorn-cli verify [--seeds N=1000000] [--random N=--seeds] [--full N=10000] [--full-random N=--full]
                [--rng N=100000] [--solve N=580] [--solve-rounds R=400] [--solve-pool T=4]
                [--play N=32] [--play-rounds R=300] [--tune N=20000]
                [--from K=0] [--jobs J=ядра] [--quick] [--tli ../gorn-tli]
    сверка с эталоном: веса, ГСЧ, забеги по хешу и целиком, решатель";

fn parse(args: &[String]) -> Result<HashMap<String, String>, String> {
    let mut m = HashMap::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        let key = a.strip_prefix("--").ok_or_else(|| format!("непонятный аргумент {a}"))?;
        let flag = matches!(key, "ts" | "quick" | "rate" | "regret" | "quiet" | "report");
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
                    solve: num(&o, "solve", if quick { 40 } else { 580 })?,
                    solve_rounds: num(&o, "solve-rounds", 400)?,
                    solve_pool: num(&o, "solve-pool", 4)?,
                    play: num(&o, "play", if quick { 2 } else { 32 })?,
                    play_rounds: num(&o, "play-rounds", 300)?,
                    tune: num(&o, "tune", if quick { 2000 } else { 20_000 })?,
                })
            }
            "play" => {
                let mode = match o.get("mode").map(String::as_str).unwrap_or("new") {
                    "new" => solverbench::PlayMode::New,
                    "flat" => solverbench::PlayMode::Flat,
                    "old" => solverbench::PlayMode::Old,
                    "fixed" => solverbench::PlayMode::Fixed(num(&o, "rounds", 400)? as u64),
                    m => return Err(format!("--mode: {m}")),
                };
                solverbench::play(
                    num(&o, "from", 0)?,
                    num(&o, "seeds", 800)?,
                    mode,
                    num(&o, "threads", 1)? as usize,
                    num(&o, "solver-threads", 1)? as usize,
                    o.contains_key("quiet"),
                );
                Ok(true)
            }
            "solve-bench" => {
                let max = match o.get("max") {
                    None => u64::MAX,
                    Some(_) => num(&o, "max", 0)? as u64,
                };
                solverbench::solve_bench(&solverbench::BenchOpts {
                    games: num(&o, "games", 12)?,
                    every: num(&o, "every", 2)?,
                    positions: num(&o, "positions", u32::MAX)? as usize,
                    min_rounds: num(&o, "min", 256)? as u64,
                    max_rounds: max,
                    cap_ms: num(&o, "cap", 300)? as f64,
                    threads: num(&o, "threads", 1)? as usize,
                    rate: o.contains_key("rate").then(|| num(&o, "stride", 5)).transpose()?.map(|x| x as usize),
                    regret: if o.contains_key("regret") {
                        Some(solverbench::RegretOpts {
                            file: o.get("ref").cloned().unwrap_or_else(|| "prison-solve-ref.json".into()),
                            rounds: num(&o, "ref-rounds", 20_000)? as u64,
                            threads: num(&o, "ref-threads", 14)? as usize,
                        })
                    } else {
                        None
                    },
                })?;
                Ok(true)
            }
            "tune" => {
                let threads = num(&o, "threads", 0)? as usize;
                let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build().map_err(|e| e.to_string())?;
                pool.install(|| {
                    if o.contains_key("report") {
                        tune::report(num(&o, "runs", 20_000)?);
                        Ok(true)
                    } else {
                        tune::tune(
                            num(&o, "runs", 6000)?,
                            num(&o, "passes", 4)?,
                            o.get("from").map(String::as_str) == Some("hand"),
                            o.get("out").map(String::as_str).unwrap_or("weights-tuned.json"),
                        )?;
                        Ok(true)
                    }
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
