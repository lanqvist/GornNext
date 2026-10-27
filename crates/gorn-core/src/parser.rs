//! Построчный разбор лога в типизированные события. Перенос `core/eventParser.ts`.
//!
//! Хранит состояние: сокет-сообщения многострочные — заголовок с SynId,
//! тело `+ключ [значение]`, маркер End. Запрос и ответ связываются по SynId.

use indexmap::IndexMap;

use crate::patterns::patterns;
use crate::types::*;

/// Метки в UE_game.log записаны в UTC. `Date.UTC(...)` по метке строки.
pub fn parse_timestamp(line: &str) -> Option<i64> {
    let m = patterns().timestamp.captures(line)?;
    let n = |i: usize| m[i].parse::<i64>().unwrap_or(0);
    Some(date_utc(n(1), n(2) - 1, n(3), n(4), n(5), n(6), n(7)))
}

/// `Date.UTC` для обычных значений (месяц с нуля, переполнения нормализуются).
pub fn date_utc(y: i64, mon0: i64, d: i64, h: i64, mi: i64, s: i64, ms: i64) -> i64 {
    let y = y + mon0.div_euclid(12);
    let mo = mon0.rem_euclid(12) + 1;
    let (yy, mm) = if mo <= 2 { (y - 1, mo + 9) } else { (y, mo - 3) };
    let era = yy.div_euclid(400);
    let yoe = yy - era * 400;
    let doy = (153 * mm + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    (((days * 24 + h) * 60 + mi) * 60 + s) * 1000 + ms
}

/// Дешёвый предфильтр: строка без маркера события не несёт.
fn may_carry_event(raw: &str) -> bool {
    raw.contains("BagMgr@")
        || raw.contains("ItemChange@")
        || raw.contains("LevelMgr")
        || raw.contains("PlayerIdTrace@")
        || raw.contains("SetPlayerEquipment")
        || (raw.contains("[Game] S") && matches!(raw.trim_end().rsplit(' ').next(), Some("Run" | "Destory")))
}

fn may_socket_marker(raw: &str) -> bool {
    raw.contains("-Socket")
}

struct PendingSocket {
    direction: String,
    action: String,
    syn_id: i64,
    ts: i64,
    body: Vec<String>,
}

/// Узел дерева тела сокет-дампа: вложенные ключи или значение.
#[derive(Clone, Debug, PartialEq)]
pub enum BodyNode {
    Map(IndexMap<String, BodyNode>),
    Value(String),
}

impl BodyNode {
    pub fn as_map(&self) -> Option<&IndexMap<String, BodyNode>> {
        match self {
            BodyNode::Map(m) => Some(m),
            _ => None,
        }
    }
}

#[derive(Default)]
pub struct EventParser {
    pending: Option<PendingSocket>,
    /// SynId → предмет, цену которого запрашивали, и были ли фильтры по аффиксам.
    price_requests: IndexMap<i64, (i64, bool)>,
    line_no: i64,
}

impl EventParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Событие, если строка его несёт.
    pub fn parse(&mut self, raw: &str) -> Option<LogEvent> {
        self.line_no += 1;
        let line = self.line_no;
        // Тело сокет-сообщения не имеет метки времени — копим до маркера End.
        if self.pending.is_some() {
            if may_socket_marker(raw) && patterns().socket_end.is_match(raw) {
                let done = self.pending.take().unwrap();
                return self.finish_socket(done, line);
            }
            // Маркер End может потеряться: недописанный дамп бросаем, как только
            // строка опознаётся как начало нового дампа или обычное событие.
            if may_socket_marker(raw) && patterns().socket_start.is_match(raw) {
                self.pending = None;
            } else {
                if let Some(ev) = self.parse_event(raw, line) {
                    self.pending = None;
                    return Some(ev);
                }
                self.pending.as_mut().unwrap().body.push(raw.to_string());
                return None;
            }
        }
        if let Some(ev) = self.parse_event(raw, line) {
            return Some(ev);
        }
        self.parse_socket_start(raw);
        None
    }

    fn parse_event(&self, raw: &str, line: i64) -> Option<LogEvent> {
        if !may_carry_event(raw) {
            return None;
        }
        let p = patterns();
        let ts = parse_timestamp(raw);
        if let Some(m) = p.bag_modify.captures(raw) {
            return Some(LogEvent::BagModify {
                ts: ts?,
                line,
                page_id: digits(&m[1]),
                slot_id: digits(&m[2]),
                config_base_id: digits(&m[3]),
                num: digits(&m[4]),
            });
        }
        if let Some(m) = p.bag_init.captures(raw) {
            return Some(LogEvent::BagInit {
                ts: ts?,
                line,
                page_id: digits(&m[1]),
                slot_id: digits(&m[2]),
                config_base_id: digits(&m[3]),
                num: digits(&m[4]),
            });
        }
        if let Some(m) = p.bag_remove.captures(raw) {
            return Some(LogEvent::BagRemove { ts: ts?, line, page_id: digits(&m[1]), slot_id: digits(&m[2]) });
        }
        if let Some(m) = p.item_change.captures(raw) {
            return Some(LogEvent::ItemChange { ts: ts?, line, proto: m[1].to_string(), start: &m[2] == "start" });
        }
        if let Some(m) = p.item_add.captures(raw) {
            return Some(LogEvent::BagAdd {
                ts: ts?,
                line,
                config_base_id: digits(&m[1]),
                num: digits(&m[2]),
                page_id: digits(&m[3]),
                slot_id: digits(&m[4]),
            });
        }
        if let Some(m) = p.item_reset.captures(raw) {
            return Some(LogEvent::BagReset { ts: ts?, line, page_id: digits(&m[1]) });
        }
        if let Some(m) = p.enter_level.captures(raw) {
            return Some(LogEvent::EnterLevel { ts: ts?, line, area_id: digits(&m[1]) });
        }
        if let Some(m) = p.level_path.captures(raw) {
            return Some(LogEvent::LevelPath { ts: ts?, line, asset: m[1].to_string() });
        }
        if let Some(m) = p.level_ids.captures(raw) {
            return Some(LogEvent::LevelIds {
                ts: ts?,
                line,
                level_uid: digits(&m[1]),
                level_type: digits(&m[2]),
                level_id: digits(&m[3]),
            });
        }
        if let Some(m) = p.gear_identity.captures(raw) {
            // GoldId=0 — вещь не легендарка: опознавать нечего.
            let unique_id = digits(&m[2]);
            if unique_id == 0 {
                return None;
            }
            return Some(LogEvent::GearIdentity { ts: ts?, line, base_id: digits(&m[1]), unique_id });
        }
        if let Some(m) = p.season_page.captures(raw) {
            return Some(LogEvent::Page { ts: ts?, line, page: m[1].to_string(), open: &m[2] == "Run" });
        }
        if let Some(m) = p.player_login.captures(raw) {
            // Id остаётся строкой: в double не влезает.
            return Some(LogEvent::PlayerInfo(PlayerInfo {
                ts: ts?,
                line,
                player_id: m[1].to_string(),
                hero_id: None,
                level: None,
                name: None,
                season_id: None,
            }));
        }
        None
    }

    fn parse_socket_start(&mut self, raw: &str) {
        if !may_socket_marker(raw) {
            return;
        }
        if let Some(m) = patterns().socket_start.captures(raw) {
            self.pending = Some(PendingSocket {
                direction: m[1].to_string(),
                action: m[2].trim().to_string(),
                syn_id: digits(&m[3]),
                ts: parse_timestamp(raw).unwrap_or(0),
                body: vec![],
            });
        }
    }

    fn finish_socket(&mut self, msg: PendingSocket, line: i64) -> Option<LogEvent> {
        if msg.action == "GetPlayerData" && msg.direction == "RecvMessage" {
            return finish_player_data(&msg, line);
        }
        if msg.action != "XchgSearchPrice" {
            return None;
        }
        let fields = parse_body(&msg.body);
        if msg.direction == "SendMessage" {
            // `+filters+1+refer [112410]` — какой предмет пробиваем.
            let refer = find_value(&fields, "refer").map(|v| js_number(&v)).unwrap_or(f64::NAN);
            if refer.is_finite() && refer > 0.0 {
                if is_filtered_search(&fields) {
                    // Эталон пробивку с фильтрами не запоминает вовсе: ждущий под
                    // тем же SynId запрос она не вытесняет.
                    self.price_requests.entry(msg.syn_id).or_insert((refer as i64, true));
                } else {
                    self.price_requests.insert(msg.syn_id, (refer as i64, false));
                }
            }
            return None;
        }
        // Ответ: цены активных лотов.
        let (referred, filtered) = self.price_requests.shift_remove(&msg.syn_id)?;
        let groups = fields.get("prices").and_then(BodyNode::as_map)?;
        let mut unit_prices: Vec<f64> = vec![];
        let mut currency = FE_ID;
        for group in groups.values() {
            let Some(group) = group.as_map() else { continue };
            // Отсутствие валюты — валюта по умолчанию, то есть ФЕ.
            let group_currency = match group.get("currency") {
                Some(BodyNode::Value(s)) => js_number(s),
                _ => FE_ID as f64,
            };
            let Some(list) = group.get("unitPrices").and_then(BodyNode::as_map) else { continue };
            let prices: Vec<f64> = list
                .values()
                .filter_map(|v| match v {
                    BodyNode::Value(s) => Some(js_number(s)),
                    _ => None,
                })
                .filter(|n| n.is_finite() && *n > 0.0)
                .collect();
            if prices.is_empty() {
                continue;
            }
            if group_currency == FE_ID as f64 {
                unit_prices = prices;
                currency = FE_ID;
                break;
            }
            // Не ФЕ — запоминаем на случай, если группы в ФЕ вовсе нет.
            if unit_prices.is_empty() {
                unit_prices = prices;
                currency = group_currency as i64;
            }
        }
        if unit_prices.is_empty() {
            return None;
        }
        let obs = PriceObservation {
            kind: "priceObservation",
            ts: msg.ts,
            line,
            config_base_id: referred,
            currency,
            unit_prices,
            syn_id: msg.syn_id,
        };
        Some(if filtered { LogEvent::FilteredPrice(obs) } else { LogEvent::PriceObservation(obs) })
    }
}

/// Профиль персонажа из дампа GetPlayerData: блок `+player`.
fn finish_player_data(msg: &PendingSocket, line: i64) -> Option<LogEvent> {
    let fields = parse_body(&msg.body);
    let player = fields.get("player").and_then(BodyNode::as_map)?;
    let player_id = match player.get("PlayerId") {
        Some(BodyNode::Value(s)) if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) => s.clone(),
        _ => return None,
    };
    let num = |key: &str| -> Option<f64> {
        match player.get(key) {
            Some(BodyNode::Value(s)) => Some(js_number(s)).filter(|n| n.is_finite()),
            _ => None,
        }
    };
    let name = match player.get("Name") {
        Some(BodyNode::Value(s)) if !s.is_empty() => Some(s.clone()),
        _ => None,
    };
    Some(LogEvent::PlayerInfo(PlayerInfo {
        ts: msg.ts,
        line,
        player_id,
        hero_id: num("HeroId"),
        level: num("Level"),
        name,
        season_id: num("SeasonId"),
    }))
}

/// Все листья поддерева: путь ключей и значение.
fn leaves(node: &IndexMap<String, BodyNode>, path: &mut Vec<String>, out: &mut Vec<(Vec<String>, String)>) {
    for (key, child) in node {
        path.push(key.clone());
        match child {
            BodyNode::Map(m) => leaves(m, path, out),
            BodyNode::Value(v) => out.push((path.clone(), v.clone())),
        }
        path.pop();
    }
}

/// Пробивка с фильтрами по аффиксам: цена конкретного ролла, а не базы.
pub fn is_filtered_search(fields: &IndexMap<String, BodyNode>) -> bool {
    let Some(filters) = fields.get("filters").and_then(BodyNode::as_map) else { return false };
    let mut all = vec![];
    leaves(filters, &mut vec![], &mut all);
    let mut base_seen = false;
    for (path, value) in all {
        let key = path.last().map(String::as_str).unwrap_or("");
        if key == "refer" {
            // Первый refer — сам предмет: `+filters+1+refer [71001]`.
            if !base_seen && path.len() == 2 {
                base_seen = true;
                continue;
            }
            if value != "0" && !value.is_empty() {
                return true;
            }
        }
        if (key == "min" || key == "max") && js_number(&value) != 0.0 {
            return true;
        }
    }
    false
}

/// Разбор отступного тела сокет-дампа в дерево: глубина ключа — колонка его `+`.
pub fn parse_body(lines: &[String]) -> IndexMap<String, BodyNode> {
    let p = patterns();
    let mut root: IndexMap<String, BodyNode> = IndexMap::new();
    let mut path: Vec<(usize, String)> = vec![];
    for raw in lines {
        let stripped = p.body_prefix.replace(raw, "");
        // Ключи ищем только до первого значения.
        let head = match stripped.find('[') {
            Some(b) => &stripped[..b],
            None => &stripped[..],
        };
        let tokens: Vec<(usize, String)> = p
            .body_key
            .captures_iter(head)
            .map(|m| {
                let at = m.get(0).unwrap().start();
                // колонка — в кодовых единицах UTF-16, как index в JS
                (head[..at].encode_utf16().count(), m[1].to_string())
            })
            .collect();
        if tokens.is_empty() {
            continue;
        }
        for t in tokens {
            // Ключ заменяет всё, что было в его колонке и правее.
            path.retain(|q| q.0 < t.0);
            path.push(t);
        }
        if let Some(v) = p.body_value.captures(&stripped) {
            let keys: Vec<&str> = path.iter().map(|q| q.1.as_str()).collect();
            set_path(&mut root, &keys, v[1].to_string());
        }
    }
    root
}

fn set_path(root: &mut IndexMap<String, BodyNode>, keys: &[&str], value: String) {
    let Some((last, init)) = keys.split_last() else { return };
    let mut node = root;
    for k in init {
        let entry = node.entry(k.to_string()).or_insert_with(|| BodyNode::Map(IndexMap::new()));
        if !matches!(entry, BodyNode::Map(_)) {
            *entry = BodyNode::Map(IndexMap::new());
        }
        node = match entry {
            BodyNode::Map(m) => m,
            _ => unreachable!(),
        };
    }
    node.insert(last.to_string(), BodyNode::Value(value));
}

/// Первое значение с таким ключом на любой глубине.
pub fn find_value(node: &IndexMap<String, BodyNode>, key: &str) -> Option<String> {
    if let Some(BodyNode::Value(v)) = node.get(key) {
        return Some(v.clone());
    }
    for child in node.values() {
        if let BodyNode::Map(m) = child {
            if let Some(v) = find_value(m, key) {
                return Some(v);
            }
        }
    }
    None
}

#[cfg(test)]
mod season_page_tests {
    use super::*;

    #[test]
    fn season_pages() {
        let mut p = EventParser::new();
        let ev = p.parse("[2026.10.27-17.49.48:975][443]TLLua: Display: [Game] S16BoardMain Run");
        assert!(matches!(ev, Some(LogEvent::Page { ref page, open: true, .. }) if page == "S16BoardMain"));
        let ev = p.parse("[2026.10.27-18.15.22:648][ 12]TLLua: Display: [Game] S13GamePlay Destory\r");
        assert!(matches!(ev, Some(LogEvent::Page { ref page, open: false, .. }) if page == "S13GamePlay"));
        // Не окно: «Hide» и строки о страницах с хвостом.
        assert!(p.parse("[2026.10.27-18.16.57:010][443]TLLua: Display: [Game] S16GamePlayEntry Hide").is_none());
        assert!(p
            .parse("[2026.10.27-17.37.36:870][443]TLLua: Display: [Game] TipMsgShowMgr@DispatchPageRunChange PageName = S16BoardMain , PageRunState = Run ")
            .is_none());
    }
}
