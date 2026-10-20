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
