//! CSV, который Excel открывает без мастера импорта. Перенос `core/csv.ts`.
//!
//! Excel берёт разделители из региональных настроек: русский ждёт «;» и
//! запятую в дробях, английский — «,» и точку. Без BOM Excel читает UTF-8
//! как cp1251.

use crate::labels::Lang;

#[derive(Clone, Debug, PartialEq)]
pub enum CsvCell {
    Text(String),
    Num(f64),
    Null,
}

/// `Math.round` из JS: половина — вверх.
pub fn js_round(x: f64) -> f64 {
    let r = x.floor();
    if x - r >= 0.5 {
        r + 1.0
    } else {
        r
    }
}

/// Число строкой, как `String(n)` в JS (кратчайшая запись, без «-0»).
pub fn js_num_str(x: f64) -> String {
    if x == 0.0 {
        return "0".into();
    }
    if x.is_nan() {
        return "NaN".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    let a = x.abs();
    if (1e-6..1e21).contains(&a) {
        return format!("{x}");
    }
    // Экспонента как в JS: «1e+21», «1.5e-7».
    let s = format!("{x:e}");
    let (m, e) = s.split_once('e').unwrap();
    let e: i32 = e.parse().unwrap();
    format!("{m}e{}{}", if e >= 0 { "+" } else { "-" }, e.abs())
}

fn cell(value: &CsvCell, sep: &str, decimal: &str) -> String {
    let text = match value {
        CsvCell::Null => return String::new(),
        // Без разделителя тысяч: Excel принял бы «1 234» за текст.
        CsvCell::Num(v) => js_num_str(js_round(v * 100.0) / 100.0).replacen('.', decimal, 1),
        CsvCell::Text(t) => t.clone(),
    };
    let needs_quotes = text.contains(sep) || text.contains(['"', '\r', '\n']);
    if needs_quotes {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text
    }
}

pub fn to_csv(header: &[String], rows: &[Vec<CsvCell>], dialect: Lang) -> String {
    let (sep, decimal) = if dialect == Lang::En { (",", ".") } else { (";", ",") };
    let mut lines: Vec<String> = vec![header.iter().map(|h| cell(&CsvCell::Text(h.clone()), sep, decimal)).collect::<Vec<_>>().join(sep)];
    for r in rows {
        lines.push(r.iter().map(|v| cell(v, sep, decimal)).collect::<Vec<_>>().join(sep));
    }
    format!("\u{feff}{}\r\n", lines.join("\r\n"))
}
