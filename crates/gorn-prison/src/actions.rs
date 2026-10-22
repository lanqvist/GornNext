//! Легальные ходы и их применение. Перенос `engine/actions.ts`.
//!
//! Ход — целое число (код): (вид << 6) | аргумент. Горячий путь (плейаут,
//! самоигра) работает с кодами и переиспользуемым буфером, без аллокаций.

use crate::model::{start_event, Params, CELLS, OFFICE};
use crate::rng::Rng;
use crate::rules::*;
use crate::state::*;

// ── коды ходов ──────────────────────────────────────────────────────────────

pub const A_PATROL: i32 = 0;
pub const A_DEVOUR: i32 = 1;
pub const A_CHAIN: i32 = 2;
pub const A_STOPCHAIN: i32 = 3;
pub const A_SWAP: i32 = 4;
pub const A_END: i32 = 5;
pub const A_HUNT: i32 = 6;
pub const A_BESTOW: i32 = 7;
pub const A_INSTIGATE: i32 = 8;
pub const A_ACCEPT: i32 = 9;
pub const A_SKIP: i32 = 10;
pub const A_TOOL: i32 = 11;
pub const A_GIFT: i32 = 12;
pub const A_REROLL: i32 = 13;

#[inline]
pub const fn code(kind: i32, arg: i32) -> i32 {
    (kind << 6) | arg
}

/// Буфер для legal_codes: ходов не больше 32.
pub type CodeBuf = [i32; 32];

/// Название хода для отладки и сообщений сверки.
pub fn describe(c: i32) -> String {
    let arg = c & 63;
    match c >> 6 {
        A_PATROL => format!("patrol {arg}"),
        A_DEVOUR => format!("devour {arg}"),
        A_CHAIN => format!("chain {arg}"),
        A_STOPCHAIN => "stopchain".into(),
        A_SWAP => "swap".into(),
        A_END => "end".into(),
        A_HUNT => format!("hunt {arg}"),
        A_BESTOW => "bestow".into(),
        A_INSTIGATE => "instigate".into(),
        A_ACCEPT => "accept".into(),
        A_SKIP => "skip".into(),
        A_TOOL => format!("tool {}", crate::model::TOOL_IDS.get(arg as usize).unwrap_or(&"?")),
        A_GIFT => format!("gift {}", crate::model::GIFT_IDS.get(arg as usize).unwrap_or(&"?")),
        A_REROLL => "reroll".into(),
        _ => format!("? {c}"),
    }
}

// ── продвижение до следующего решения игрока ────────────────────────────────

/// Довести состояние до ближайшего решения игрока: выборы орудий и подарков
/// идут первыми, утро и конец дня проходят сами, ряд решений без выбора
/// (только «Завершить день») пропускается.
pub fn settle(s: &mut State, r: &mut Rng, p: &Params) {
    loop {
        if s.m[PICK_GIFTS] > 0 || s.m[PICK_TOOLS] > 0 {
            if s.m[OFFER_KIND] != OFFER_NONE || make_offer(s, r) {
                s.m[PHASE] = P_PICK;
                return;
            }
            continue;
        }
        let nx = s.m[NEXT];
        match nx {
            P_MORNING => {
                r.morning(s.m[DAY]);
                morning(s, r, p);
                continue;
            }
            P_DAYEND => {
                end_day(s);
                continue;
            }
            P_PATROL => {
                // Обход, двери которого ещё не видны (подарок утром пришёл раньше дверей).
                if s.m[DOORS_N] == 0 {
                    r.morning(s.m[DAY]);
                    deal_doors(s, r, p);
                }
            }
            P_MANAGE => {
                // После замены ряд решений — только если новому есть кого съесть.
                let w = s.m[WARDEN];
                let again = s.m[USED] == USED_SWAP && (targets(s, w) | hunt_targets(s)) != 0;
                if (s.m[USED] != 0 && !again) || s.m[TIER + w as usize] < 0 {
                    s.m[NEXT] = P_DAYEND;
                    continue;
                }
            }
            P_CHAIN => {
                let e = s.m[CHAIN_EATER];
                if e < 0 || targets(s, e) == 0 {
                    finish_devour(s);
                    continue;
                }
            }
            P_EVENT => {
                if s.m[EVT + s.m[WARDEN] as usize] == 0 {
                    s.m[NEXT] = P_MANAGE;
                    continue;
                }
            }
            _ => {}
        }
        s.m[PHASE] = nx;
        return;
    }
}

// ── легальные ходы ──────────────────────────────────────────────────────────

/// Записать коды легальных ходов в out, вернуть их число.
pub fn legal_codes(s: &State, out: &mut CodeBuf) -> usize {
    let m = &s.m;
    let mut n = 0;
    let mut push = |c: i32| {
        out[n] = c;
        n += 1;
    };
    match m[PHASE] {
        P_PICK => {
            let kind = if m[OFFER_KIND] == OFFER_GIFT { A_GIFT } else { A_TOOL };
            for i in 0..m[OFFER_N] as usize {
                push(code(kind, m[OFFER + i]));
            }
            if kind == A_TOOL && m[REROLLS] > 0 {
                push(code(A_REROLL, 0));
            }
        }
        P_PATROL => {
            for i in 0..m[DOORS_N] as usize {
                push(code(A_PATROL, m[DOORS + i]));
            }
        }
        // В игре у окна события одна кнопка — «Подтвердить»: пропустить нельзя.
        P_EVENT => push(code(A_ACCEPT, 0)),
        P_OFFICE => {
            push(code(A_BESTOW, 0));
            push(code(A_INSTIGATE, 0));
        }
        P_RATWAY => {
            let w = m[WARDEN];
            for c in 1..CELLS as i32 {
                if c != w {
                    push(code(A_PATROL, c));
                }
            }
        }
        P_MANAGE => {
            let mask = targets(s, m[WARDEN]);
            for c in 1..CELLS as i32 {
                if (mask >> c) & 1 == 1 {
                    push(code(A_DEVOUR, c));
                }
            }
            let prey = hunt_targets(s);
            for c in 1..CELLS as i32 {
                if (prey >> c) & 1 == 1 {
                    push(code(A_HUNT, c));
                }
            }
            if m[USED] != USED_SWAP {
                push(code(A_SWAP, 0));
            }
            push(code(A_END, 0));
        }
        P_CHAIN => {
            let mask = targets(s, m[CHAIN_EATER]);
            for c in 1..CELLS as i32 {
                if (mask >> c) & 1 == 1 {
                    push(code(A_CHAIN, c));
                }
            }
            push(code(A_STOPCHAIN, 0));
        }
        _ => {}
    }
    n
}

/// Ход нельзя сделать в этой фазе.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Illegal {
    pub code: i32,
    pub phase: i32,
}

impl std::fmt::Display for Illegal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "тюрьма: ход {} нельзя в фазе {}", describe(self.code), self.phase)
    }
}

impl std::error::Error for Illegal {}

/// Применить ход по коду с проверкой легальности (фаза и цель).
/// Проверки не трогают ГСЧ: результат тот же, что у [`apply_trusted`].
pub fn apply_code(s: &mut State, c: i32, r: &mut Rng, p: &Params) -> Result<(), Illegal> {
    let arg = c & 63;
    let ph = s.m[PHASE];
    let bad = Illegal { code: c, phase: ph };
    let ok = match c >> 6 {
        A_PATROL => {
            if ph == P_PATROL {
                (0..s.m[DOORS_N] as usize).any(|i| s.m[DOORS + i] == arg)
            } else if ph == P_RATWAY {
                !(arg == OFFICE || arg >= CELLS as i32 || arg == s.m[WARDEN])
            } else {
                false
            }
        }
        A_DEVOUR => ph == P_MANAGE && (targets(s, s.m[WARDEN]) >> arg) & 1 == 1,
        A_CHAIN => ph == P_CHAIN && (targets(s, s.m[CHAIN_EATER]) >> arg) & 1 == 1,
        A_STOPCHAIN | A_END => ph == if c >> 6 == A_STOPCHAIN { P_CHAIN } else { P_MANAGE },
        A_SWAP => ph == P_MANAGE && s.m[USED] != USED_SWAP,
        A_HUNT => ph == P_MANAGE && (hunt_targets(s) >> arg) & 1 == 1,
        A_BESTOW | A_INSTIGATE => ph == P_OFFICE,
        A_ACCEPT | A_SKIP => ph == P_EVENT,
        A_TOOL | A_GIFT => {
            let want = if c >> 6 == A_GIFT { OFFER_GIFT } else { OFFER_TOOL };
            ph == P_PICK && s.m[OFFER_KIND] == want && (0..s.m[OFFER_N] as usize).any(|i| s.m[OFFER + i] == arg)
        }
        A_REROLL => ph == P_PICK && s.m[OFFER_KIND] == OFFER_TOOL && s.m[REROLLS] > 0,
        _ => false,
    };
    if !ok {
        return Err(bad);
    }
    apply_trusted(s, c, r, p);
    Ok(())
}

/// То же без проверок: ход должен быть легальным.
pub fn apply_trusted(s: &mut State, c: i32, r: &mut Rng, p: &Params) {
    let arg = c & 63;
    match c >> 6 {
        A_PATROL => {
            if s.m[PHASE] == P_RATWAY {
                ratway(s, r, p, arg);
            } else {
                patrol(s, r, p, arg);
            }
        }
        A_DEVOUR => devour(s, r, p, arg),
        A_CHAIN => chain_bite(s, r, p, arg),
        A_STOPCHAIN => finish_devour(s),
        A_SWAP => swap(s, r, p),
        A_END => s.m[NEXT] = P_DAYEND,
        A_HUNT => hunt(s, arg, p),
        A_BESTOW | A_INSTIGATE => office(s, r, p, c >> 6 == A_INSTIGATE),
        A_ACCEPT | A_SKIP => event_decision(s, r, p, c >> 6 == A_ACCEPT),
        A_TOOL => {
            s.m[OFFER_KIND] = OFFER_NONE;
            take_tool(s, arg);
        }
        A_GIFT => {
            s.m[OFFER_KIND] = OFFER_NONE;
            take_gift(s, r, p, arg);
        }
        A_REROLL => {
            s.m[REROLLS] -= 1;
            s.m[OFFER_KIND] = OFFER_NONE;
        }
        _ => {}
    }
    settle(s, r, p);
}

// ── начало забега и плейаут ─────────────────────────────────────────────────

/// Как начать забег (`RunOptions` эталона).
#[derive(Clone, Copy, Debug)]
pub struct RunOptions {
    /// Тема (start_event::*).
    pub start: i32,
    /// Цель: `Some(n)` — задана, `Some(-1)` — без цели, `None` — случайная (бросок r.int(5)).
    pub objective: Option<i32>,
    /// Маска орудий, которые уже есть.
    pub tools: i32,
    /// Выбор орудия в начале.
    pub start_pick: bool,
}

impl Default for RunOptions {
    fn default() -> Self {
        RunOptions { start: start_event::NONE, objective: None, tools: 0, start_pick: true }
    }
}

/// Начать забег и довести до первого решения.
pub fn start_run(s: &mut State, r: &mut Rng, p: &Params, opts: RunOptions) {
    let objective = match opts.objective {
        Some(o) => o,
        None => r.int(5),
    };
    new_run(s, r, p, RunSetup { start: opts.start, objective, tools: opts.tools, start_pick: opts.start_pick });
    settle(s, r, p);
}

/// Доиграть забег случайными легальными ходами; вернуть счёт. Без аллокаций.
/// Как в эталоне, ход применяется с проверкой (она не трогает ГСЧ).
pub fn random_playout(s: &mut State, r: &mut Rng, p: &Params, buf: &mut CodeBuf) -> i32 {
    while s.m[PHASE] != P_END {
        let n = legal_codes(s, buf);
        let c = buf[r.int(n as i32) as usize];
        if let Err(e) = apply_code(s, c, r, p) {
            panic!("{e}");
        }
    }
    s.score()
}
