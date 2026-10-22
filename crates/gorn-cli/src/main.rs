//! gorn-cli — самоигра, замеры и сверка тюрьмы с эталоном на TS.
//!
//!   gorn-cli bench [--selfplay N] [--threads T] [--ts] [--tli ПУТЬ]
//!   gorn-cli selfplay [--seeds N] [--from K] [--mode instant|random] [--threads T]
//!   gorn-cli verify [--seeds N] [--random N] [--full N] [--full-random N] [--rng N]
//!                   [--from K] [--jobs J] [--quick] [--tli ПУТЬ]
//!
//! --threads: 1 — один поток (по умолчанию для selfplay), 0 — все ядра.

mod audit;
mod bench;
mod dbcheck;
mod enginecheck;
mod facts;
mod fit;
mod learn;
mod liveab;
mod debugtrace;
mod frames;
mod genlog;
mod reference;
mod replay;
mod rulescheck;
mod report;
mod solverbench;
mod tailcheck;
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
              [--threads T=1] [--solver-threads S=1] [--quiet] [--model new|old]
    самоигра с решателем (сиды 777000+g), как prison-solve-bench --play; fixed — без срока;
    --model old — решатель считает по прежней модели правил (замена завершает день, «Чёрный список»
    после открытия двери), игра — по нынешней
gorn-cli fit [--db a.db,b.db] [--samples N=2000] [--iters K=3] [--out params-learned.json] [--synth N]
    подгонка шансов движка по журналу фактов: апостериорные счёты бросков в розыгрышах, давших
    ровно наблюдённое (EM); --synth — забеги по известным шансам, проверка, что fit их находит
gorn-cli rules-check [--db a.db,b.db] [--samples N=3000] [--synth N --truth вариант,…]
    варианты «[догадок]» правил против журнала фактов: сумма ln отношения правдоподобий
    переходов (те же случайные числа); --synth — забеги по варианту-истине, проверка
gorn-cli live-ab [--db a.db,b.db] [--rounds R=300] [--ref-rounds RR=5000] [--threads T=ядра]
    A/B моделей правил на позициях живых забегов: ход прежней и нынешней модели (R раундов),
    расхождения — глубокой оценкой по нынешним правилам (RR раундов, без отсечения)
gorn-cli solve-bench [--games 12] [--every 2] [--positions N] [--min 256] [--max R] [--cap 300]
              [--threads S=1] [--rate [--stride 5]] [--regret [--ref файл] [--ref-rounds 20000] [--ref-threads 14]]
    советы на позициях самоигры, как prison-solve-bench; 580 позиций аудита: --games 18 --positions 580
gorn-cli tune [--runs 6000] [--passes 4] [--from hand] [--out weights-tuned.json] [--threads T=0]
gorn-cli tune --report [--runs 20000] [--threads T=0]
    настройка весов мгновенной стратегии самоигрой, как prison-tune.ts
gorn-cli replay ПАПКА [--budget 1000] [--from ЧЧММСС] [--to ЧЧММСС] [--rounds R] [--diffs файл.md] [--json]
    свой путь подсказок на сохранённых кадрах против прежнего, как prison-replay.ts;
    --rounds — решатель без срока (детерминированно), --json — строка на кадр
gorn-cli audit [--new] [--days 2] [--budget 600] [--quiet-min 10] [--run ЧЧММСС] [--shots ПАПКА] [--out ПАПКА] [--no-keep]
    аудит забегов по кадрам Горна, как prison-audit.ts: отчёты в gorn-prison-data/audits
gorn-cli verify [--seeds N=1000000] [--random N=--seeds] [--full N=10000] [--full-random N=--full]
                [--rng N=100000] [--solve N=580] [--solve-rounds R=400] [--solve-pool T=4]
                [--play N=32] [--play-rounds R=300] [--tune N=20000]
                [--frames ПАПКА|data] [--frames-max N]
                [--audit ПАПКА|data] [--audit-runs N=2] [--logs ПАПКА|ФАЙЛ|game] [--synth N]
                [--replay ПАПКА|data] [--replay-rounds R=64] [--replay-from ЧЧММСС] [--replay-to ЧЧММСС]
                [--from K=0] [--jobs J=ядра] [--quick] [--tli ../gorn-tli]
    сверка с эталоном: веса, ГСЧ, забеги по хешу и целиком, решатель";

fn parse(args: &[String]) -> Result<HashMap<String, String>, String> {
    let mut m = HashMap::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        let Some(key) = a.strip_prefix("--") else {
            // Позиционный аргумент (папка у replay) — под ключом «_».
            if m.contains_key("_") {
                return Err(format!("непонятный аргумент {a}"));
            }
            m.insert("_".to_string(), a.clone());
            i += 1;
            continue;
        };
        let flag = matches!(key, "ts" | "quick" | "rate" | "regret" | "quiet" | "report" | "json" | "new" | "no-keep" | "no-chain");
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
        let o = if cmd.starts_with("debug-") { Default::default() } else { parse(&args[1..])? };
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
            "report" => {
                let dir = o.get("_").cloned().or_else(|| gorn_core::locate::find_log_directory().map(|d| d.to_string_lossy().to_string()));
                let dir = dir.ok_or("папка логов игры не найдена")?;
                let v = report::build(std::path::Path::new(&dir))?;
                println!("{}", if o.contains_key("json") { v.to_string() } else { serde_json::to_string_pretty(&v).unwrap() });
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
                    frames: o.get("frames").cloned(),
                    frames_max: num(&o, "frames-max", if quick { 40 } else { u32::MAX })?,
                    replay: o.get("replay").cloned(),
                    replay_rounds: num(&o, "replay-rounds", 64)?,
                    replay_from: o.get("replay-from").cloned().unwrap_or_else(|| "000000".into()),
                    replay_to: o.get("replay-to").cloned().unwrap_or_else(|| "999999".into()),
                    audit: o.get("audit").cloned(),
                    audit_runs: num(&o, "audit-runs", 2)?,
                    logs: o.get("logs").cloned(),
                    synth: num(&o, "synth", 0)?,
                    report: o.get("retro").cloned(),
                    report_synth: num(&o, "retro-synth", 0)?,
                    db: o.get("db").cloned(),
                    db_synth: num(&o, "db-synth", 0)?,
                    db_file: o.get("db-file").cloned(),
                    tail: num(&o, "tail", 0)?,
                    community: num(&o, "community", 0)?,
                    engine: num(&o, "engine", 0)?,
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
                    match o.get("model").map(String::as_str) {
                        None | Some("new") => false,
                        Some("old") => true,
                        Some(m) => return Err(format!("--model: {m}")),
                    },
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
            "replay" => {
                let dir = match o.get("_") {
                    Some(d) => std::path::PathBuf::from(d),
                    None => return Err("replay: папка кадров (день Горна)".into()),
                };
                replay::report(
                    &dir,
                    o.get("from").map(String::as_str).unwrap_or("000000"),
                    o.get("to").map(String::as_str).unwrap_or("999999"),
                    num(&o, "budget", 1000)? as f64,
                    o.get("rounds").map(|_| num(&o, "rounds", 0)).transpose()?.map(|r| r as u64),
                    o.get("diffs").map(String::as_str),
                    o.contains_key("json"),
                )?;
                Ok(true)
            }
            "facts" => {
                let dir = match o.get("_") {
                    Some(d) => std::path::PathBuf::from(d),
                    None => return Err("facts: папка кадров тюрьмы (день или папка дней)".into()),
                };
                let db = o.get("db").map(std::path::PathBuf::from).unwrap_or_else(|| std::path::PathBuf::from("prison-facts.db"));
                facts::run(
                    &dir,
                    &db,
                    o.get("rounds").map(|_| num(&o, "rounds", 0)).transpose()?.map(|r| r as u64),
                    num(&o, "budget", 1500)? as f64,
                    !o.contains_key("no-chain"),
                )?;
                Ok(true)
            }
            "learn" => {
                // По умолчанию — живой журнал Горна Next и история по кадрам.
                let dbs: Vec<std::path::PathBuf> = match o.get("db") {
                    Some(list) => list.split(',').map(std::path::PathBuf::from).collect(),
                    None => {
                        let d = std::path::PathBuf::from(std::env::var("APPDATA").unwrap_or_default()).join("com.gorn.next");
                        vec![d.join("prison-facts-history.db"), d.join("prison-facts.db")]
                    }
                };
                learn::run(
                    &dbs,
                    o.contains_key("json"),
                    o.get("detail").map(String::as_str),
                    o.get("gift-chains").map(|_| num(&o, "gift-chains", 2000)).transpose()?.map(u64::from),
                    num(&o, "samples", learn::SAMPLES)?,
                )?;
                Ok(true)
            }
            "debug-gifts" => {
                debugtrace::gifts(args.get(1).ok_or("файл")?)?;
                Ok(true)
            }
            "debug-trace" => {
                let a: Vec<&String> = args[1..].iter().collect();
                if a.len() < 3 {
                    return Err("debug-trace <state.json> <seed> <rounds>".into());
                }
                debugtrace::run(a[0], a[1].parse().map_err(|_| "seed")?, a[2].parse().map_err(|_| "rounds")?)?;
                Ok(true)
            }
            "fit" => {
                let dbs: Vec<std::path::PathBuf> = match o.get("db") {
                    Some(list) => list.split(',').map(std::path::PathBuf::from).collect(),
                    None => {
                        let d = std::path::PathBuf::from(std::env::var("APPDATA").unwrap_or_default()).join("com.gorn.next");
                        vec![d.join("prison-facts-history.db"), d.join("prison-facts.db")]
                    }
                };
                fit::run(
                    &dbs,
                    num(&o, "samples", 2000)?,
                    num(&o, "iters", 3)?,
                    o.get("out").map(String::as_str),
                    o.get("synth").map(|_| num(&o, "synth", 200)).transpose()?,
                )?;
                Ok(true)
            }
            "rules-check" => {
                let dbs: Vec<std::path::PathBuf> = match o.get("db") {
                    Some(list) => list.split(',').map(std::path::PathBuf::from).collect(),
                    None => {
                        let d = std::path::PathBuf::from(std::env::var("APPDATA").unwrap_or_default()).join("com.gorn.next");
                        vec![d.join("prison-facts-history.db"), d.join("prison-facts.db")]
                    }
                };
                rulescheck::run(
                    &dbs,
                    num(&o, "samples", 3000)?,
                    o.get("synth").map(|_| num(&o, "synth", 100)).transpose()?,
                    o.get("truth").map(String::as_str),
                )?;
                Ok(true)
            }
            "live-ab" => {
                let dbs: Vec<std::path::PathBuf> = match o.get("db") {
                    Some(list) => list.split(',').map(std::path::PathBuf::from).collect(),
                    None => {
                        let d = std::path::PathBuf::from(std::env::var("APPDATA").unwrap_or_default()).join("com.gorn.next");
                        vec![d.join("prison-facts-history.db"), d.join("prison-facts.db")]
                    }
                };
                liveab::run(&dbs, num(&o, "rounds", 300)? as u64, num(&o, "ref-rounds", 5000)? as u64, num(&o, "threads", cores())? as usize)?;
                Ok(true)
            }
            "debug-move" => {
                let a: Vec<&String> = args[1..].iter().collect();
                if a.len() < 3 {
                    return Err("debug-move <state.json> <код> <n> [камеры]".into());
                }
                let cells: Vec<usize> = a.get(3).map(|s| s.split(',').filter_map(|x| x.parse().ok()).collect()).unwrap_or_default();
                debugtrace::moves(a[0], a[1].parse().map_err(|_| "код")?, a[2].parse().map_err(|_| "n")?, &cells)?;
                Ok(true)
            }
            "audit" => {
                let data = frames::data_dir();
                audit::run(&audit::AuditOpts {
                    only_new: o.contains_key("new"),
                    days: num(&o, "days", 2)? as usize,
                    budget: num(&o, "budget", 600)? as f64,
                    quiet_min: num(&o, "quiet-min", 10)? as f64,
                    only_run: o.get("run").cloned(),
                    shots: o.get("shots").map(std::path::PathBuf::from).unwrap_or_else(audit::default_shots),
                    out: o.get("out").map(std::path::PathBuf::from).unwrap_or_else(|| data.join("audits")),
                    keep: !o.contains_key("no-keep"),
                })?;
                Ok(true)
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
