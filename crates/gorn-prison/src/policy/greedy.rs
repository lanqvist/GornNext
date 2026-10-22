//! Жадная базовая стратегия — точка сравнения для настройки, не для советов:
//! есть, когда можно, есть дальше, пока можно; иначе заменить слабого.
//! Перенос `policy/greedy.ts`.

use crate::actions::{apply_code, code, A_ACCEPT, A_BESTOW, A_CHAIN, A_DEVOUR, A_END, A_GIFT, A_PATROL, A_SWAP, A_TOOL};
use crate::model::{Params, CELLS, OFFICE};
use crate::rng::Rng;
use crate::rules::targets;
use crate::state::*;

fn lowest(mask: i32) -> i32 {
    for c in 1..CELLS as i32 {
        if (mask >> c) & 1 == 1 {
            return c;
        }
    }
    -1
}

pub fn greedy_code(s: &State) -> i32 {
    let m = &s.m;
    match m[PHASE] {
        P_PICK => code(if m[OFFER_KIND] == OFFER_GIFT { A_GIFT } else { A_TOOL }, m[OFFER]),
        P_PATROL => {
            // камера, где узник может есть; иначе запертая; иначе первая
            let mut locked = -1;
            for i in 0..m[DOORS_N] as usize {
                let c = m[DOORS + i];
                if c != OFFICE && m[TIER + c as usize] >= 0 && targets(s, c) != 0 {
                    return code(A_PATROL, c);
                }
                if locked < 0 && m[LOCK + c as usize] == 1 {
                    locked = c;
                }
            }
            code(A_PATROL, if locked >= 0 { locked } else { m[DOORS] })
        }
        P_EVENT => code(A_ACCEPT, 0),
        P_OFFICE => code(A_BESTOW, 0),
        P_RATWAY => {
            for c in 1..CELLS as i32 {
                if c != m[WARDEN] && m[TIER + c as usize] >= 0 && targets(s, c) != 0 {
                    return code(A_PATROL, c);
                }
            }
            for c in 1..CELLS as i32 {
                if c != m[WARDEN] {
                    return code(A_PATROL, c);
                }
            }
            code(A_PATROL, 1)
        }
        P_MANAGE => {
            let e = m[WARDEN];
            let j = lowest(targets(s, e));
            if j >= 0 {
                return code(A_DEVOUR, j);
            }
            code(if m[TIER + e as usize] <= 1 && m[USED] != USED_SWAP { A_SWAP } else { A_END }, 0)
        }
        P_CHAIN => code(A_CHAIN, lowest(targets(s, m[CHAIN_EATER]))),
        _ => panic!("тюрьма: забег окончен, ходов нет"),
    }
}

/// Доиграть забег жадной стратегией; вернуть счёт.
pub fn greedy_playout(s: &mut State, r: &mut Rng, p: &Params) -> i32 {
    while s.m[PHASE] != P_END {
        let c = greedy_code(s);
        if let Err(e) = apply_code(s, c, r, p) {
            panic!("{e}");
        }
    }
    s.m[STORAGE]
}
