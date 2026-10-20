//! Хранилище разобранной истории на SQLite. Перенос `storage/db.ts`:
//! тот же файл `gorn.db`, та же схема, миграции по `user_version`
//! продолжают нумерацию эталона.

use std::collections::HashMap;
use std::path::Path;

use indexmap::IndexMap;
use rusqlite::{named_params, params, Connection, OptionalExtension};
use serde::Serialize;

use crate::prices::{PricePoint, PriceRecord};
use crate::trade::{Listing, Recall, Receipt};
use crate::types::*;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS run (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  season_id    INTEGER,
  area_id      INTEGER NOT NULL,
  asset        TEXT,
  level_type   INTEGER,
  started_at   INTEGER NOT NULL,
  ended_at     INTEGER,
  duration_ms  INTEGER,
  is_hub       INTEGER NOT NULL,
  -- Персонаж хранится СТРОКОЙ: id вида 956082728577617920 не помещается
  -- в double, а INTEGER-колонку node:sqlite отдал бы как number.
  player_id    TEXT,
  UNIQUE (started_at, area_id)
);

-- Известные персонажи: ник аккаунта, класс и уровень для показа в интерфейсе.
CREATE TABLE IF NOT EXISTS player (
  player_id    TEXT PRIMARY KEY,
  name         TEXT,
  hero_id      INTEGER,
  level        INTEGER,
  season_id    INTEGER,
  last_seen_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS drop_item (
  run_id          INTEGER NOT NULL REFERENCES run(id) ON DELETE CASCADE,
  config_base_id  INTEGER NOT NULL,
  qty             INTEGER NOT NULL,
  proto           TEXT NOT NULL,
  ts              INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_drop_run ON drop_item(run_id);

CREATE TABLE IF NOT EXISTS cost_item (
  run_id          INTEGER NOT NULL REFERENCES run(id) ON DELETE CASCADE,
  config_base_id  INTEGER NOT NULL,
  qty             INTEGER NOT NULL,
  ts              INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_cost_run ON cost_item(run_id);

-- season_id участвует в первичном ключе, а в SQLite NULL != NULL,
-- поэтому NULL здесь запрещён: неизвестный сезон записывается нулём.
-- Иначе INSERT OR REPLACE никогда не срабатывал и каждый запуск
-- добавлял полный дубль всех цен.
CREATE TABLE IF NOT EXISTS price (
  config_base_id  INTEGER NOT NULL,
  season_id       INTEGER NOT NULL DEFAULT 0,
  fe              REAL NOT NULL,
  median          REAL NOT NULL,
  min_price       REAL NOT NULL,
  sample_size     INTEGER NOT NULL,
  observed_at     INTEGER NOT NULL,
  PRIMARY KEY (config_base_id, season_id, observed_at)
);
CREATE INDEX IF NOT EXISTS idx_price_item ON price(config_base_id, observed_at DESC);

CREATE TABLE IF NOT EXISTS listing (
  ts              INTEGER NOT NULL,
  config_base_id  INTEGER NOT NULL,
  qty             INTEGER NOT NULL,
  UNIQUE (ts, config_base_id, qty)
);

-- Отзывы лотов — событиями, как в логе: действующие лоты получаются
-- вычитанием при загрузке (applyRecalls). seq — как у выплат.
CREATE TABLE IF NOT EXISTS listing_recall (
  ts              INTEGER NOT NULL,
  config_base_id  INTEGER NOT NULL,
  qty             INTEGER NOT NULL,
  seq             INTEGER NOT NULL DEFAULT 0,
  UNIQUE (ts, config_base_id, qty, seq)
);

-- seq различает одинаковые выплаты в одну миллисекунду: выкуп двух
-- одинаковых лотов — это две выплаты, а не одна.
CREATE TABLE IF NOT EXISTS receipt (
  ts   INTEGER NOT NULL,
  fe   INTEGER NOT NULL,
  seq  INTEGER NOT NULL DEFAULT 0,
  UNIQUE (ts, fe, seq)
);

-- Сессии игрока: граница отсчёта для счётчиков на экране. Забеги при
-- сбросе не трогаются — сессия лишь помнит, С КАКОГО момента считать.
-- Активная сессия — единственная с ended_at IS NULL.
CREATE TABLE IF NOT EXISTS session (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  started_at INTEGER NOT NULL,
  ended_at   INTEGER
);
"#;

/// Удаляет забеги, целиком лежащие внутри другого забега той же зоны
/// (см. `merge_runs`): прошлые версии писали вторую половину карты после
/// захода порталом отдельно. Состав — каскадом. Миграция 5 и после каждой записи забегов.
const DROP_NESTED_RUNS: &str = r#"
  DELETE FROM run WHERE id IN (
    SELECT id FROM (
      SELECT id, ended_at,
             MAX(ended_at) OVER (
               PARTITION BY area_id ORDER BY started_at
               ROWS BETWEEN UNBOUNDED PRECEDING AND 1 PRECEDING
             ) AS reach
      FROM run WHERE is_hub = 0 AND ended_at IS NOT NULL
    ) WHERE reach IS NOT NULL AND ended_at <= reach
  )"#;

/// Миграции баз прошлых версий. Номер выполненной — в PRAGMA user_version,
/// каждая один раз, в своей транзакции. Каждая сама проверяет схему: до
/// появления номера они гонялись на каждом запуске, а новая база, созданная
/// SCHEMA, проходит их вхолостую. Добавлять — только в конец.
const MIGRATIONS: [fn(&Connection) -> rusqlite::Result<()>; 6] = [
    // 1. Цены: season_id был NULL, а NULL в SQLite не равен NULL — дубли.
    |db| {
        db.execute_batch(
            "DELETE FROM price WHERE rowid NOT IN (
               SELECT MAX(rowid) FROM price
               GROUP BY config_base_id, season_id, observed_at
             );
             UPDATE price SET season_id = 0 WHERE season_id IS NULL;",
        )
    },
    // 2. Забеги: добавилась колонка player_id.
    |db| {
        if !has_column(db, "run", "player_id")? {
            db.execute_batch("ALTER TABLE run ADD COLUMN player_id TEXT")?;
        }
        Ok(())
    },
    // 3. Выплаты: добавилась колонка seq; UNIQUE старой таблицы не снять — пересоздание.
    |db| {
        if has_column(db, "receipt", "seq")? {
            return Ok(());
        }
        // Текст как у эталона: SQLite хранит CREATE TABLE как написан.
        db.execute_batch(
            r"
      CREATE TABLE receipt_migrated (
        ts   INTEGER NOT NULL,
        fe   INTEGER NOT NULL,
        seq  INTEGER NOT NULL DEFAULT 0,
        UNIQUE (ts, fe, seq)
      );
      INSERT INTO receipt_migrated (ts, fe, seq) SELECT ts, fe, 0 FROM receipt;
      DROP TABLE receipt;
      ALTER TABLE receipt_migrated RENAME TO receipt;
    ",
        )
    },
    // 4. Позиции чтения логов (log_cursor) так и не понадобились.
    |db| db.execute_batch("DROP TABLE IF EXISTS log_cursor"),
    // 5. Половины карт, записанные отдельным забегом рядом с целой картой.
    |db| db.execute_batch(DROP_NESTED_RUNS),
    // 6. Частично отозванный лот прошлые версии уменьшали на месте, и повторный
    //    разбор дописывал вторую строку: настоящая — с наибольшим количеством.
    |db| {
        db.execute_batch(
            "DELETE FROM listing WHERE EXISTS (
               SELECT 1 FROM listing o
               WHERE o.ts = listing.ts AND o.config_base_id = listing.config_base_id
                 AND o.qty > listing.qty
             )",
        )
    },
];

fn has_column(db: &Connection, table: &str, column: &str) -> rusqlite::Result<bool> {
    let mut st = db.prepare(&format!("PRAGMA table_info({table})"))?;
    let names = st.query_map([], |r| r.get::<_, String>("name"))?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(names.iter().any(|n| n == column))
}

/// Тело в транзакции: откат при ошибке.
fn in_tx<T>(db: &Connection, f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> rusqlite::Result<T> {
    db.execute_batch("BEGIN")?;
    match f(db) {
        Ok(v) => {
            db.execute_batch("COMMIT")?;
            Ok(v)
        }
        Err(e) => {
            let _ = db.execute_batch("ROLLBACK");
            Err(e)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub started_at: i64,
    /// None — активная сессия.
    pub ended_at: Option<i64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct StoredTrade {
    pub listings: Vec<Listing>,
    pub receipts: Vec<Receipt>,
    pub recalls: Vec<Recall>,
}

/// Хранилище разобранной истории: игра ротирует логи и удаляет старые
/// бэкапы — всё, что не сохранено, теряется навсегда.
pub struct Storage {
    db: Connection,
}

impl Storage {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let db = Connection::open(path)?;
        db.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")?;
        db.execute_batch(SCHEMA)?;
        let s = Storage { db };
        s.migrate()?;
        Ok(s)
    }

    /// Соединение — для проверок и выборок, которых нет в методах.
    pub fn connection(&self) -> &Connection {
        &self.db
    }

    fn migrate(&self) -> rusqlite::Result<()> {
        let version: i64 = self.db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        for v in version.max(0) as usize..MIGRATIONS.len() {
            in_tx(&self.db, |db| {
                MIGRATIONS[v](db)?;
                db.execute_batch(&format!("PRAGMA user_version = {}", v + 1))
            })?;
        }
        self.repair_seasons()
    }

    /// Досведение сезонов — ремонт данных на каждом запуске, не миграция.
    fn repair_seasons(&self) -> rusqlite::Result<()> {
        // Сезон забега без профиля в файле — у персонажа: он живёт ровно в одном сезоне.
        self.db.execute_batch(
            "UPDATE run SET season_id = (
               SELECT p.season_id FROM player p WHERE p.player_id = run.player_id
             )
             WHERE season_id IS NULL AND player_id IS NOT NULL",
        )?;
        // Свои пробивки без сезона (0) — сезон последнего забега, начатого до неё.
        // OR IGNORE: при конфликте строка с сезоном остаётся, бессезонный двойник удаляется.
        in_tx(&self.db, |db| {
            db.execute_batch(
                "UPDATE OR IGNORE price SET season_id = (
                   SELECT r.season_id FROM run r
                   WHERE r.season_id IS NOT NULL AND r.started_at <= price.observed_at
                   ORDER BY r.started_at DESC LIMIT 1
                 )
                 WHERE season_id = 0 AND EXISTS (
                   SELECT 1 FROM run r
                   WHERE r.season_id IS NOT NULL AND r.started_at <= price.observed_at
                 );
                 DELETE FROM price WHERE season_id = 0 AND EXISTS (
                   SELECT 1 FROM price p
                   WHERE p.config_base_id = price.config_base_id
                     AND p.observed_at = price.observed_at
                     AND p.season_id <> 0
                 );",
            )
        })
    }

    /// Сохраняет забеги; повторный скан дублей не создаёт (старт + зона).
    /// Сезон — свой у забега; `fallback_season_id` — забегам без профиля в логе.
    pub fn save_runs(&self, runs: &[Run], fallback_season_id: Option<i64>) -> rusqlite::Result<usize> {
        // Начало следующего забега пачки: копия, заходящая за него, — склейка заходов.
        let mut starts: Vec<i64> = runs.iter().map(|r| r.started_at).collect();
        starts.sort();
        starts.dedup();
        let next_start = |ts: i64| starts.iter().copied().find(|&t| t > ts);
        let saved = in_tx(&self.db, |db| {
            let mut insert_run = db.prepare_cached(
                "INSERT OR IGNORE INTO run
                   (season_id, area_id, asset, level_type, started_at, ended_at, duration_ms, is_hub, player_id)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )?;
            let mut find_run = db.prepare_cached("SELECT id, duration_ms, ended_at FROM run WHERE started_at = ? AND area_id = ?")?;
            let mut update_run = db.prepare_cached(
                "UPDATE run SET
                   season_id  = COALESCE(?, season_id, ?),
                   asset      = COALESCE(?, asset),
                   level_type = COALESCE(?, level_type),
                   ended_at   = ?,
                   duration_ms = ?,
                   player_id  = COALESCE(?, player_id)
                 WHERE id = ?",
            )?;
            let mut insert_drop = db.prepare_cached("INSERT INTO drop_item (run_id, config_base_id, qty, proto, ts) VALUES (?, ?, ?, ?, ?)")?;
            let mut insert_cost = db.prepare_cached("INSERT INTO cost_item (run_id, config_base_id, qty, ts) VALUES (?, ?, ?, ?)")?;
            let mut clear_drops = db.prepare_cached("DELETE FROM drop_item WHERE run_id = ?")?;
            let mut clear_costs = db.prepare_cached("DELETE FROM cost_item WHERE run_id = ?")?;
            let mut saved = 0;
            for run in runs {
                let season = run.season_id.or(fallback_season_id.map(|s| s as f64));
                insert_run.execute(params![
                    season,
                    run.area_id,
                    run.asset,
                    run.level_type,
                    run.started_at,
                    run.ended_at,
                    run.duration_ms,
                    run.is_hub as i64,
                    run.player_id
                ])?;
                let row: Option<(i64, Option<i64>, Option<i64>)> =
                    find_run.query_row(params![run.started_at, run.area_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).optional()?;
                let Some((id, duration, ended)) = row else { continue };
                // Обновляет только более полная версия забега; но копия,
                // перекрывающая следующий забег пачки, — склейка, и её свежий разбор заменяет.
                let merged = matches!((next_start(run.started_at), ended), (Some(next), Some(e)) if e > next);
                let better = merged || duration.is_none_or(|d| run.duration_ms.unwrap_or(0) >= d);
                if !better {
                    continue;
                }
                update_run.execute(params![
                    run.season_id,
                    fallback_season_id,
                    run.asset,
                    run.level_type,
                    run.ended_at,
                    run.duration_ms,
                    run.player_id,
                    id
                ])?;
                // Состав перезаписывается: повторный разбор мог стать точнее.
                clear_drops.execute([id])?;
                clear_costs.execute([id])?;
                for d in &run.drops {
                    insert_drop.execute(params![id, d.config_base_id, d.qty, d.proto, d.ts])?;
                }
                for c in &run.costs {
                    insert_cost.execute(params![id, c.config_base_id, c.qty, c.ts])?;
                }
                saved += 1;
            }
            Ok(saved)
        })?;
        // Целая карта пришла после своей половины, записанной раньше отдельно.
        if saved > 0 {
            self.db.execute_batch(DROP_NESTED_RUNS)?;
        }
        Ok(saved)
    }

    pub fn save_prices(&self, records: &[PriceRecord]) -> rusqlite::Result<usize> {
        in_tx(&self.db, |db| {
            let mut st = db.prepare_cached(
                "INSERT OR REPLACE INTO price
                   (config_base_id, season_id, fe, median, min_price, sample_size, observed_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
            )?;
            for r in records {
                // Неизвестный сезон — нулём: NULL сломал бы первичный ключ.
                st.execute(params![r.config_base_id, r.season_id.unwrap_or(0), r.fe, r.median, r.min, r.sample_size as i64, r.observed_at])?;
            }
            Ok(records.len())
        })
    }

    pub fn save_trade(&self, listings: &[Listing], receipts: &[Receipt], recalls: &[Recall]) -> rusqlite::Result<()> {
        in_tx(&self.db, |db| {
            let mut il = db.prepare_cached("INSERT OR IGNORE INTO listing (ts, config_base_id, qty) VALUES (?, ?, ?)")?;
            let mut ir = db.prepare_cached("INSERT OR IGNORE INTO receipt (ts, fe, seq) VALUES (?, ?, ?)")?;
            let mut ic = db.prepare_cached("INSERT OR IGNORE INTO listing_recall (ts, config_base_id, qty, seq) VALUES (?, ?, ?, ?)")?;
            for l in listings {
                il.execute(params![l.ts, l.config_base_id, l.qty])?;
            }
            // Одинаковые выплаты в одну миллисекунду различаются порядковым номером.
            let mut seq: HashMap<(i64, i64), i64> = HashMap::new();
            for r in receipts {
                let n = seq.entry((r.ts, r.fe)).or_insert(0);
                ir.execute(params![r.ts, r.fe, *n])?;
                *n += 1;
            }
            let mut seq: HashMap<(i64, i64, i64), i64> = HashMap::new();
            for r in recalls {
                let n = seq.entry((r.ts, r.config_base_id, r.qty)).or_insert(0);
                ic.execute(params![r.ts, r.config_base_id, r.qty, *n])?;
                *n += 1;
            }
            Ok(())
        })
    }

    /// Забеги из базы вместе с составом дропа и расхода.
    pub fn load_runs(&self) -> rusqlite::Result<Vec<Run>> {
        let mut st = self.db.prepare(
            "SELECT id, area_id, asset, level_type, started_at, ended_at, duration_ms, is_hub, player_id, season_id
             FROM run ORDER BY started_at",
        )?;
        let mut drops = self.db.prepare("SELECT config_base_id, qty, proto, ts FROM drop_item WHERE run_id = ?")?;
        let mut costs = self.db.prepare("SELECT config_base_id, qty, ts FROM cost_item WHERE run_id = ?")?;
        let rows = st
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    Run {
                        area_id: r.get(1)?,
                        asset: r.get(2)?,
                        level_type: r.get(3)?,
                        started_at: r.get(4)?,
                        ended_at: r.get(5)?,
                        duration_ms: r.get(6)?,
                        is_hub: r.get::<_, i64>(7)? == 1,
                        player_id: r.get(8)?,
                        season_id: r.get(9)?,
                        drops: vec![],
                        costs: vec![],
                    },
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut out = Vec::with_capacity(rows.len());
        for (id, mut run) in rows {
            run.drops = drops
                .query_map([id], |r| {
                    Ok(ItemDelta {
                        ts: r.get(3)?,
                        config_base_id: r.get(0)?,
                        qty: r.get(1)?,
                        proto: r.get(2)?,
                        kind: ProtoKind::Loot,
                        page_id: 0,
                        slot_id: 0,
                    })
                })?
                .collect::<rusqlite::Result<_>>()?;
            run.costs = costs
                .query_map([id], |r| {
                    Ok(ItemDelta {
                        ts: r.get(2)?,
                        config_base_id: r.get(0)?,
                        qty: r.get(1)?,
                        proto: "Spv3Open".into(),
                        kind: ProtoKind::MapCost,
                        page_id: 0,
                        slot_id: 0,
                    })
                })?
                .collect::<rusqlite::Result<_>>()?;
            out.push(run);
        }
        Ok(out)
    }

    /// Профили: свежие значения побеждают, пустые не затирают известные.
    pub fn save_players(&self, profiles: &[PlayerProfile]) -> rusqlite::Result<()> {
        if profiles.is_empty() {
            return Ok(());
        }
        in_tx(&self.db, |db| {
            let mut st = db.prepare_cached(
                "INSERT INTO player (player_id, name, hero_id, level, season_id, last_seen_at)
                 VALUES (?, ?, ?, ?, ?, ?)
                 ON CONFLICT(player_id) DO UPDATE SET
                   name         = COALESCE(excluded.name, name),
                   hero_id      = COALESCE(excluded.hero_id, hero_id),
                   level        = COALESCE(excluded.level, level),
                   season_id    = COALESCE(excluded.season_id, season_id),
                   last_seen_at = MAX(excluded.last_seen_at, last_seen_at)",
            )?;
            for p in profiles {
                st.execute(params![p.player_id, p.name, p.hero_id, p.level, p.season_id, p.last_seen_at])?;
            }
            Ok(())
        })
    }

    pub fn load_players(&self) -> rusqlite::Result<Vec<PlayerProfile>> {
        let mut st = self.db.prepare(
            "SELECT player_id, name, hero_id, level, season_id, last_seen_at FROM player ORDER BY last_seen_at DESC",
        )?;
        let v = st
            .query_map([], |r| {
                Ok(PlayerProfile {
                    player_id: r.get(0)?,
                    name: r.get(1)?,
                    hero_id: r.get(2)?,
                    level: r.get(3)?,
                    season_id: r.get(4)?,
                    last_seen_at: r.get(5)?,
                })
            })?
            .collect();
        v
    }

    pub fn load_trade(&self) -> rusqlite::Result<StoredTrade> {
        let listing = |sql: &str| -> rusqlite::Result<Vec<Listing>> {
            let mut st = self.db.prepare(sql)?;
            let v = st.query_map([], |r| Ok(Listing { ts: r.get(0)?, config_base_id: r.get(1)?, qty: r.get(2)? }))?.collect();
            v
        };
        let mut st = self.db.prepare("SELECT ts, fe FROM receipt")?;
        let receipts = st.query_map([], |r| Ok(Receipt { ts: r.get(0)?, fe: r.get(1)? }))?.collect::<rusqlite::Result<_>>()?;
        Ok(StoredTrade {
            listings: listing("SELECT ts, config_base_id, qty FROM listing")?,
            receipts,
            recalls: listing("SELECT ts, config_base_id, qty FROM listing_recall")?,
        })
    }

    /// Свежие цены по предметам — только своего сезона; None — все.
    pub fn load_prices(&self, season_id: Option<i64>) -> rusqlite::Result<Vec<PriceRecord>> {
        let scope = if season_id.is_some() { "AND season_id = :season" } else { "" };
        let mut st = self.db.prepare(&format!(
            "SELECT config_base_id, season_id, fe, median, min_price, sample_size, observed_at
             FROM price p
             WHERE observed_at = (
               SELECT MAX(observed_at) FROM price
               WHERE config_base_id = p.config_base_id {scope}
             ) {scope}"
        ))?;
        let map = |r: &rusqlite::Row| {
            Ok(PriceRecord {
                config_base_id: r.get(0)?,
                season_id: r.get(1)?,
                fe: r.get(2)?,
                median: r.get(3)?,
                min: r.get(4)?,
                sample_size: r.get::<_, i64>(5)? as usize,
                observed_at: r.get(6)?,
            })
        };
        let v = match season_id {
            Some(s) => st.query_map(named_params! { ":season": s }, map)?.collect(),
            None => st.query_map([], map)?.collect(),
        };
        v
    }

    /// Последние `limit` своих пробивок предмета по времени (внутри сезона, если он задан).
    pub fn price_history(&self, id: ConfigBaseId, limit: i64, season_id: Option<i64>) -> rusqlite::Result<Vec<PricePoint>> {
        let scope = if season_id.is_some() { "AND season_id = :season" } else { "" };
        let mut st = self.db.prepare(&format!(
            "SELECT observed_at AS t, fe FROM price WHERE config_base_id = :id {scope} ORDER BY observed_at DESC LIMIT :limit"
        ))?;
        let map = |r: &rusqlite::Row| Ok(PricePoint { t: r.get(0)?, fe: r.get(1)? });
        let mut v: Vec<PricePoint> = match season_id {
            Some(s) => st.query_map(named_params! { ":id": id, ":limit": limit, ":season": s }, map)?.collect::<rusqlite::Result<_>>()?,
            None => st.query_map(named_params! { ":id": id, ":limit": limit }, map)?.collect::<rusqlite::Result<_>>()?,
        };
        v.reverse();
        Ok(v)
    }

    pub fn counts(&self) -> rusqlite::Result<IndexMap<&'static str, i64>> {
        let one = |sql: &str| self.db.query_row(sql, [], |r| r.get::<_, i64>(0));
        Ok(IndexMap::from([
            ("runs", one("SELECT COUNT(*) AS n FROM run")?),
            ("farmingRuns", one("SELECT COUNT(*) AS n FROM run WHERE is_hub = 0")?),
            ("drops", one("SELECT COUNT(*) AS n FROM drop_item")?),
            ("prices", one("SELECT COUNT(DISTINCT config_base_id) AS n FROM price")?),
            ("priceRows", one("SELECT COUNT(*) AS n FROM price")?),
            ("listings", one("SELECT COUNT(*) AS n FROM listing")?),
            ("receipts", one("SELECT COUNT(*) AS n FROM receipt")?),
        ]))
    }

    /// Реально полученные ФЕ за всю сохранённую историю.
    pub fn realized_fe(&self) -> rusqlite::Result<i64> {
        self.db.query_row("SELECT COALESCE(SUM(fe), 0) AS n FROM receipt", [], |r| r.get(0))
    }

    /// Начало активной сессии; без сессий создаётся первая с начала истории (0).
    pub fn active_session_start(&self) -> rusqlite::Result<i64> {
        let row: Option<i64> = self
            .db
            .query_row("SELECT started_at FROM session WHERE ended_at IS NULL ORDER BY id DESC LIMIT 1", [], |r| r.get(0))
            .optional()?;
        if let Some(t) = row {
            return Ok(t);
        }
        self.db.execute("INSERT INTO session (started_at, ended_at) VALUES (0, NULL)", [])?;
        Ok(0)
    }

    /// Сброс: активная сессия закрывается, новая — с этого момента. Забеги не трогаются.
    pub fn reset_session(&self, now: i64) -> rusqlite::Result<i64> {
        in_tx(&self.db, |db| {
            db.execute("UPDATE session SET ended_at = ? WHERE ended_at IS NULL", [now])?;
            db.execute("INSERT INTO session (started_at, ended_at) VALUES (?, NULL)", [now])?;
            Ok(now)
        })
    }

    /// Все сессии в порядке создания; активная — с ended_at = None.
    pub fn list_sessions(&self) -> rusqlite::Result<Vec<Session>> {
        let mut st = self.db.prepare("SELECT started_at, ended_at FROM session ORDER BY id")?;
        let v = st.query_map([], |r| Ok(Session { started_at: r.get(0)?, ended_at: r.get(1)? }))?.collect();
        v
    }

    /// Последняя закрытая сессия — для строки «прошлая сессия».
    pub fn previous_session(&self) -> rusqlite::Result<Option<(i64, i64)>> {
        self.db
            .query_row(
                "SELECT started_at, ended_at FROM session WHERE ended_at IS NOT NULL ORDER BY ended_at DESC, id DESC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
    }
}

/// Копия базы целиком (`VACUUM INTO`): работает и с живой базой в режиме WAL,
/// которую в этот момент держит другой процесс; источник не меняется.
pub fn copy_database(src: &Path, dst: &Path) -> rusqlite::Result<()> {
    let db = Connection::open_with_flags(src, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    db.execute("VACUUM INTO ?1", [dst.to_string_lossy()])?;
    Ok(())
}
