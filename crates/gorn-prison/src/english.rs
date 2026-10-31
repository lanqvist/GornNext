//! Подсказки на английском — только для показа. Строки подсказок внутри
//! остаются русскими: по ним `move_key`, журнал фактов, кадры и `gorn-cli
//! exam`/`audit`, и перевод не должен их трогать. Фразы подсказок — из
//! конечного набора (`hint.rs`, `advisor.rs`, `policy/instant.rs`), поэтому
//! перевод — таблица фраз и шаблоны «фраза + клетка/название».
//!
//! Названия орудий и подарков — из локализации клиента (`data/client-names.json`,
//! дамп файлов игры 18.10); тест `names_match_client` держит таблицы равными ему.
//! Не переводить названия самим: 31.10 из 35 «дословных» 14 не совпали с клиентом,
//! и OCR карт выбора не узнавал «Felon's Dog Tag» (в таблице был «Convict's Dog Tag»).

use crate::model::{GIFT_NAMES, TOOL_NAMES};

/// Английские названия орудий (по номеру, как TOOL_NAMES).
pub const TOOL_NAMES_EN: [&str; 31] = [
    "Forged Key",
    "Lost Ring of Keys",
    "Felon's Dog Tag",
    "Executioner's Blacklist",
    "Searing Branding Iron",
    "Sorting Hook",
    "Short-Chained Shackles",
    "Long-Chained Shackles",
    "Burning Stake",
    "Barbed Whip",
    "Rusty Saw",
    "Maddened Beast's Barbed Collar",
    "Raging Beast's Agonizing Muzzle",
    "Avenger's Casket",
    "Crowded Beastcage",
    "Skewered Rat Heads: Colossal Rat",
    "Iron Crown of the Prison King: Berserk Lion",
    "Elephant-Foot Maul: Mammoth",
    "Fanged Guillotine: Alpha Wolf",
    "Ferocious Beast's Ravenous Chains",
    "Hangman's Noose",
    "Iron Pestle",
    "Warden's Scales",
    "Gaoler's Execution Axe",
    "Severed Iron Maiden",
    "Scapegoat's Guillotine",
    "Iron Tongs of Lust: Mammoth",
    "Filthy Claws: Alpha Wolf",
    "Squirming Iron Box: Colossal Rat",
    "Kin's Coin of Fate",
    "Blood-Stained Trap: Berserk Lion",
];

pub const GIFT_NAMES_EN: [&str; 4] = [
    "Three-Headed Thumb Ring",
    "Brass Horn of Strife",
    "Crimson Vintage",
    "Brand of Original Sin",
];

/// Целые фразы: подписи ходов, следующие шаги, причины.
const PHRASES: &[(&str, &str)] = &[
    ("Отменить", "Cancel"),
    ("Пожирание", "Devour"),
    ("цепочка", "chain"),
    ("Остановить цепочку (Отменить)", "Stop the chain (Cancel)"),
    ("остановить цепочку", "stop the chain"),
    ("Заменить", "Replace"),
    ("Завершить день", "End day"),
    ("Выслеживание", "Hunt"),
    ("добыча", "prey"),
    ("Наградить", "Bestow"),
    ("Подстрекнуть", "Instigate"),
    ("Принять событие", "Accept the event"),
    ("Пропустить событие", "Skip the event"),
    ("Обновить выбор", "Reroll"),
    ("В допросную", "To Interrogation"),
    ("Заменить узника", "Replace a prisoner"),
    ("Наградить случайного узника", "Bestow a random prisoner"),
    // причины ходов (`instant.rs`, часть после «: ») и советника
    ("цепочка к призматическому", "chain to a Prismatic"),
    ("узник съест соседа", "the prisoner eats a neighbor"),
    ("событие", "event"),
    ("новый узник", "new prisoner"),
    ("дальше невыгодно", "not worth going on"),
    ("все три карты слабые", "all three cards are weak"),
    ("события за дверями", "events behind the doors"),
    ("одну карту не узнал", "one card not recognized"),
    ("уточнено", "refined"),
    (
        "Подарок: нажми «Посмотреть состояние тюрьмы» и вернись — подскажу, какой взять",
        "Gift: open the prison status and come back — I'll tell you which to take",
    ),
    ("Не узнал карты выбора — выбери сам", "Didn't recognize the cards — pick yourself"),
];

/// «Фраза + клетка»: русское начало → английское.
const WITH_CELL: &[(&str, &str)] = &[
    ("Пожирание: съесть ", "Devour: eat "),
    ("Цепочка: съесть ", "Chain: eat "),
    ("Метка добычи: ", "Mark prey: "),
    ("Пометить ", "Mark "),
    ("Съесть ", "Eat "),
    ("затем ", "then "),
    ("Выбери ", "Pick "),
    ("Иди в ", "Go to "),
    ("узник в камере ", "prisoner in cell "),
    ("в камеру ", "to cell "),
    ("камера ", "cell "),
];

fn cell(s: &str) -> Option<String> {
    if s == "допросную" {
        return Some("Interrogation".into());
    }
    let n = s.strip_prefix('№').unwrap_or(s);
    (!n.is_empty() && n.chars().all(|c| c.is_ascii_digit())).then(|| format!("#{n}"))
}

/// Название орудия или подарка, как его пишет подсказка, → английское.
fn name(s: &str) -> Option<&'static str> {
    if let Some(i) = TOOL_NAMES.iter().position(|t| t.split(':').next() == Some(s)) {
        return Some(TOOL_NAMES_EN[i]);
    }
    GIFT_NAMES.iter().position(|g| *g == s).map(|i| GIFT_NAMES_EN[i])
}

fn phrase(s: &str) -> Option<&'static str> {
    PHRASES.iter().find(|(ru, _)| *ru == s).map(|(_, en)| *en)
}

/// Одна часть подсказки (между « · »).
fn segment(s: &str) -> String {
    if let Some(en) = phrase(s) {
        return en.into();
    }
    // «Отменить, затем «Заменить»» и «затем «Завершить день»»
    for (ru, en) in [("Отменить, затем «", "Cancel, then “"), ("затем «", "then “")] {
        if let Some(inner) = s.strip_prefix(ru).and_then(|r| r.strip_suffix('»')) {
            return format!("{en}{}”", phrase(inner).unwrap_or(inner));
        }
    }
    for (ru, en) in WITH_CELL {
        if let Some(c) = s.strip_prefix(ru).and_then(cell) {
            return format!("{en}{c}");
        }
    }
    if let Some(n) = s.strip_prefix("Взять: ").and_then(name) {
        return format!("Take: {n}");
    }
    // Причина, обрезанная многоточием (`fit`): фраза по началу.
    if let Some(head) = s.strip_suffix('…').filter(|h| !h.is_empty()) {
        if let Some((_, en)) = PHRASES.iter().find(|(ru, _)| ru.starts_with(head)) {
            return (*en).into();
        }
    }
    s.into()
}

/// Строка подсказки (заголовок, подпись или текст с « · ») на английском;
/// незнакомые части остаются как есть.
pub fn hint_line(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    s.split(" · ").map(segment).collect::<Vec<_>>().join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::{A_GIFT, A_PATROL, A_REROLL, A_TOOL};
    use crate::hint::{hint, HintContext};
    use crate::model::{objective_by_title, tool_by_title, EVENT_IDS, OBJECTIVE_IDS, OBJECTIVE_TITLES, TOOL_IDS};
    use serde_json::Value;

    fn client() -> Value {
        serde_json::from_str(include_str!("../data/client-names.json")).unwrap()
    }

    fn rows<'a>(v: &'a Value, key: &str) -> Vec<(&'a str, &'a str, &'a str)> {
        v[key]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| (r["id"].as_str().unwrap_or(""), r["ru"].as_str().unwrap(), r["en"].as_str().unwrap()))
            .collect()
    }

    /// Названия в коде — ровно как в локализации клиента (`data/client-names.json`).
    #[test]
    fn names_match_client() {
        let c = client();
        let tools = rows(&c, "tools");
        assert_eq!(tools.len(), TOOL_IDS.len());
        for (i, (id, ru, en)) in tools.into_iter().enumerate() {
            assert_eq!((id, ru, en), (TOOL_IDS[i], TOOL_NAMES[i], TOOL_NAMES_EN[i]), "орудие {i}");
        }
        let gifts = rows(&c, "gifts");
        assert_eq!(gifts.len(), GIFT_NAMES.len());
        for (i, (_, ru, en)) in gifts.into_iter().enumerate() {
            assert_eq!((ru, en), (GIFT_NAMES[i], GIFT_NAMES_EN[i]), "подарок {i}");
        }
        for (i, (id, ru, en)) in rows(&c, "objectives").into_iter().enumerate() {
            let [r, e] = OBJECTIVE_TITLES[i];
            assert_eq!((id, ru, en), (OBJECTIVE_IDS[i], format!("Цель: {r}").as_str(), format!("Objective: {e}").as_str()));
        }
        let events: Vec<&str> = rows(&c, "events").into_iter().map(|r| r.0).collect();
        assert_eq!(events, EVENT_IDS);
    }

    /// Каждое название клиента читается обратно (OCR карт выбора и цели): целиком,
    /// обрезанное многоточием, в две строки и с ошибкой в одной букве.
    #[test]
    fn every_client_name_reads_back() {
        let variants = |name: &str| -> Vec<String> {
            let ch: Vec<char> = name.chars().collect();
            let cut = (ch.len() * 2 / 3).max(8).min(ch.len());
            let mut typo = ch.clone();
            let k = ch.len() / 2;
            typo[k] = if typo[k] == 'x' { 'y' } else { 'x' };
            let wrap = match name.find(' ') {
                Some(i) => format!("{}\n{}", &name[..i], &name[i + 1..]),
                None => name.to_string(),
            };
            vec![name.into(), ch[..cut].iter().collect::<String>() + "...", wrap, typo.into_iter().collect()]
        };
        let c = client();
        for (i, (id, ru, en)) in rows(&c, "tools").into_iter().enumerate() {
            for name in [ru, en] {
                for v in variants(name) {
                    assert_eq!(tool_by_title(&v), Some(i as u32), "{id}: «{v}»");
                }
            }
        }
        for (i, (id, ru, en)) in rows(&c, "objectives").into_iter().enumerate() {
            // Заголовок цели клиент переносит, а не обрезает.
            for name in [ru, en] {
                for v in variants(name).into_iter().filter(|v| !v.ends_with("...")) {
                    assert_eq!(objective_by_title(&v), Some(i as i32), "{id}: «{v}»");
                }
            }
        }
        // Ошибки OCR в приставке (кадры 31.10).
        assert_eq!(objective_by_title("0bjective: Survival of the Fittest"), Some(1));
        assert_eq!(objective_by_title("0bjective Survival of the
Fittest"), Some(1));
        assert_eq!(objective_by_title("Torture Instrument"), None);
        assert_eq!(objective_by_title(""), None);
    }

    fn code(kind: i32, arg: i32) -> i32 {
        (kind << 6) | arg
    }

    #[test]
    fn lines() {
        assert_eq!(hint_line("Пожирание: съесть №6"), "Devour: eat #6");
        assert_eq!(hint_line("Иди в №4 · событие"), "Go to #4 · event");
        assert_eq!(hint_line("Иди в допросную"), "Go to Interrogation");
        assert_eq!(hint_line("затем №6 · одну карту не узнал"), "then #6 · one card not recognized");
        assert_eq!(hint_line("Отменить, затем «Завершить день»"), "Cancel, then “End day”");
        assert_eq!(hint_line("Взять: Ржавая пила"), "Take: Rusty Saw");
        assert_eq!(hint_line("Взять: Багряное вино"), "Take: Crimson Vintage");
        assert_eq!(hint_line("цепочка к призма…"), "chain to a Prismatic");
        assert_eq!(hint_line("что-то новое"), "что-то новое");
    }

    /// Каждый ход, который умеет подписать `hint`, переводится целиком — без кириллицы.
    #[test]
    fn every_move_translates() {
        let ctx = HintContext::default();
        let mut codes = vec![];
        codes.extend((0..31).map(|i| code(A_TOOL, i)));
        codes.extend((0..4).map(|i| code(A_GIFT, i)));
        codes.extend((0..25).map(|i| code(A_PATROL, i)));
        for kind in 0..=A_REROLL {
            codes.push(code(kind, 3));
        }
        for c in codes {
            let h = hint(c, &ctx);
            for s in [&h.text, &h.title, &h.sub] {
                let en = hint_line(s);
                assert!(!en.chars().any(|ch| ('а'..='я').contains(&ch.to_lowercase().next().unwrap())), "{c}: «{s}» → «{en}»");
            }
        }
        for (ru, _) in PHRASES {
            assert!(phrase(ru).is_some());
        }
    }
}
