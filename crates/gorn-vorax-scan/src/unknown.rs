//! Что на доске не узнано наверняка — по этому Горн откладывает кадр в «незнакомое», чтобы дополнить эталоны:
//! русские названия (data/names-ru.json), картинки карт, разметку экрана.

use crate::data::ScanData;
use crate::reading::{Problem, Reading};
use gorn_vorax::advisor::ScreenPhase;

fn phase_key(p: ScreenPhase) -> &'static str {
    match p {
        ScreenPhase::Potion => "potion",
        ScreenPhase::Instrument => "instrument",
        ScreenPhase::Protocol => "protocol",
    }
}

fn russian(t: &str) -> bool {
    t.chars().any(|c| ('а'..='я').contains(&c.to_lowercase().next().unwrap_or(c)))
}

/// Первые строки прочитанного заголовка (сам заголовок, без описания), в одну строку.
fn head(t: &str) -> String {
    t.lines().map(str::trim).filter(|l| !l.is_empty()).take(2).collect::<Vec<_>>().join(" ")
}

/// Причины отложить кадр; пусто — всё узнано. `texts`: что OCR прочитал на заголовке каждой карты, слева
/// направо (пусто без OCR).
pub fn unknowns(r: &Reading, texts: &[String], data: &ScanData) -> Vec<String> {
    let mut why = Vec::new();
    let mut named = Vec::new();
    for p in &r.problems {
        match p {
            Problem::UnknownArt { field, text, .. } => {
                named.push(field.clone());
                match text.as_deref().map(head).filter(|t| !t.is_empty()) {
                    Some(t) => why.push(format!("{field}: карта не узнана, OCR: «{t}»")),
                    None => why.push(format!("{field}: карта не узнана")),
                }
            }
            Problem::Unreadable { field } => {
                named.push(field.clone());
                why.push(format!("{field}: не прочитано"));
            }
            Problem::UnknownName { name } => why.push(format!("неизвестное имя: {name}")),
            _ => {}
        }
    }
    // Вид выбора по заголовку сравнивается с английским образцом; когда все карты узнаны, их названия и так
    // говорят, что это за выбор.
    let hand: Vec<_> = r.fields.iter().filter(|(k, _)| k.starts_with("hand") && k.len() == 5).collect();
    let cards_sure = !hand.is_empty() && hand.iter().all(|(_, f)| f.sure);
    for (k, f) in &r.fields {
        let watched = (k == "phase" && !cards_sure) || k == "round" || (k.starts_with("hand") && k.len() == 5);
        if watched && !f.sure && !named.contains(k) {
            let ocr = k.strip_prefix("hand").and_then(|j| j.parse::<usize>().ok()).and_then(|j| texts.get(j)).map(|t| head(t)).filter(|t| !t.is_empty());
            match ocr {
                Some(t) => why.push(format!("{k}: не наверняка ({}), OCR: «{t}»", f.guess.as_deref().unwrap_or("?"))),
                None => why.push(format!("{k}: не наверняка ({})", f.guess.as_deref().unwrap_or("?"))),
            }
        }
    }
    // OCR прочитал русский заголовок, а русского названия у карты нет: её узнала только картинка
    let ru = data.ru.get(phase_key(r.state.phase));
    for (j, t) in texts.iter().enumerate() {
        let key = format!("hand{j}");
        let Some(guess) = r.fields.get(&key).and_then(|f| f.guess.as_deref()) else { continue };
        if russian(t) && !ru.is_some_and(|m| m.contains_key(guess)) && !named.contains(&key) {
            why.push(format!("{key}: нет русского названия для «{guess}», OCR: «{}»", head(t)));
        }
    }
    why
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reading::FieldRead;
    use crate::scan_data_ru;

    fn reading(phase: &str, hand: &[(&str, bool)]) -> Reading {
        let mut r: Reading = serde_json::from_value(serde_json::json!({
            "state": { "tanks": [null, null, null, null, null, null], "hand": [], "instruments": [], "phase": phase },
            "fields": {}, "problems": []
        }))
        .unwrap();
        for (j, (name, sure)) in hand.iter().enumerate() {
            r.fields.insert(format!("hand{j}"), FieldRead { sure: *sure, guess: Some(name.to_string()), score: None, second: None });
        }
        r.fields.insert("round".into(), FieldRead::sure(true));
        r
    }

    #[test]
    fn a_known_board_has_nothing_to_learn() {
        let r = reading("potion", &[("Mutagenic Powder", true), ("Slough Solution", true)]);
        let texts = ["Мутагенный порошок\nВыбирает 1-2".to_string(), "Раствор для линьки".to_string()];
        assert!(unknowns(&r, &texts, scan_data_ru()).is_empty());
    }

    #[test]
    fn a_card_known_only_by_its_picture_is_kept_for_its_russian_name() {
        let r = reading("potion", &[("Brain Fog Tincture", true)]);
        let why = unknowns(&r, &["Настойка помутнения\nВыбирает 1".to_string()], scan_data_ru());
        assert_eq!(why, ["hand0: нет русского названия для «Brain Fog Tincture», OCR: «Настойка помутнения Выбирает 1»"]);
    }

    #[test]
    fn an_unsure_header_with_all_cards_known_is_not_kept() {
        let mut r = reading("instrument", &[("Canine Teeth File", true)]);
        r.fields.insert("phase".into(), FieldRead { sure: false, guess: Some("instrument".into()), score: None, second: None });
        assert!(unknowns(&r, &[], scan_data_ru()).is_empty());
    }

    #[test]
    fn an_unsure_card_carries_its_title() {
        let r = reading("protocol", &[("Immunosuppressive Experiment", false)]);
        let why = unknowns(&r, &["Эксперимент по\nпересадке кожи\nИгрок получает".to_string()], scan_data_ru());
        assert_eq!(why, ["hand0: не наверняка (Immunosuppressive Experiment), OCR: «Эксперимент по пересадке кожи»"]);
    }

    #[test]
    fn unsure_cards_and_rounds_are_kept() {
        let mut r = reading("potion", &[("Xeno-Hormone", false)]);
        r.fields.insert("round".into(), FieldRead { sure: false, guess: Some("5".into()), score: None, second: None });
        let why = unknowns(&r, &[], scan_data_ru());
        assert_eq!(why, ["hand0: не наверняка (Xeno-Hormone)", "round: не наверняка (5)"]);
    }
}
