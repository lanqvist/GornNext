//! Подсказки на английском — только для показа. Строки подсказок внутри
//! остаются русскими: по ним `move_key`, журнал фактов, кадры и `gorn-cli
//! exam`/`audit`, и перевод не должен их трогать. Фразы подсказок — из
//! конечного набора (`hint.rs`, `advisor.rs`, `policy/instant.rs`), поэтому
//! перевод — таблица фраз и шаблоны «фраза + клетка/название».
//!
//! Названия орудий и подарков — предварительные (дословный перевод русских),
//! кроме сверенных с английским клиентом (кадры 31.10): Long-Chained Shackles,
//! Short-Chained Shackles, Sorting Hook, Severed Iron Maiden, Warden's Scales, Rusty Saw,
//! Searing Branding Iron, Maddened Beast's Barbed Collar, Avenger's Casket, Kin's Coin of Fate. Остальные заменят названия с кадров сбора (`collect.rs`).

use crate::model::{GIFT_NAMES, TOOL_NAMES};

/// Английские названия орудий (по номеру, как TOOL_NAMES).
pub const TOOL_NAMES_EN: [&str; 31] = [
    "Forged Key",
    "Lost Key Ring",
    "Convict's Dog Tag",
    "Executioner's Blacklist",
    "Searing Branding Iron",
    "Sorting Hook",
    "Short-Chained Shackles",
    "Long-Chained Shackles",
    "Burning Stake",
    "Spiked Iron Whip",
    "Rusty Saw",
    "Maddened Beast's Barbed Collar",
    "Rabid Beast's Torture Muzzle",
    "Avenger's Casket",
    "Overcrowded Beast Cage",
    "Rat-Head Skewer",
    "Prison King's Iron Crown",
    "Elephant-Foot Maul",
    "Wolf-Fang Guillotine",
    "Ravenous Beast's Choke Chain",
    "Hangman's Noose",
    "Iron Pestle",
    "Warden's Scales",
    "Warden's Execution Axe",
    "Severed Iron Maiden",
    "Scapegoat Guillotine",
    "Tongs of Vice",
    "Filthy Iron Claw",
    "Swarming Iron Box",
    "Kin's Coin of Fate",
    "Bloody Trap",
];

pub const GIFT_NAMES_EN: [&str; 4] = [
    "Three-Headed Ring",
    "Bronze War Horn",
    "Crimson Wine",
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
        assert_eq!(hint_line("Взять: Багряное вино"), "Take: Crimson Wine");
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
