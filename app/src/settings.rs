//! Настройки в том же `settings.json`, что у Горна на Electron (`main/settings.ts`):
//! версия схемы 1, те же поля и умолчания. Файл держится как JSON целиком —
//! поля, которых эта версия не знает (из более новой), не теряются.

use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

const SCHEMA_VERSION: i64 = 1;
pub const OVERLAY_MODES: [&str; 4] = ["minimal", "compact", "full", "strip"];

fn defaults(ui: &str) -> Value {
    json!({
        "uiLanguage": ui,
        "itemNames": if ui == "en" { "en" } else { "ru" },
        "communityPrices": true,
        // В Горне Next подсказки в тюрьме включены: ради них он и запускался первым.
        "prisonHints": true,
        "prisonSaveShots": true,
        // Все экраны тюрьмы подряд (для перевода на английский клиент) — по желанию.
        "prisonCollect": false,
        // Подсказки в Vorax и уровень Интерна Шелли (1–6): от него перебросы
        // инструментов, которых на доске зелий не видно.
        "voraxHints": true,
        "voraxShelley": 4,
        "logDir": null,
        "playerFilter": null,
        "allSeasons": false,
        "excludedItems": [],
        "minDropFe": 0,
        "closeToTray": false,
        "autoStart": false,
        "overlay": {
            "enabled": false,
            "mode": "compact",
            "locked": true,
            "opacity": 0.9,
            "fontScale": 1,
            "placement": null,
            "popups": true,
            "sound": true,
            "volume": 0.5,
            "divineFe": 20,
            "jackpotFe": 50,
            "exaltedFe": 5,
            "introShown": false,
            "goalFe": 0,
            "heat": true
        }
    })
}

/// Язык новой установки по системной локали: русская — русский, иначе английский.
pub fn language_for_locale(locale: Option<&str>) -> &'static str {
    if locale.is_some_and(|l| l.to_lowercase().starts_with("ru")) {
        "ru"
    } else {
        "en"
    }
}

pub struct Settings {
    path: PathBuf,
    data: Value,
}

impl Settings {
    pub fn load(data_dir: &Path, locale: Option<&str>) -> Self {
        let path = data_dir.join("settings.json");
        let fresh = defaults(language_for_locale(locale));
        let raw: Value = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or(Value::Null);
        let data = if raw["version"].as_i64() == Some(SCHEMA_VERSION) && raw["settings"].is_object()
        {
            normalize(&raw["settings"], &fresh)
        } else {
            // Файл первых версий Горна Next: только флаги тюрьмы.
            let mut d = fresh;
            for k in ["prisonHints", "prisonSaveShots"] {
                if let Some(b) = raw[k].as_bool() {
                    d[k] = json!(b);
                }
            }
            d
        };
        Settings { path, data }
    }

    pub fn get(&self) -> &Value {
        &self.data
    }

    pub fn overlay(&self) -> &Value {
        &self.data["overlay"]
    }

    pub fn str(&self, key: &str) -> Option<&str> {
        self.data[key].as_str()
    }

    pub fn bool(&self, key: &str) -> bool {
        self.data[key].as_bool().unwrap_or(false)
    }

    pub fn set(&mut self, key: &str, value: Value) {
        self.data[key] = value;
        self.save();
    }

    pub fn excluded(&self) -> Vec<i64> {
        self.data["excludedItems"]
            .as_array()
            .map(|a| a.iter().filter_map(Value::as_i64).collect())
            .unwrap_or_default()
    }

    pub fn set_excluded(&mut self, id: i64, excluded: bool) -> Vec<i64> {
        let mut set: std::collections::BTreeSet<i64> = self.excluded().into_iter().collect();
        if excluded {
            set.insert(id);
        } else {
            set.remove(&id);
        }
        let v: Vec<i64> = set.into_iter().collect();
        self.set("excludedItems", json!(v));
        v
    }

    pub fn patch_overlay(&mut self, patch: &Map<String, Value>) -> Value {
        if let Some(o) = self.data["overlay"].as_object_mut() {
            for (k, v) in patch {
                o.insert(k.clone(), v.clone());
            }
        }
        self.save();
        self.data["overlay"].clone()
    }

    /// Запись через временный файл: краш посреди записи не оставит битый JSON.
    fn save(&self) {
        if let Some(d) = self.path.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let tmp = self.path.with_extension("json.tmp");
        let body = json!({ "version": SCHEMA_VERSION, "settings": self.data });
        if std::fs::write(
            &tmp,
            serde_json::to_string_pretty(&body).unwrap_or_default(),
        )
        .is_ok()
        {
            let _ = std::fs::rename(&tmp, &self.path);
        }
    }
}

/// Поля из файла поверх умолчаний — с теми же проверками, что у эталона.
fn normalize(s: &Value, fresh: &Value) -> Value {
    let mut d = fresh.clone();
    if let Some(obj) = s.as_object() {
        for (k, v) in obj {
            if k != "overlay" {
                d[k.as_str()] = v.clone();
            }
        }
    }
    if !matches!(s["uiLanguage"].as_str(), Some("ru" | "en")) {
        d["uiLanguage"] = json!("ru");
    }
    for k in ["prisonHints", "prisonSaveShots", "voraxHints"] {
        if !d[k].is_boolean() {
            d[k] = fresh[k].clone();
        }
    }
    d["allSeasons"] = json!(s["allSeasons"].as_bool() == Some(true));
    // Выбор персонажа убран: старый фильтр из файла не действует.
    d["playerFilter"] = Value::Null;
    let excluded: Vec<i64> = s["excludedItems"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_i64).collect())
        .unwrap_or_default();
    d["excludedItems"] = json!(excluded);
    if !s["minDropFe"].as_f64().is_some_and(|v| v >= 0.0) {
        d["minDropFe"] = json!(0);
    }
    if let (Some(o), Some(src)) = (d["overlay"].as_object_mut(), s["overlay"].as_object()) {
        for (k, v) in src {
            o.insert(k.clone(), v.clone());
        }
        // Незнакомый режим (из более новой версии) — компактный.
        if !src
            .get("mode")
            .and_then(Value::as_str)
            .is_some_and(|m| OVERLAY_MODES.contains(&m))
        {
            o.insert("mode".into(), json!("compact"));
        }
    }
    d
}

/// Первый запуск Горна Next рядом с Горном на Electron: база, настройки трекера
/// и кэш цен сообщества переезжают копией (сам Горн на Electron не трогается).
/// Возвращает, что перенесено.
pub fn import_from_electron(data_dir: &Path, old_dir: &Path) -> Vec<String> {
    let mut done = vec![];
    let db = data_dir.join("gorn.db");
    if db.exists() || !old_dir.join("gorn.db").exists() {
        return done;
    }
    let _ = std::fs::create_dir_all(data_dir);
    match gorn_core::storage::copy_database(&old_dir.join("gorn.db"), &db) {
        Ok(()) => done.push("gorn.db".into()),
        Err(e) => {
            eprintln!("[горн] перенос базы не удался: {e}");
            let _ = std::fs::remove_file(&db);
            return done;
        }
    }
    // Настройки трекера: свои флаги тюрьмы остаются своими.
    let old: Value = std::fs::read_to_string(old_dir.join("settings.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(Value::Null);
    if old["version"].as_i64() == Some(SCHEMA_VERSION) {
        let ours: Value = std::fs::read_to_string(data_dir.join("settings.json"))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or(Value::Null);
        let mut merged = old.clone();
        for k in ["prisonHints", "prisonSaveShots"] {
            merged["settings"][k] = ours[k].as_bool().map(Value::Bool).unwrap_or(json!(true));
        }
        if std::fs::write(
            data_dir.join("settings.json"),
            serde_json::to_string_pretty(&merged).unwrap_or_default(),
        )
        .is_ok()
        {
            done.push("settings.json".into());
        }
    }
    if let Ok(rd) = std::fs::read_dir(old_dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with("community-prices")
                && name.ends_with(".json")
                && std::fs::copy(e.path(), data_dir.join(&name)).is_ok()
            {
                done.push(name);
            }
        }
    }
    done
}
