//! Язык клиента игры — по её логу: при запуске `Localization@ Ini-LanguageCode = ru`,
//! после смены языка в настройках игры — запросы SDK с `sdkLang=en_US` и
//! `SetTextLanguageCodeen` (лог 31.10, смена ru → en без перезапуска игры).
//! Последний признак в логе и есть язык. От него — язык распознавания текста
//! (OCR) и названия карт Vorax; язык интерфейса Горна — отдельная настройка.

use std::path::Path;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

use regex::Regex;

const UNKNOWN: u8 = 0;
const RU: u8 = 1;
const EN: u8 = 2;

static LANG: AtomicU8 = AtomicU8::new(UNKNOWN);

fn marker() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"LanguageCode = ([a-z]{2})\b|sdkLang=([a-z]{2})_|SetTextLanguageCode([a-z]{2})\b").unwrap()
    })
}

/// Язык по строке лога, если она о нём.
fn of_line(l: &str) -> Option<u8> {
    let c = marker().captures(l)?;
    let code = c.iter().skip(1).flatten().next()?.as_str();
    Some(match code {
        "ru" => RU,
        "en" => EN,
        // Другие языки клиента: эталонов нет — как английский (OCR по-латински).
        _ => EN,
    })
}

/// Английский ли клиент; None — в логе ещё не было признака.
pub fn english() -> Option<bool> {
    match LANG.load(Ordering::Relaxed) {
        RU => Some(false),
        EN => Some(true),
        _ => None,
    }
}

/// Новые строки лога. true — язык сменился.
pub fn feed(lines: &[String]) -> bool {
    let Some(l) = lines.iter().rev().find_map(|l| of_line(l)) else {
        return false;
    };
    let was = LANG.swap(l, Ordering::Relaxed);
    if was != l {
        crate::log(&format!("[горн] язык клиента игры: {}", if l == EN { "английский" } else { "русский" }));
    }
    was != l
}

/// Язык по уже записанному логу (при запуске Горна посреди сессии игры).
pub fn scan(log: &Path) {
    let Ok(bytes) = std::fs::read(log) else {
        return;
    };
    let text = String::from_utf8_lossy(&bytes);
    let lines: Vec<String> = text.lines().filter(|l| l.contains("anguage") || l.contains("sdkLang=")).map(str::to_string).collect();
    // Строки, пришедшие, пока читали файл, новее — их не перебивать.
    if LANG.load(Ordering::Relaxed) == UNKNOWN {
        feed(&lines);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markers() {
        assert_eq!(of_line("[Game] Localization@ Ini-LanguageCode = ru"), Some(RU));
        assert_eq!(of_line("[Game] Localization@ReadOptionsLanguageFromIni ! LanguageCode = en"), Some(EN));
        assert_eq!(of_line("Verbose: https://x.xd.com/api?clientId=1&sdkVer=7.8.0&sdkLang=en_US&appVer=17"), Some(EN));
        assert_eq!(of_line("ChatMsgMgr@:InitPublicChannel SettingMgr:SetTextLanguageCodeen"), Some(EN));
        assert_eq!(of_line("Accept-Language: en_US"), None);
        assert_eq!(of_line("LogTemp: normal-click Btn_S16Board_RouteCell_3"), None);
    }
}
