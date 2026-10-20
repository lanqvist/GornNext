//! Подписи сезонов и классов персонажей. Перенос `core/seasons.ts` и `core/heroes.ts`.

/// Язык подписей.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Lang {
    #[default]
    Ru,
    En,
}

fn tr(lang: Lang, ru: String, en: String) -> String {
    if lang == Lang::En {
        en
    } else {
        ru
    }
}

/// Номер растёт на сотню за сезон (1501 — SS13, 1601 — SS14); имена — собственные, без перевода.
const SEASONS: [(i64, &str); 4] = [(1301, "SS11 Vorax"), (1401, "SS12 Lunaria"), (1501, "SS13 Afterlight"), (1601, "SS14 Tartarus")];

/// Постоянный (несезонный) сервер.
const PERMANENT_SEASON: i64 = 1;

pub fn season_name(season_id: Option<i64>, lang: Lang) -> Option<String> {
    let id = season_id?;
    if id == PERMANENT_SEASON {
        return Some(tr(lang, "Постоянный сервер".into(), "Permanent server".into()));
    }
    if let Some((_, name)) = SEASONS.iter().find(|(s, _)| *s == id) {
        return Some(name.to_string());
    }
    if id > 1000 && id % 100 == 1 {
        return Some(format!("SS{}", id.div_euclid(100) - 2));
    }
    Some(tr(lang, format!("Сезон {id}"), format!("Season {id}")))
}

/// Короткая метка для списков: «SS14».
pub fn season_short(season_id: Option<i64>, lang: Lang) -> Option<String> {
    let name = season_name(season_id, lang)?;
    let digits = name.strip_prefix("SS").map(|r| r.bytes().take_while(u8::is_ascii_digit).count()).unwrap_or(0);
    Some(if digits > 0 { name[..2 + digits].to_string() } else { name })
}

/// Классы по HeroId: публичного соответствия нет, словарь пополняется по живым логам.
const HEROES: [(i64, &str, &str); 1] = [
    // Подтверждено владельцем персонажа на живом логе SS13.
    (1700, "Селена", "Selena"),
];

/// Имя класса для показа. Неизвестный id — честное «Герой N».
pub fn hero_name(hero_id: Option<i64>, lang: Lang) -> Option<String> {
    let id = hero_id?;
    Some(match HEROES.iter().find(|(h, _, _)| *h == id) {
        Some((_, ru, en)) => tr(lang, ru.to_string(), en.to_string()),
        None => tr(lang, format!("Герой {id}"), format!("Hero {id}")),
    })
}
