//! Веса мгновенной стратегии тюрьмы. Перенос `policy/weights.ts`.
//!
//! В горячем цикле веса — массив f64 с индексами W_*, в файле —
//! именованный JSON (`data/weights.json`, копия weights.json эталона).
//! Все ценности — в долях призматического узника (он стоит 1).
//! JSON читается с точным округлением чисел (serde_json float_roundtrip):
//! веса должны совпасть с эталоном до бита.

use std::sync::OnceLock;

use crate::model::{TOOL_COUNT, TOOL_IDS};

/// Ценность узника по редкости 0…4 в начале забега.
pub const W_VAL0: usize = 0;
/// То же к концу забега (между ними — линейно по оставшимся дням).
pub const W_VAL1: usize = 5;
/// Неиспользованный подарок сокровищницы.
pub const W_GIFT: usize = 10;
/// Запертая дверь: будущий узник за ней.
pub const W_DOOR: usize = 11;
/// Событие «!» на поле.
pub const W_EVENT: usize = 12;
/// Сдвиг ценности Замены.
pub const W_SWAP: usize = 13;
/// «Подстрекнуть»: за каждое поставленное событие.
pub const W_INSTIGATE: usize = 14;
/// Обновить выбор, если лучшая карта стоит меньше.
pub const W_REROLL: usize = 15;
/// Сдвиг решения «есть дальше» в цепочке.
pub const W_CHAIN: usize = 16;
/// Предпочтение дальнего хода, за шаг.
pub const W_FAR: usize = 17;
/// «Клеймо первородного греха»: за узника, которому оно даст еду.
pub const W_BRAND: usize = 18;
/// Обход: ценность камер впереди двери (1–6 шагов за ней, среднее).
pub const W_AHEAD: usize = 19;
/// Обход: потеря от камер, которые ход проскакивает (сумма).
pub const W_SKIP: usize = 20;
/// «Трёхглавое кольцо»: добавка за каждую открытую дверь.
pub const W_RING: usize = 21;
/// Ценность орудия при выборе, по номеру орудия.
pub const W_TOOL: usize = 22;
pub const W_SIZE: usize = W_TOOL + TOOL_COUNT;

pub type Weights = [f64; W_SIZE];

const SCALARS: [(&str, usize); 12] = [
    ("gift", W_GIFT),
    ("door", W_DOOR),
    ("event", W_EVENT),
    ("swap", W_SWAP),
    ("instigate", W_INSTIGATE),
    ("reroll", W_REROLL),
    ("chain", W_CHAIN),
    ("far", W_FAR),
    ("brand", W_BRAND),
    ("ahead", W_AHEAD),
    ("skip", W_SKIP),
    ("ring", W_RING),
];

/// Имя веса по индексу (для отчётов).
pub fn weight_name(i: usize) -> String {
    if i < W_VAL1 {
        return format!("val0[{i}]");
    }
    if i < W_GIFT {
        return format!("val1[{}]", i - W_VAL1);
    }
    if i >= W_TOOL {
        return format!("tools.{}", TOOL_IDS[i - W_TOOL]);
    }
    SCALARS.iter().find(|(_, k)| *k == i).map(|(n, _)| n.to_string()).unwrap_or_else(|| format!("w{i}"))
}

/// Веса из JSON (формат weights.json). Пропущенные поля — как в `fromJson` эталона.
pub fn from_json(text: &str) -> Result<Weights, String> {
    let j: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let num = |v: Option<&serde_json::Value>| v.and_then(|x| x.as_f64());
    let mut w = [0.0; W_SIZE];
    for t in 0..5 {
        w[W_VAL0 + t] = num(j.get("val0").and_then(|a| a.get(t))).unwrap_or(0.0);
        w[W_VAL1 + t] = num(j.get("val1").and_then(|a| a.get(t))).unwrap_or(0.0);
    }
    for (name, k) in SCALARS {
        w[k] = num(j.get(name)).unwrap_or(if name == "brand" { 0.05 } else { 0.0 });
    }
    for (i, id) in TOOL_IDS.iter().enumerate() {
        w[W_TOOL + i] = num(j.get("tools").and_then(|t| t.get(*id))).unwrap_or(0.0);
    }
    Ok(w)
}

/// Веса по умолчанию — настроенные самоигрой (data/weights.json).
pub fn default_weights() -> &'static Weights {
    static W: OnceLock<Weights> = OnceLock::new();
    W.get_or_init(|| from_json(include_str!("../../data/weights.json")).expect("data/weights.json"))
}

/// `Math.round` из JS: половина — вверх.
fn js_round(x: f64) -> f64 {
    let r = x.floor();
    if x - r >= 0.5 {
        r + 1.0
    } else {
        r
    }
}

/// Веса в JSON (формат weights.json, как `toJson` эталона: 4 знака).
pub fn to_json(w: &Weights) -> String {
    // Целые — без «.0», как JSON.stringify.
    let r = |x: f64| {
        let v = js_round(x * 10000.0) / 10000.0;
        if v == v.trunc() && v.abs() < 1e15 {
            serde_json::json!(v as i64)
        } else {
            serde_json::json!(v)
        }
    };
    let mut j = serde_json::Map::new();
    let arr = |a: &[f64]| serde_json::Value::Array(a.iter().map(|&x| r(x)).collect());
    j.insert("val0".into(), arr(&w[W_VAL0..W_VAL0 + 5]));
    j.insert("val1".into(), arr(&w[W_VAL1..W_VAL1 + 5]));
    let mut tools = serde_json::Map::new();
    for (i, id) in TOOL_IDS.iter().enumerate() {
        tools.insert(id.to_string(), r(w[W_TOOL + i]));
    }
    j.insert("tools".into(), serde_json::Value::Object(tools));
    for (name, k) in SCALARS {
        j.insert(name.into(), r(w[k]));
    }
    let mut s = serde_json::to_string_pretty(&serde_json::Value::Object(j)).unwrap();
    s.push('\n');
    s
}

/// Ручные веса: от них стартует настройка и на них — «жадная» сравнительная точка.
pub fn hand_weights() -> Weights {
    let mut w = [0.0; W_SIZE];
    let val0 = [0.02, 0.05, 0.12, 0.27, 0.55];
    let val1 = [0.0, 0.01, 0.03, 0.08, 0.2];
    for t in 0..5 {
        w[W_VAL0 + t] = val0[t];
        w[W_VAL1 + t] = val1[t];
    }
    w[W_GIFT] = 0.3;
    w[W_DOOR] = 0.03;
    w[W_EVENT] = 0.05;
    w[W_SWAP] = 0.0;
    w[W_INSTIGATE] = 0.03;
    w[W_REROLL] = 0.3;
    w[W_CHAIN] = 0.0;
    w[W_FAR] = 0.0;
    w[W_BRAND] = 0.05;
    w[W_AHEAD] = 0.0;
    w[W_SKIP] = 0.0;
    w[W_RING] = 0.0;
    for i in 0..TOOL_COUNT {
        w[W_TOOL + i] = 0.5;
    }
    w
}
