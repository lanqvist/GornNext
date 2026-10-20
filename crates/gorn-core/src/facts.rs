//! Журнал фактов тюрьмы (этап 7, шаг 1) — запись: каждое решение советника в живом
//! забеге — позиция, оценки решателя по всем ходам, быстрый и показанный
//! совет. Следующая позиция того же забега — это «что вышло на самом деле»:
//! сделанный ход и случайный исход восстанавливаются по паре соседних фактов
//! при разборе (`gorn-cli learn`), а не в живой игре.
//!
//! Отдельная база `prison-facts.db` рядом с `gorn.db`: у трекера своя схема
//! и своя сверка с эталоном. Запись — в своём потоке: диск не задерживает совет.

use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS run (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  started_at    INTEGER NOT NULL,
  theme         INTEGER,
  objective     INTEGER,
  day_limit     INTEGER,
  tools         TEXT,
  last_at       INTEGER NOT NULL,
  last_day      INTEGER,
  last_storage  INTEGER,
  decisions     INTEGER NOT NULL DEFAULT 0,
  -- Версия Горна, чей советник давал советы: факты разных версий не смешивать.
  app           TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS fact (
  id        INTEGER PRIMARY KEY AUTOINCREMENT,
  run_id    INTEGER NOT NULL REFERENCES run(id),
  ts        INTEGER NOT NULL,
  shot      INTEGER,
  day       INTEGER,
  step      TEXT,
  warden    INTEGER,
  storage   INTEGER,
  -- Позиция движка (State.m) массивом JSON и её ключ.
  state     TEXT NOT NULL,
  key       TEXT,
  instant   INTEGER,
  advised   INTEGER,
  best      INTEGER,
  done      INTEGER,
  rounds    INTEGER,
  ms        REAL,
  -- Все ходы решателя: code, mean, gapSe, rounds, pruned, shown.
  options   TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_fact_run ON fact(run_id, ts);
";

/// Пауза дольше этого — точно другой забег, даже без выбора орудий в начале.
const NEW_RUN_GAP_MS: i64 = 3 * 3_600_000;

/// Сводка журнала для настроек: забегов с решениями и решений всего.
/// Журнала ещё нет — нули (файл не создаётся).
pub fn summary(path: &Path) -> rusqlite::Result<(i64, i64)> {
    if !path.exists() {
        return Ok((0, 0));
    }
    let db = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    db.query_row("SELECT COUNT(*), COALESCE(SUM(decisions), 0) FROM run WHERE decisions > 0", [], |r| Ok((r.get(0)?, r.get(1)?)))
}

pub struct Journal {
    db: Connection,
    /// Версия Горна, чей советник давал советы.
    app: String,
}

impl Journal {
    pub fn open(path: &Path, app: &str) -> rusqlite::Result<Self> {
        let db = Connection::open(path)?;
        db.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")?;
        db.execute_batch(SCHEMA)?;
        db.execute_batch("PRAGMA user_version = 1")?;
        Ok(Journal { db, app: app.to_string() })
    }

    /// Факт от советника; ts — время записи (мс UTC).
    pub fn add(&self, f: &Value, ts: i64) -> rusqlite::Result<i64> {
        let day = f["day"].as_i64();
        let last: Option<(i64, i64, Option<i64>)> = self
            .db
            .query_row("SELECT id, last_at, last_day FROM run ORDER BY id DESC LIMIT 1", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .optional()?;
        // Новый забег: выбор орудий в начале, долгая пауза или день назад.
        let fresh = f["fresh"].as_bool() == Some(true);
        let run = match last {
            Some((id, at, last_day)) if !fresh && ts - at < NEW_RUN_GAP_MS && !matches!((day, last_day), (Some(d), Some(l)) if d < l) => id,
            _ => {
                self.db.execute(
                    "INSERT INTO run (started_at, theme, objective, day_limit, tools, last_at, app) VALUES (?, ?, ?, ?, ?, ?, ?)",
                    params![
                        ts,
                        f["theme"].as_i64(),
                        f["objective"].as_i64(),
                        f["dayLimit"].as_i64(),
                        f["tools"].to_string(),
                        ts,
                        self.app.as_str(),
                    ],
                )?;
                self.db.last_insert_rowid()
            }
        };
        self.db.execute(
            "INSERT INTO fact (run_id, ts, shot, day, step, warden, storage, state, key, instant, advised, best, done, rounds, ms, options)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                run,
                ts,
                f["shot"].as_i64(),
                day,
                f["step"].as_str(),
                f["warden"].as_i64(),
                f["storage"].as_i64(),
                f["state"].to_string(),
                f["key"].as_str(),
                f["instant"].as_i64(),
                f["advised"].as_i64(),
                f["best"].as_i64(),
                f["done"].as_bool().map(i64::from),
                f["rounds"].as_i64(),
                f["ms"].as_f64(),
                f["options"].to_string(),
            ],
        )?;
        // Цель и орудия уточняются по ходу забега — у забега последние известные.
        self.db.execute(
            "UPDATE run SET last_at = ?, last_day = ?, last_storage = ?, decisions = decisions + 1,
               objective = COALESCE(?, objective), day_limit = COALESCE(?, day_limit), tools = ?
             WHERE id = ?",
            params![ts, day, f["storage"].as_i64(), f["objective"].as_i64(), f["dayLimit"].as_i64(), f["tools"].to_string(), run],
        )?;
        Ok(run)
    }
}

