//! Хранилище для сверки: тот же сценарий записи и тот же дамп базы, что у
//! команд `db-write` / `db-read` эталона (tools/reference.mts).

use std::collections::HashSet;
use std::path::Path;

use gorn_core::retroscan::retro_scan;
use gorn_core::storage::Storage;
use gorn_core::{AnalyzeOptions, Run};
use rusqlite::types::ValueRef;
use serde_json::{json, Value};

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn cell(v: ValueRef) -> Value {
    match v {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(i) => json!(i),
        ValueRef::Real(f) => json!(f),
        ValueRef::Text(t) => json!(String::from_utf8_lossy(t)),
        ValueRef::Blob(b) => json!(b),
    }
}

/// Дамп базы: таблицы со всеми строками и типами значений, load*, история цен, счётчики, сессии.
pub fn dump(s: &Storage, season: Option<i64>) -> Result<Value, String> {
    let db = s.connection();
    let mut tables = vec![];
    let mut st = db.prepare("SELECT name, sql FROM sqlite_master WHERE type = 'table' AND name <> 'sqlite_sequence' ORDER BY name").map_err(err)?;
    let list: Vec<(String, String)> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
    for (name, sql) in list {
        let mut ti = db.prepare(&format!("PRAGMA table_info({name})")).map_err(err)?;
        let cols: Vec<String> = ti.query_map([], |r| r.get("name")).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
        let sel = cols.iter().map(|c| format!("{c}, typeof({c})")).collect::<Vec<_>>().join(", ");
        let mut q = db.prepare(&format!("SELECT {sel} FROM {name} ORDER BY {}", cols.join(", "))).map_err(err)?;
        let n = cols.len() * 2;
        let rows: Vec<Value> = q
            .query_map([], |r| Ok(Value::Array((0..n).map(|i| r.get_ref(i).map(cell)).collect::<Result<_, _>>()?)))
            .map_err(err)?
            .collect::<Result<_, _>>()
            .map_err(err)?;
        tables.push(json!([name, sql, rows]));
    }
    let user_version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0)).map_err(err)?;
    // Выборки без ORDER BY упорядочены планом запроса — сверяем без порядка.
    let price_key = |p: &gorn_core::prices::PriceRecord| (p.config_base_id, p.observed_at, p.season_id.unwrap_or(-1), p.fe.to_bits());
    let mut prices = s.load_prices(None).map_err(err)?;
    prices.sort_by_key(price_key);
    let mut prices_season = s.load_prices(Some(season.unwrap_or(0))).map_err(err)?;
    prices_season.sort_by_key(price_key);
    let mut trade = s.load_trade().map_err(err)?;
    trade.listings.sort_by_key(|l| (l.ts, l.config_base_id, l.qty));
    trade.recalls.sort_by_key(|l| (l.ts, l.config_base_id, l.qty));
    trade.receipts.sort_by_key(|r| (r.ts, r.fe));
    let mut history = vec![];
    for p in prices.iter().take(6) {
        history.push(json!([
            s.price_history(p.config_base_id, 60, None).map_err(err)?,
            s.price_history(p.config_base_id, 3, p.season_id).map_err(err)?
        ]));
    }
    Ok(json!({
        "season": season,
        "userVersion": user_version,
        "tables": tables,
        "runs": s.load_runs().map_err(err)?,
        "players": s.load_players().map_err(err)?,
        "trade": trade,
        "prices": prices,
        "pricesSeason": prices_season,
        "history": history,
        "counts": s.counts().map_err(err)?,
        "realizedFe": s.realized_fe().map_err(err)?,
        "sessions": s.list_sessions().map_err(err)?,
        "previous": s.previous_session().map_err(err)?.map(|(a, b)| json!({ "startedAt": a, "endedAt": b })),
        "active": s.active_session_start().map_err(err)?,
    }))
}

/// Сценарий записи: ретро-скан папки и запись пачками — склейка, усечённые копии, повторы, сессии.
pub fn write(dir: &Path, db: &Path) -> Result<Value, String> {
    let items: Value = serde_json::from_str(include_str!("../../gorn-core/data/items.json")).map_err(err)?;
    let keys: HashSet<i64> = items
        .as_array()
        .ok_or("items.json")?
        .iter()
        .filter(|it| gorn_core::patterns::is_map_key_type(it["typeCn"].as_str()))
        .filter_map(|it| it["id"].as_i64())
        .collect();
    let is_key = |id: i64| keys.contains(&id);
    let r = retro_scan(dir, None, AnalyzeOptions { is_map_key: Some(&is_key), current: false, gear_base: None }).map_err(err)?;
    let s = Storage::open(db).map_err(err)?;
    let runs = &r.runs;
    let season = r.latest_season_id;
    s.active_session_start().map_err(err)?;
    if runs.len() > 1 && runs[1].started_at > runs[0].started_at {
        let glued = Run { ended_at: Some(runs[1].started_at + 1), duration_ms: Some(1_000_000_000), ..runs[0].clone() };
        s.save_runs(&[glued], None).map_err(err)?;
    }
    s.save_runs(&runs[..runs.len().div_ceil(2)], season).map_err(err)?;
    s.save_players(&r.players).map_err(err)?;
    s.save_prices(r.prices.observations()).map_err(err)?;
    s.save_trade(&r.trade.listings, &r.trade.receipts, &r.trade.recalls).map_err(err)?;
    let cut: Vec<Run> = runs
        .iter()
        .step_by(3)
        .map(|x| Run { duration_ms: x.duration_ms.map(|d| d.div_euclid(2)), drops: x.drops[..x.drops.len() / 2].to_vec(), ..x.clone() })
        .collect();
    s.save_runs(&cut, None).map_err(err)?;
    if let Some(first) = runs.first() {
        s.reset_session(first.ended_at.unwrap_or(first.started_at)).map_err(err)?;
    }
    s.save_runs(runs, season).map_err(err)?;
    let bare: Vec<Run> =
        runs.iter().skip(1).step_by(4).map(|x| Run { season_id: None, player_id: None, asset: None, ..x.clone() }).collect();
    s.save_runs(&bare, Some(1601)).map_err(err)?;
    s.save_trade(&r.trade.listings, &r.trade.receipts[..r.trade.receipts.len().min(3)], &r.trade.recalls).map_err(err)?;
    // Одинаковые выплаты и отзывы в одну миллисекунду — разные события.
    let last: Vec<_> = r.trade.receipts.iter().rev().take(1).cloned().collect();
    let first: Vec<_> = r.trade.recalls.iter().take(1).cloned().collect();
    s.save_trade(&[], &[last.clone(), last.clone(), last].concat(), &[first.clone(), first].concat()).map_err(err)?;
    let unseasoned: Vec<_> = r.prices.all().into_iter().map(|p| gorn_core::prices::PriceRecord { season_id: None, ..p }).collect();
    s.save_prices(&unseasoned).map_err(err)?;
    if let Some(last) = runs.last() {
        s.reset_session(last.started_at).map_err(err)?;
    }
    drop(s);
    let again = Storage::open(db).map_err(err)?;
    dump(&again, season)
}

/// Открыть базу (миграции, ремонт сезонов) и снять дамп.
pub fn read(db: &Path, season: Option<i64>) -> Result<Value, String> {
    let s = Storage::open(db).map_err(err)?;
    dump(&s, season)
}

/// База прошлых версий (user_version = 0): цены с NULL-сезоном и дублями, забеги
/// без player_id и половины карт внутри целых, выплаты без seq, log_cursor,
/// частично отозванные лоты второй строкой, пробивки без сезона рядом с забегами.
pub fn old_fixture(path: &Path, seed: u64) -> Result<(), String> {
    let _ = std::fs::remove_file(path);
    let db = rusqlite::Connection::open(path).map_err(err)?;
    let mut x = seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1;
    let mut rnd = move |n: u64| {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        (x % n) as i64
    };
    db.execute_batch(
        "CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
         CREATE TABLE run (
           id INTEGER PRIMARY KEY AUTOINCREMENT, season_id INTEGER, area_id INTEGER NOT NULL, asset TEXT,
           level_type INTEGER, started_at INTEGER NOT NULL, ended_at INTEGER, duration_ms INTEGER,
           is_hub INTEGER NOT NULL, UNIQUE (started_at, area_id));
         CREATE TABLE player (player_id TEXT PRIMARY KEY, name TEXT, hero_id INTEGER, level INTEGER,
           season_id INTEGER, last_seen_at INTEGER NOT NULL);
         CREATE TABLE drop_item (run_id INTEGER NOT NULL REFERENCES run(id) ON DELETE CASCADE,
           config_base_id INTEGER NOT NULL, qty INTEGER NOT NULL, proto TEXT NOT NULL, ts INTEGER NOT NULL);
         CREATE TABLE cost_item (run_id INTEGER NOT NULL REFERENCES run(id) ON DELETE CASCADE,
           config_base_id INTEGER NOT NULL, qty INTEGER NOT NULL, ts INTEGER NOT NULL);
         CREATE TABLE price (config_base_id INTEGER NOT NULL, season_id INTEGER, fe REAL NOT NULL,
           median REAL NOT NULL, min_price REAL NOT NULL, sample_size INTEGER NOT NULL, observed_at INTEGER NOT NULL,
           PRIMARY KEY (config_base_id, season_id, observed_at));
         CREATE TABLE listing (ts INTEGER NOT NULL, config_base_id INTEGER NOT NULL, qty INTEGER NOT NULL,
           UNIQUE (ts, config_base_id, qty));
         CREATE TABLE receipt (ts INTEGER NOT NULL, fe INTEGER NOT NULL, UNIQUE (ts, fe));
         CREATE TABLE log_cursor (path TEXT PRIMARY KEY, position INTEGER NOT NULL);
         INSERT INTO log_cursor VALUES ('UE_game.log', 123);",
    )
    .map_err(err)?;
    let base = 1_790_000_000_000i64;
    for i in 0..40 {
        let area = [5348, 5351, 506][rnd(3) as usize];
        let start = base + i * 600_000 + rnd(1000);
        let dur = 30_000 + rnd(400_000);
        let season: Option<i64> = [None, Some(1501), Some(1601)][rnd(3) as usize];
        db.execute(
            "INSERT OR IGNORE INTO run (season_id, area_id, asset, level_type, started_at, ended_at, duration_ms, is_hub) VALUES (?, ?, ?, 3, ?, ?, ?, ?)",
            rusqlite::params![season, area, format!("Asset{area}"), start, start + dur, dur, (rnd(10) == 0) as i64],
        )
        .map_err(err)?;
        let id = db.last_insert_rowid();
        // Половина карты внутри целой — отдельным забегом.
        if rnd(4) == 0 {
            db.execute(
                "INSERT OR IGNORE INTO run (season_id, area_id, asset, level_type, started_at, ended_at, duration_ms, is_hub) VALUES (?, ?, ?, 3, ?, ?, ?, 0)",
                rusqlite::params![season, area, format!("Asset{area}"), start + 1000, start + dur / 2, dur / 2],
            )
            .map_err(err)?;
        }
        for k in 0..rnd(5) {
            db.execute("INSERT INTO drop_item VALUES (?, ?, ?, 'PickItems', ?)", rusqlite::params![id, 100300 + rnd(3), 1 + rnd(9), start + k * 1000])
                .map_err(err)?;
        }
        if rnd(2) == 0 {
            db.execute("INSERT INTO cost_item VALUES (?, 5210, 1, ?)", rusqlite::params![id, start]).map_err(err)?;
        }
    }
    for _ in 0..60 {
        let at = base + rnd(40) * 600_000 + rnd(5000) - 2_000_000;
        let fe = (1 + rnd(5000)) as f64 / 7.0;
        for _ in 0..1 + rnd(3) {
            db.execute(
                "INSERT OR IGNORE INTO price VALUES (?, ?, ?, ?, ?, ?, ?)",
                rusqlite::params![200 + rnd(8), [None, Some(1601i64)][rnd(2) as usize], fe, fe * 1.5, fe / 2.0, 1 + rnd(30), at],
            )
            .map_err(err)?;
        }
    }
    for _ in 0..30 {
        let ts = base + rnd(20) * 1000;
        let id = 300 + rnd(3);
        let q = 1 + rnd(5);
        db.execute("INSERT OR IGNORE INTO listing VALUES (?, ?, ?)", rusqlite::params![ts, id, q]).map_err(err)?;
        db.execute("INSERT OR IGNORE INTO receipt VALUES (?, ?)", rusqlite::params![ts, rnd(4) * 10]).map_err(err)?;
    }
    db.execute("INSERT INTO player VALUES ('95608272857761', 'X', 1700, 80, 1601, ?)", [base]).map_err(err)?;
    db.execute("UPDATE run SET season_id = NULL WHERE id % 7 = 0", []).map_err(err)?;
    Ok(())
}
