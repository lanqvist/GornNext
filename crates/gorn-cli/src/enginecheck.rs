//! Движок трекера для сверки: сценарий (строки лога, часы, действия интерфейса,
//! срезы) и проход своего движка по нему — как команда `engine` эталона.

use std::path::Path;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use gorn_core::engine::{CatalogueItem, Engine, EngineOptions, NameLanguage};
use gorn_core::labels::Lang;
use gorn_core::mapnames::MapCatalogue;
use serde_json::{json, Value};

fn lang(v: &Value) -> NameLanguage {
    match v.as_str() {
        Some("en") => NameLanguage::En,
        Some("both") => NameLanguage::Both,
        _ => NameLanguage::Ru,
    }
}

/// Строки текста как у дочитывателя: по `\r?\n`, хвост без перевода — тоже строка.
fn lines_of(bytes: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(bytes);
    let mut v: Vec<String> = text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l).to_string()).collect();
    if v.last().is_some_and(|l| l.is_empty()) {
        v.pop();
    }
    v
}

fn ts_of(line: &str) -> Option<i64> {
    gorn_core::parser::parse_timestamp(line)
}

/// Сценарий по зерну. Папку логов готовит вызывающий: `prefix` — уже лежащее в UE_game.log.
pub fn script(seed: u64, log_dir: &Path, live: &[u8], next_log: &[u8], player_ids: &[String]) -> Value {
    let mut x = seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1;
    let mut rnd = move |n: u64| {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x % n.max(1)
    };
    let live = lines_of(live);
    let next = lines_of(next_log);
    let ids: Vec<i64> = live
        .iter()
        .filter_map(|l| l.split("ConfigBaseId = ").nth(1).and_then(|r| r.split_whitespace().next()).and_then(|n| n.parse().ok()))
        .collect();
    let mut steps = vec![json!(["snapshot"])];
    let mut clock = live.iter().find_map(|l| ts_of(l)).unwrap_or(1_790_000_000_000) - 1000;
    let feed = |steps: &mut Vec<Value>, lines: &[String], clock: &mut i64, rnd: &mut dyn FnMut(u64) -> u64| {
        let mut i = 0;
        while i < lines.len() {
            let n = 1 + rnd(120) as usize;
            let batch = &lines[i..(i + n).min(lines.len())];
            i += n;
            if let Some(t) = batch.iter().rev().find_map(|l| ts_of(l)) {
                *clock = (*clock).max(t + rnd(3000) as i64);
            }
            steps.push(json!(["clock", *clock]));
            steps.push(json!(["lines", batch]));
            match rnd(14) {
                0 | 1 | 2 => steps.push(json!(["snapshot"])),
                3 => {
                    let p = if rnd(2) == 0 { Value::Null } else { json!(player_ids[rnd(player_ids.len() as u64) as usize]) };
                    steps.push(json!(["scope", p, rnd(2) == 0]));
                    steps.push(json!(["snapshot"]));
                }
                4 if !ids.is_empty() => {
                    let id = ids[rnd(ids.len() as u64) as usize];
                    steps.push(json!(["exclude", id, true]));
                    steps.push(json!(["snapshot"]));
                    if rnd(2) == 0 {
                        steps.push(json!(["exclude", id, false]));
                    }
                }
                5 => steps.push(json!(["minDrop", rnd(20)])),
                6 => {
                    steps.push(json!(["language", (["en", "both", "ru"][rnd(3) as usize])]));
                    steps.push(json!(["snapshot"]));
                }
                7 => {
                    steps.push(json!(["resetSession"]));
                    steps.push(json!(["snapshot"]));
                }
                8 => steps.push(json!(["queryRuns", (["", "пуст", "a", "Маячок", "beacon", "без вложений"][rnd(6) as usize])])),
                9 => steps.push(json!(["queryPrices", (["", "огн", "a", "Маячок"][rnd(4) as usize])])),
                10 => {
                    // Долгая пауза: подсказка «игра давно ничего не пишет».
                    *clock += 200_000;
                    steps.push(json!(["clock", *clock]));
                    steps.push(json!(["snapshot"]));
                }
                _ => {}
            }
        }
    };
    feed(&mut steps, &live, &mut clock, &mut rnd);
    if seed % 3 == 1 {
        steps.push(json!(["community", false]));
        steps.push(json!(["snapshot"]));
        steps.push(json!(["community", true]));
    }
    steps.push(json!(["csv"]));
    steps.push(json!(["seen"]));
    steps.push(json!(["snapshot"]));
    if !next.is_empty() {
        // Игра перезапущена: лог начался заново.
        steps.push(json!(["rotate"]));
        steps.push(json!(["snapshot"]));
        clock = clock.max(next.iter().find_map(|l| ts_of(l)).unwrap_or(clock));
        feed(&mut steps, &next, &mut clock, &mut rnd);
    }
    steps.push(json!(["queryRuns", ""]));
    steps.push(json!(["queryPrices", ""]));
    steps.push(json!(["snapshot"]));
    steps.push(json!(["stop"]));
    json!({
        "logDir": log_dir.to_string_lossy(),
        "clock": live.iter().find_map(|l| ts_of(l)).unwrap_or(1_790_000_000_000) - 1000,
        "options": {
            "communityPrices": seed % 4 != 3,
            "playerFilter": Value::Null,
            "allSeasons": seed % 5 == 2,
            "excludedItems": if seed % 2 == 0 { json!([]) } else { json!(ids.iter().take(1).collect::<Vec<_>>()) },
            "minDropFe": 0,
            "language": (["ru", "en", "both"][(seed % 3) as usize]),
        },
        "steps": steps,
    })
}

/// Свой проход по сценарию: результаты шагов как у эталона.
pub fn run(script: &Value, data_dir: &Path) -> Result<Value, String> {
    let items = CatalogueItem::parse_list(include_str!("../../gorn-core/data/items.json")).map_err(|e| e.to_string())?;
    let maps: MapCatalogue = serde_json::from_str(include_str!("../../gorn-core/data/maps.json")).map_err(|e| e.to_string())?;
    let now = Arc::new(AtomicI64::new(script["clock"].as_i64().unwrap_or(0)));
    let clock_now = now.clone();
    let o = &script["options"];
    let mut engine = Engine::new(
        data_dir,
        &items,
        EngineOptions {
            community_prices: o["communityPrices"].as_bool().unwrap_or(true),
            player_filter: o["playerFilter"].as_str().map(str::to_string),
            all_seasons: o["allSeasons"].as_bool().unwrap_or(false),
            excluded_items: o["excludedItems"].as_array().map(|a| a.iter().filter_map(|v| v.as_i64()).collect()).unwrap_or_default(),
            min_drop_fe: o["minDropFe"].as_f64().unwrap_or(0.0),
            maps: Some(maps),
            language: lang(&o["language"]),
            ui: Lang::Ru,
            sync_community: true,
            community_url: Some("http://127.0.0.1:9".into()),
            icon_base: None,
        },
        Arc::new(move || clock_now.load(Ordering::SeqCst)),
    )
    .map_err(|e| e.to_string())?;
    let log_dir = std::path::PathBuf::from(script["logDir"].as_str().unwrap_or_default());
    if let Err((_, e)) = engine.start(Some(&log_dir), None) {
        return Err(format!("start: {e}"));
    }
    let mut out = vec![];
    for step in script["steps"].as_array().ok_or("шаги")? {
        let op = step[0].as_str().unwrap_or("");
        let (a, b) = (&step[1], &step[2]);
        match op {
            "clock" => now.store(a.as_i64().unwrap_or(0), Ordering::SeqCst),
            "lines" => {
                let lines: Vec<String> = a.as_array().map(|l| l.iter().filter_map(|s| s.as_str().map(str::to_string)).collect()).unwrap_or_default();
                engine.consume(&lines);
            }
            "rotate" => engine.on_log_rotate(),
            "snapshot" => out.push(json!(["snapshot", engine.build_snapshot()])),
            "scope" => engine.set_run_scope(a.as_str().map(str::to_string), b.as_bool().unwrap_or(false)),
            "exclude" => engine.set_excluded(a.as_i64().unwrap_or(0), b.as_bool().unwrap_or(false)),
            "minDrop" => engine.set_min_drop_fe(a.as_f64().unwrap_or(0.0)),
            "language" => engine.set_language(lang(a)),
            "resetSession" => engine.reset_session(),
            "community" => engine.set_community_prices(a.as_bool().unwrap_or(true)),
            "queryRuns" => out.push(json!(["queryRuns", engine.query_runs(a.as_str().unwrap_or(""))])),
            "queryPrices" => out.push(json!(["queryPrices", engine.query_prices(a.as_str().unwrap_or(""))])),
            "csv" => out.push(json!(["csv", engine.export_runs_csv()])),
            "seen" => out.push(json!(["seen", engine.seen_item_ids()])),
            "stop" => engine.stop(),
            _ => return Err(format!("шаг {op}")),
        }
    }
    Ok(Value::Array(out))
}
