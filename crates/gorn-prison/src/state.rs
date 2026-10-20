//! Состояние забега в одном массиве i32 фиксированного размера: копия —
//! один memcpy, без аллокаций. Раскладка полей та же, что в `engine/state.ts`
//! эталона, — сверка сравнивает массивы целиком.
//!
//! Поля камер — по 16 подряд, обращение `m[TIER + c]`. Камера 0 — допросная.
//! Незапертая камера без узника — TIER = −1. За запертой дверью узник не
//! выбран, пока дверь не откроют, если его не «закрепило» орудие или
//! событие (PRE_TIER ≥ 0): тогда он уже известен движку, а что из этого
//! видит игрок — флаги F_VIS_*.

use crate::model::CELLS;

// ── поля камер (по CELLS штук) ──────────────────────────────────────────────

/// Зверь 0…3, −1 — нет узника.
pub const BEAST: usize = 0;
/// Редкость 0…4, −1 — нет узника.
pub const TIER: usize = BEAST + CELLS;
/// 1 — заперта.
pub const LOCK: usize = TIER + CELLS;
/// Событие в камере: номер события + 1, 0 — нет.
pub const EVT: usize = LOCK + CELLS;
/// Гнездо «Железного ящика»: сколько утр до вылупления, 0 — нет гнезда.
pub const NEST: usize = EVT + CELLS;
/// Флаги камеры: F_*.
pub const FLAGS: usize = NEST + CELLS;
/// Закреплённый узник за запертой дверью: редкость, −1 — не выбран.
pub const PRE_TIER: usize = FLAGS + CELLS;
/// Закреплённый узник: зверь.
pub const PRE_BEAST: usize = PRE_TIER + CELLS;

/// Игрок видит редкость за дверью.
pub const F_VIS_TIER: i32 = 1;
/// Игрок видит зверя за дверью.
pub const F_VIS_BEAST: i32 = 2;
/// Игрок видит «не ниже Редкой».
pub const F_VIS_RARE: i32 = 4;
/// Метка «бивень» (Щипцы).
pub const F_TUSK: i32 = 8;
/// Царапина (Когти): волк ест сквозь дверь.
pub const F_SCRATCH: i32 = 16;

// ── скаляры ─────────────────────────────────────────────────────────────────

const S0: usize = PRE_BEAST + CELLS;
/// Камера надзирателя.
pub const WARDEN: usize = S0;
/// Прошло дней (0 — первый день).
pub const DAY: usize = S0 + 1;
pub const DAY_LIMIT: usize = S0 + 2;
/// Фаза, в которой ждём ход игрока: P_*.
pub const PHASE: usize = S0 + 3;
/// Куда вернуться после выборов (орудие, подарок).
pub const NEXT: usize = S0 + 4;
/// Действие дня в камере надзирателя уже сделано (USED_*).
pub const USED: usize = S0 + 5;
/// Едок в цепочке пожирания, −1 — цепочки нет.
pub const CHAIN_EATER: usize = S0 + 6;
/// Укусов в текущем Пожирании.
pub const CHAIN_BITES: usize = S0 + 7;
/// Орудия: бит tool::*.
pub const TOOLS: usize = S0 + 8;
/// Ждут выбора: орудий (старт и цели) и подарков.
pub const PICK_TOOLS: usize = S0 + 9;
pub const PICK_GIFTS: usize = S0 + 10;
/// Что сейчас предложено: OFFER_NONE / OFFER_TOOL / OFFER_GIFT.
pub const OFFER_KIND: usize = S0 + 11;
pub const OFFER_N: usize = S0 + 12;
/// До 4 предложенных ид (номер орудия или подарка).
pub const OFFER: usize = S0 + 13;
pub const REROLLS: usize = S0 + 17;
/// Призматических в хранилище.
pub const STORAGE: usize = S0 + 18;
/// Текущая цель (objective::*), −1 — нет.
pub const OBJECTIVE_ID: usize = S0 + 19;
pub const OBJ_PROGRESS: usize = S0 + 20;
/// Выполнено целей.
pub const OBJ_DONE: usize = S0 + 21;
pub const START: usize = S0 + 22;
/// Пункты назначения утра: число и камеры.
pub const DOORS_N: usize = S0 + 23;
pub const DOORS: usize = S0 + 24;
/// «Цепная реакция» ждёт следующего обхода.
pub const CHAIN_REACTION: usize = S0 + 27;
/// Сколько ещё выборов в допросной за этот визит.
pub const OFFICE_LEFT: usize = S0 + 28;
// S0 + 29…31 свободны.
/// Счётчики забега.
pub const STAT_OPENED: usize = S0 + 32;
pub const STAT_BITES: usize = S0 + 33;
pub const STAT_STEPS: usize = S0 + 34;
pub const STAT_EVENTS: usize = S0 + 35;
pub const STAT_DEVOURS: usize = S0 + 36;
/// Ступень Узурпации в пуле событий (event::USURP1…USURP4), −1 — Узурпации в пуле нет.
pub const USURP: usize = S0 + 37;
/// 1 — стартовая доска ещё не разыграна: её покажет первое утро.
pub const START_PENDING: usize = S0 + 38;
/// Метки «Добыча» по камере добычи: сколько утр ещё действует (0 — метки нет) и камера льва.
pub const PREY_LEFT: usize = S0 + 39;
pub const PREY_LION: usize = S0 + 39 + CELLS;

pub const SIZE: usize = S0 + 39 + 2 * CELLS;

// ── фазы ────────────────────────────────────────────────────────────────────

/// Выбор орудия или подарка.
pub const P_PICK: i32 = 0;
/// Не используется (номер оставлен, чтобы не сдвигать фазы).
pub const P_HUNT: i32 = 1;
/// Выбор пункта назначения.
pub const P_PATROL: i32 = 2;
/// Окно события.
pub const P_EVENT: i32 = 3;
/// Допросная: Наградить или Подстрекнуть.
pub const P_OFFICE: i32 = 4;
/// «Крысиная тропа»: переход в любую камеру.
pub const P_RATWAY: i32 = 5;
/// «Примите решение»: Пожирание, Замена, Выслеживание, Завершить день.
pub const P_MANAGE: i32 = 6;
/// «Выберите узника для пожирания».
pub const P_CHAIN: i32 = 7;
/// Забег окончен.
pub const P_END: i32 = 8;
/// Служебные (только в NEXT): начать утро, завершить день.
pub const P_MORNING: i32 = 9;
pub const P_DAYEND: i32 = 10;

pub const PHASE_NAMES: [&str; 11] = [
    "pick", "hunt", "patrol", "event", "office", "ratway", "manage", "chain", "end", "morning", "dayend",
];

pub const OFFER_NONE: i32 = 0;
pub const OFFER_TOOL: i32 = 1;
pub const OFFER_GIFT: i32 = 2;

pub const USED_DEVOUR: i32 = 1;
pub const USED_SWAP: i32 = 2;
pub const USED_HUNT: i32 = 3;

// ── состояние ───────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct State {
    pub m: [i32; SIZE],
}

impl Default for State {
    fn default() -> Self {
        State { m: [0; SIZE] }
    }
}

impl std::fmt::Debug for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("State").field("m", &&self.m[..]).finish()
    }
}

impl State {
    pub fn new() -> Self {
        Self::default()
    }

    #[inline]
    pub fn has_tool(&self, tool: u32) -> bool {
        (self.m[TOOLS] as u32 >> tool) & 1 == 1
    }

    #[inline]
    pub fn phase(&self) -> i32 {
        self.m[PHASE]
    }

    #[inline]
    pub fn score(&self) -> i32 {
        self.m[STORAGE]
    }

    /// Хеш состояния для сверки (см. [`StreamHash`]).
    pub fn hash(&self) -> u64 {
        let mut h = StreamHash::new();
        h.push_state(self);
        h.finish()
    }
}

/// Потоковый хеш слов i32 для сверки с эталоном: две 32-битные дорожки
/// (FNV-1a и умножение murmur2 с перемешиванием) — то, что JS считает
/// без BigInt. Тот же расчёт — в tools/reference.mts.
#[derive(Clone, Copy, Debug)]
pub struct StreamHash {
    pub h1: u32,
    pub h2: u32,
}

impl Default for StreamHash {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamHash {
    pub const fn new() -> Self {
        StreamHash { h1: 0x811c_9dc5, h2: 0x9747_b28c }
    }

    #[inline]
    pub fn push(&mut self, v: i32) {
        let v = v as u32;
        self.h1 = (self.h1 ^ v).wrapping_mul(0x0100_0193);
        let h2 = (self.h2 ^ v).wrapping_mul(0x5bd1_e995);
        self.h2 = h2 ^ (h2 >> 15);
    }

    #[inline]
    pub fn push_state(&mut self, s: &State) {
        for &v in &s.m {
            self.push(v);
        }
    }

    pub fn finish(&self) -> u64 {
        ((self.h1 as u64) << 32) | self.h2 as u64
    }
}

/// Копия.
#[inline]
pub fn copy(from: &State, to: &mut State) {
    to.m = from.m;
}

/// Расстояние по часовой стрелке от a до b.
#[inline]
pub fn dist(a: i32, b: i32) -> i32 {
    (b - a + CELLS as i32) & (CELLS as i32 - 1)
}

/// Имя поля состояния по индексу — для сообщений сверки.
pub fn field_name(i: usize) -> String {
    const CELL: [&str; 8] = ["BEAST", "TIER", "LOCK", "EVT", "NEST", "FLAGS", "PRE_TIER", "PRE_BEAST"];
    const SCALAR: [(&str, usize); 30] = [
        ("WARDEN", WARDEN),
        ("DAY", DAY),
        ("DAY_LIMIT", DAY_LIMIT),
        ("PHASE", PHASE),
        ("NEXT", NEXT),
        ("USED", USED),
        ("CHAIN_EATER", CHAIN_EATER),
        ("CHAIN_BITES", CHAIN_BITES),
        ("TOOLS", TOOLS),
        ("PICK_TOOLS", PICK_TOOLS),
        ("PICK_GIFTS", PICK_GIFTS),
        ("OFFER_KIND", OFFER_KIND),
        ("OFFER_N", OFFER_N),
        ("REROLLS", REROLLS),
        ("STORAGE", STORAGE),
        ("OBJECTIVE_ID", OBJECTIVE_ID),
        ("OBJ_PROGRESS", OBJ_PROGRESS),
        ("OBJ_DONE", OBJ_DONE),
        ("START", START),
        ("DOORS_N", DOORS_N),
        ("CHAIN_REACTION", CHAIN_REACTION),
        ("OFFICE_LEFT", OFFICE_LEFT),
        ("STAT_OPENED", STAT_OPENED),
        ("STAT_BITES", STAT_BITES),
        ("STAT_STEPS", STAT_STEPS),
        ("STAT_EVENTS", STAT_EVENTS),
        ("STAT_DEVOURS", STAT_DEVOURS),
        ("USURP", USURP),
        ("START_PENDING", START_PENDING),
        ("PREY_LEFT", PREY_LEFT),
    ];
    if i < WARDEN {
        return format!("{}[{}]", CELL[i / CELLS], i % CELLS);
    }
    if (OFFER..OFFER + 4).contains(&i) {
        return format!("OFFER[{}]", i - OFFER);
    }
    if (DOORS..DOORS + 3).contains(&i) {
        return format!("DOORS[{}]", i - DOORS);
    }
    if (PREY_LEFT..PREY_LION).contains(&i) {
        return format!("PREY_LEFT[{}]", i - PREY_LEFT);
    }
    if (PREY_LION..SIZE).contains(&i) {
        return format!("PREY_LION[{}]", i - PREY_LION);
    }
    SCALAR.iter().find(|(_, k)| *k == i).map(|(n, _)| n.to_string()).unwrap_or_else(|| format!("m[{i}]"))
}
