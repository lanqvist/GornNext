//! Язык интерфейса на стороне Rust: окна поверх игры, трей, подсказки.
//! Как `core/i18n.ts`: перевод рядом с оригиналом — `tr("Выход", "Quit")`.
//! Ставится из настройки `uiLanguage` при запуске и при её смене.

use std::sync::atomic::{AtomicBool, Ordering};

static EN: AtomicBool = AtomicBool::new(false);

pub fn set(lang: &str) {
    let en = lang == "en";
    EN.store(en, Ordering::Relaxed);
    gorn_vorax_scan::marks::set_english(en);
}

pub fn en() -> bool {
    EN.load(Ordering::Relaxed)
}

pub fn tr(ru: &'static str, en: &'static str) -> &'static str {
    if self::en() {
        en
    } else {
        ru
    }
}

/// Строка подсказки тюрьмы для показа (внутри подсказки русские).
pub fn hint(s: &str) -> String {
    if en() {
        gorn_prison::english::hint_line(s)
    } else {
        s.to_string()
    }
}
