//! Игровые названия карт по ассету и LevelId из лога. Перенос `core/mapNames.ts`.
//!
//! Ассет — художественная сцена, а не зона: одна сцена переиспользуется
//! в разных зонах. Названия — из справочника `data/maps.json`; неизвестная
//! карта названия не получает: ошибочное название хуже пиньиня.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MapCatalogue {
    /// Шаблон ассета → «План - Зона». Шаблон ищется внутри имени ассета.
    #[serde(default)]
    pub assets: IndexMap<String, String>,
    /// Сцены, общие для нескольких зон: шаблон → {две последние цифры LevelId → название}.
    #[serde(default)]
    pub ambiguous: IndexMap<String, IndexMap<String, String>>,
    /// Особые зоны (боссы, испытания) — по LevelId целиком.
    #[serde(default)]
    pub level_ids: IndexMap<String, String>,
    /// Английское название зоны → русское.
    #[serde(default)]
    pub ru: IndexMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MapName {
    /// Английское название зоны, без плана.
    pub en: String,
    /// Русское; None — перевода в справочнике нет.
    pub ru: Option<String>,
    /// План по-английски; None — у зоны плана нет.
    pub plane: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct MapNames {
    catalogue: MapCatalogue,
    ru: IndexMap<String, String>,
    /// Шаблоны от длинных к коротким.
    patterns: Vec<(String, String)>,
}

impl MapNames {
    pub fn new(catalogue: MapCatalogue) -> Self {
        // Регистр у источников расходится: «Wall of the Pure» и «Wall of The Pure».
        let ru = catalogue.ru.iter().map(|(en, ru)| (en.to_lowercase(), ru.clone())).collect();
        let mut patterns: Vec<(String, String)> = catalogue.assets.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        patterns.sort_by_key(|p| std::cmp::Reverse(p.0.encode_utf16().count()));
        MapNames { catalogue, ru, patterns }
    }

    /// Справочник из JSON (`data/maps.json`).
    pub fn from_json(text: &str) -> serde_json::Result<Self> {
        Ok(Self::new(serde_json::from_str(text)?))
    }

    /// Встроенный справочник.
    pub fn builtin() -> Self {
        Self::from_json(include_str!("../data/maps.json")).expect("data/maps.json")
    }

    pub fn empty() -> Self {
        Self::default()
    }

    pub fn lookup(&self, asset: Option<&str>, level_id: i64) -> Option<MapName> {
        self.resolve(asset, level_id).map(|full| self.split(full))
    }

    /// Английское «План - Зона».
    fn resolve(&self, asset: Option<&str>, level_id: i64) -> Option<&str> {
        if let Some(exact) = self.catalogue.level_ids.get(&level_id.to_string()).filter(|s| !s.is_empty()) {
            return Some(exact);
        }
        let asset = asset?;
        // Сцена, общая для нескольких зон, различается двумя последними цифрами LevelId.
        for (pattern, by_suffix) in &self.catalogue.ambiguous {
            if !asset.contains(pattern.as_str()) {
                continue;
            }
            if let Some(name) = by_suffix.get(&(level_id % 100).to_string()).filter(|s| !s.is_empty()) {
                return Some(name);
            }
        }
        self.patterns.iter().find(|(p, _)| asset.contains(p.as_str())).map(|(_, n)| n.as_str())
    }

    fn split(&self, full: &str) -> MapName {
        let (plane, en) = match full.find(" - ") {
            Some(at) => (Some(full[..at].to_string()), full[at + 3..].to_string()),
            None => (None, full.to_string()),
        };
        MapName { ru: self.ru.get(&en.to_lowercase()).cloned(), en, plane }
    }
}
