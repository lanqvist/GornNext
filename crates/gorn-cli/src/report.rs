//! Ретро-скан папки логов и вся аналитика над ним в JSON — та же сборка,
//! что у команды `report` эталона (tools/reference.mts), для сверки.

use std::collections::HashSet;
use std::path::Path;

use gorn_core::analysis as a;
use gorn_core::history as h;
use gorn_core::labels::{hero_name, season_name, season_short, Lang};
use gorn_core::mapnames::MapNames;
use gorn_core::prices::{self as p, PricePoint};
use gorn_core::retroscan::{retro_scan, run_cost, run_value};
use gorn_core::stats as s;
use gorn_core::{AnalyzeOptions, Run};
use indexmap::IndexMap;
use serde_json::{json, Value};

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
}

fn choice(c: Option<(f64, p::PriceSource)>) -> Value {
    c.map(|(fe, source)| json!({ "fe": fe, "source": source })).unwrap_or(Value::Null)
}

pub fn build(dir: &Path) -> Result<Value, String> {
    let items: Value = serde_json::from_str(include_str!("../../gorn-core/data/items.json")).map_err(|e| e.to_string())?;
    let items = items.as_array().ok_or("items.json")?;
    let keys: HashSet<i64> = items
        .iter()
        .filter(|it| gorn_core::patterns::is_map_key_type(it["typeCn"].as_str()))
        .filter_map(|it| it["id"].as_i64())
        .collect();
    let is_key = |id: i64| keys.contains(&id);
    let mut r = retro_scan(dir, None, AnalyzeOptions { is_map_key: Some(&is_key), current: false }).map_err(|e| format!("{}: {e}", dir.display()))?;
    let bound: Vec<i64> = items
        .iter()
        .filter(|it| p::is_bound_name(it["name"].as_str()) || p::is_bound_name(it["nameRu"].as_str()))
        .filter_map(|it| it["id"].as_i64())
        .collect();
    r.prices.register_bound_ids(bound.iter().copied());
    let store = &r.prices;
    let price_of = |id: i64| store.price_of(id);
    let pricing = |_: &Run, id: i64| store.price_of(id);
    let catalogue: Value = serde_json::from_str(include_str!("../../gorn-core/data/maps.json")).map_err(|e| e.to_string())?;
    let maps = MapNames::builtin();
    let name_of = |run: &Run| match maps.lookup(run.asset.as_deref(), run.area_id) {
        Some(m) => m.ru.unwrap_or(m.en),
        None => format!("{}#{}", run.asset.as_deref().unwrap_or("?"), run.area_id),
    };

    let runs = &r.runs;
    let farming: Vec<Run> = runs.iter().filter(|r| a::is_farming_run(r)).cloned().collect();
    let rated: Vec<Run> = runs.iter().filter(|r| a::is_rated_run(r)).cloned().collect();
    let nets: Vec<f64> = rated.iter().map(|run| a::run_net(run, &pricing)).collect();
    let rates: Vec<f64> =
        rated.iter().zip(&nets).map(|(run, &n)| s::per_hour(n, a::active_duration(run, a::IDLE_CAP_MS, None) as f64)).collect();
    let now = runs.iter().map(a::run_end).fold(0, i64::max);
    // Сессии для сверки: по семь фарм-забегов, последняя открыта.
    let sessions: Vec<h::SessionBounds> = (0..farming.len())
        .step_by(7)
        .map(|i| h::SessionBounds { started_at: farming[i].started_at, ended_at: farming.get(i + 7).map(|r| r.started_at) })
        .collect();
    let mut contents: IndexMap<i64, i64> = IndexMap::new();
    for (_, st) in &r.slots {
        if let Some(id) = st.config_base_id {
            *contents.entry(id).or_insert(0) += st.qty;
        }
    }
    let mut history: IndexMap<i64, Vec<PricePoint>> = IndexMap::new();
    for o in store.observations() {
        history.entry(o.config_base_id).or_default().push(PricePoint { t: o.observed_at, fe: o.fe });
    }
    let mut choices = vec![];
    for rec in store.all() {
        for k in [-1i64, 0, 1] {
            let own = Some((rec.fe, rec.observed_at));
            let upd = move || if k == 1 { f64::NAN } else { (rec.observed_at + k * 1000) as f64 };
            let later = move || (rec.observed_at + 5000) as f64;
            let t = rec.observed_at + p::OWN_PRICE_TTL_MS + k;
            let ttl = p::OWN_PRICE_TTL_MS;
            choices.push(json!([
                choice(p::choose_price(own, Some((rec.median, &upd)), t, ttl)),
                choice(p::choose_price(own, None, t, ttl)),
                choice(p::choose_price(None, Some((rec.median, &upd)), t, ttl)),
                choice(p::choose_price(own, Some((rec.median, &later)), t, ttl)),
            ]));
        }
        choices.push(json!([
            p::community_reference(rec.median, rec.fe, rec.config_base_id % 2 == 0),
            p::community_reference(rec.fe, rec.median, false),
        ]));
    }
    let mut lookups = vec![];
    let mut seen = HashSet::new();
    for run in runs {
        if seen.insert(format!("{:?}#{}", run.asset, run.area_id)) {
            lookups.push(json!([run.asset, run.area_id, maps.lookup(run.asset.as_deref(), run.area_id)]));
        }
    }
    for pat in catalogue["assets"].as_object().unwrap().keys() {
        let asset = format!("X_{pat}_Y");
        for id in [5354, 506, 1] {
            lookups.push(json!([asset, id, maps.lookup(Some(&asset), id)]));
        }
    }
    for pat in catalogue["ambiguous"].as_object().unwrap().keys() {
        for id in [5354, 506, 5355, 7] {
            lookups.push(json!([pat, id, maps.lookup(Some(pat), id)]));
        }
    }
    for id in catalogue["levelIds"].as_object().unwrap().keys() {
        let id: i64 = id.parse().map_err(|_| format!("levelIds: {id}"))?;
        lookups.push(json!([null, id, maps.lookup(None, id)]));
    }
    let season_ids: Vec<Option<i64>> =
        [None, Some(1), Some(1301), Some(1401), Some(1501), Some(1601), Some(1701), Some(1801), Some(1502), Some(99), Some(2001)]
            .into_iter()
            .chain(r.season_ids.iter().map(|&s| Some(s)))
            .collect();
    let hero_ids: Vec<Option<i64>> =
        [None, Some(1700), Some(1800)].into_iter().chain(r.players.iter().map(|p| p.hero_id.map(|h| h as i64))).collect();
    let mut sorted = rates.clone();
    s::sort_num(&mut sorted);
    let half = runs.len().div_ceil(2);
    let third = runs.len() / 3;
    let top: Vec<Value> = a::top_drops(&rated, &pricing, 5, &[])
        .into_iter()
        .map(|t| {
            json!({ "configBaseId": t.config_base_id, "qty": t.qty, "totalFe": t.total_fe, "at": t.at,
                    "run": format!("{}:{}", t.run_started_at, t.run_area_id) })
        })
        .collect();
    let per_run: Vec<Value> = runs
        .iter()
        .map(|run| {
            json!([
                a::active_duration(run, a::IDLE_CAP_MS, None),
                a::active_duration(run, 60_000, Some(now)),
                a::run_net(run, &pricing),
                run_value(run, &pricing),
                run_cost(run, &pricing),
                a::run_fe(run),
                a::strategy_key(run),
                a::is_farming_run(run),
                a::is_prison_run(run),
                a::run_key(run),
                name_of(run),
                h::day_start(a::run_end(run)),
            ])
        })
        .collect();
    let trends: Vec<Value> = history.iter().map(|(id, pts)| json!([id, p::price_trend(pts)])).collect();

    Ok(json!({
        "scan": {
            "runs": runs,
            "prices": store.all(),
            "observations": store.observations(),
            "trade": { "listings": r.trade.listings, "recalls": r.trade.recalls, "receipts": r.trade.receipts, "purchases": r.trade.purchases },
            "filesScanned": r.files_scanned,
            "linesScanned": r.lines_scanned,
            "seasonIds": r.season_ids.iter().collect::<Vec<_>>(),
            "unknownProtos": r.unknown_protos.iter().map(|(k, n)| json!([k, n])).collect::<Vec<_>>(),
            "failed": r.failed.iter().map(|(p, _)| file_name(p)).collect::<Vec<_>>(),
            "slots": r.slots,
            "latestSeasonId": r.latest_season_id,
            "players": r.players,
            "lastPlayerId": r.last_player_id,
            "currentLog": r.current_log.as_ref().map(|(p, pos)| json!({ "file": file_name(p), "position": pos })),
            "segmenter": r.segmenter,
        },
        "bound": bound,
        "perRun": per_run,
        "merged": a::merge_runs(&[&runs[..half], &runs[third..]]).iter().map(a::run_key).collect::<Vec<_>>(),
        "missing": a::missing_prices(runs, &pricing, None),
        "strategies": a::group_by_strategy(&rated, &pricing, a::IDLE_CAP_MS),
        "maps": h::group_by_map(&rated, &name_of, &pricing),
        "days": h::daily_totals(&farming, &pricing, 45, now),
        "sessions": h::session_totals(&sessions, &farming, &pricing, None),
        "sessions3": h::session_totals(&sessions, &farming, &pricing, Some(3)),
        "norm": [a::rolling_norm(&rates, a::NORM_WINDOW), a::rolling_norm(&rates, 5)],
        "top": top,
        "bag": [gorn_core::bag::bag_value(&contents, &price_of, 6), gorn_core::bag::bag_value(&contents, &price_of, 2)],
        "realization": r.trade.summarize(price_of, f64::NEG_INFINITY, f64::INFINITY),
        "trends": trends,
        "choices": choices,
        "lookups": lookups,
        "seasons": season_ids.iter().map(|&id| json!([season_name(id, Lang::Ru), season_short(id, Lang::Ru)])).collect::<Vec<_>>(),
        "heroes": hero_ids.iter().map(|&id| hero_name(id, Lang::Ru)).collect::<Vec<_>>(),
        "stats": [s::median(&rates), s::percentile(&sorted, 0.1), s::percentile(&sorted, 0.9), s::top_share_of(&nets),
                  s::summarize(&rates, Some(&nets)), s::summarize(&nets, None)],
    }))
}
