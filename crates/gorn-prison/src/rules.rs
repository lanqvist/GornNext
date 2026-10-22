//! Правила тюрьмы (rules.md §1–10): двери, рост, пожирание, утро, события,
//! замена, подарки, цели. Функции меняют состояние на месте и ставят NEXT;
//! куда ход переходит дальше, решает `settle()` в actions.rs.
//!
//! Перенос `engine/rules.ts` строка в строку: порядок бросков ГСЧ и
//! короткие замыкания условий (`a && r.chance(p)`) сохранены — от них
//! зависит, сколько чисел съедает ход. Бросок с шансом 0 тоже съедает число,
//! как в эталоне.
//!
//! Наборы камер — 16-битные маски (бит c — камера c).

use crate::model::{
    event, objective, start_event, tool, Params, TierOdds, CELLS, EVENT_POOL, LEGENDARY, LION, MAMMOTH, OFFICE,
    PRISMATIC, RAT, SUPREME, TOOL_COUNT, WOLF,
};
use crate::rng::Rng;
use crate::state::*;
use crate::tables::{EVENT_EXCL, EVENT_SPAWN, EVENT_W, SPAWN_OPENED, SPAWN_UNOPENED, TOOL_EXCL, TOOL_OFFER, TOOL_W};

const C: i32 = CELLS as i32;
const CM: i32 = C - 1;

// ── маски ───────────────────────────────────────────────────────────────────

/// Первая еда: мамонт → лев, лев → волк, волк → крыса, крыса → мамонт.
pub const FIRST_FOOD: [i32; 4] = [LION, WOLF, RAT, MAMMOTH];
/// Вторая еда на любой редкости: мамонт → волк, лев → крыса.
pub const SECOND_FOOD: [i32; 4] = [WOLF, RAT, -1, -1];

const ALL_TOOLS: i32 = 0x7fff_ffff;

#[inline]
fn u(c: i32) -> usize {
    c as usize
}

#[inline]
pub fn popcount(x: i32) -> i32 {
    (x as u32).count_ones() as i32
}

/// Номер k-го установленного бита (k с нуля).
#[inline]
fn nth_bit(mask: i32, mut k: i32) -> i32 {
    let mut m = mask as u32;
    while m != 0 {
        let i = m.trailing_zeros() as i32;
        if k == 0 {
            return i;
        }
        k -= 1;
        m &= m - 1;
    }
    -1
}

/// Случайный установленный бит, −1 — маска пуста.
#[inline]
pub fn pick_bit(mask: i32, r: &mut Rng) -> i32 {
    let n = popcount(mask);
    if n == 0 {
        -1
    } else {
        nth_bit(mask, r.int(n))
    }
}

#[inline]
pub fn locked_mask(s: &State) -> i32 {
    let m = &s.m;
    let mut mask = 0;
    for c in 1..CELLS {
        if m[LOCK + c] == 1 {
            mask |= 1 << c;
        }
    }
    mask
}

#[inline]
pub fn inmate_mask(s: &State) -> i32 {
    let m = &s.m;
    let mut mask = 0;
    for c in 1..CELLS {
        if m[TIER + c] >= 0 {
            mask |= 1 << c;
        }
    }
    mask
}

// ── броски ──────────────────────────────────────────────────────────────────

/// Редкость по долям, не ниже floor.
pub fn roll_tier(r: &mut Rng, odds: &TierOdds, floor: i32) -> i32 {
    let mut sum = 0.0f64;
    for t in floor..=PRISMATIC {
        sum += odds[u(t)];
    }
    if sum <= 0.0 {
        return floor;
    }
    let mut x = r.int(0x4000_0000) as f64 * (sum / 1_073_741_824.0);
    for t in floor..PRISMATIC {
        x -= odds[u(t)];
        if x < 0.0 {
            return t;
        }
    }
    PRISMATIC
}

#[inline]
fn door_floor(s: &State, c: i32) -> i32 {
    let floor = if s.m[FLAGS + u(c)] & F_VIS_RARE != 0 { 2 } else { 0 };
    if s.has_tool(tool::HOOK) && floor < 1 {
        1
    } else {
        floor
    }
}

/// Вес события ev для камеры c; 0 — событие здесь не выпадает.
pub fn event_weight(s: &State, p: &Params, ev: i32, c: i32) -> i32 {
    let m = &s.m;
    let sp = EVENT_SPAWN[u(ev)];
    let locked = m[LOCK + u(c)] == 1;
    if if sp == SPAWN_OPENED { locked } else { sp == SPAWN_UNOPENED && !locked } {
        return 0;
    }
    if m[TOOLS] & EVENT_EXCL[u(ev)] != 0 {
        return 0;
    }
    if (event::USURP1..=event::USURP4).contains(&ev) {
        if m[USURP] != ev {
            return 0;
        }
        return if ev == event::USURP1 { EVENT_W[u(ev)] } else { p.usurp_next_weight };
    }
    EVENT_W[u(ev)]
}

/// Событие для камеры c: взвешенный выбор среди допустимых. −1 — допустимых нет.
pub fn random_event(s: &State, r: &mut Rng, p: &Params, c: i32) -> i32 {
    let uu = s.m[USURP];
    let mut total = 0;
    for &ev in &EVENT_POOL {
        total += event_weight(s, p, ev, c);
    }
    if uu >= 0 {
        total += event_weight(s, p, uu, c);
    }
    if total <= 0 {
        return -1;
    }
    let mut x = r.int(total);
    for &ev in &EVENT_POOL {
        x -= event_weight(s, p, ev, c);
        if x < 0 {
            return ev;
        }
    }
    uu
}

/// Поставить событие в камеру c (если есть допустимое).
fn put_event(s: &mut State, r: &mut Rng, p: &Params, c: i32) {
    let ev = random_event(s, r, p, c);
    if ev >= 0 {
        s.m[EVT + u(c)] = ev + 1;
    }
}

/// Поставить событие в случайную камеру из mask.
fn add_event(s: &mut State, r: &mut Rng, p: &Params, mask: i32) {
    let c = pick_bit(mask, r);
    if c >= 0 {
        put_event(s, r, p, c);
    }
}

fn event_count(s: &State) -> i32 {
    let mut n = 0;
    for c in 1..CELLS {
        if s.m[EVT + c] != 0 {
            n += 1;
        }
    }
    n
}

/// Меток событий на поле не меньше eventMin: добираются сразу в случайную
/// камеру без события, кроме камеры надзирателя.
pub fn top_up_events(s: &mut State, r: &mut Rng, p: &Params) {
    let mut n = event_count(s);
    while n < p.event_min {
        let mask = no_event_mask(s);
        add_event(s, r, p, mask);
        n += 1;
    }
}

fn no_event_mask(s: &State) -> i32 {
    let mut mask = 0;
    let w = s.m[WARDEN];
    for c in 1..C {
        if s.m[EVT + u(c)] == 0 && c != w {
            mask |= 1 << c;
        }
    }
    mask
}

// ── цели ────────────────────────────────────────────────────────────────────

/// Продвинуть цель obj на n; выполненная даёт выбор орудия.
pub fn progress(s: &mut State, r: &mut Rng, p: &Params, obj: i32, n: i32) {
    let m = &mut s.m;
    if m[OBJECTIVE_ID] != obj {
        return;
    }
    m[OBJ_PROGRESS] += n;
    if m[OBJ_PROGRESS] < p.objective_target[u(obj)] {
        return;
    }
    m[PICK_TOOLS] += 1;
    m[OBJ_DONE] += 1;
    m[OBJ_PROGRESS] = 0;
    if !p.objective_repeat {
        m[OBJECTIVE_ID] = -1;
        return;
    }
    let mut next = r.int(5 - 1);
    if next >= obj {
        next += 1;
    }
    set_objective(s, r, p, next);
}

/// Поставить цель; «Загнанный зверь-царь» при уже имеющемся Величайшем выполняется сразу.
pub fn set_objective(s: &mut State, r: &mut Rng, p: &Params, obj: i32) {
    s.m[OBJECTIVE_ID] = obj;
    s.m[OBJ_PROGRESS] = 0;
    if obj == objective::APEX {
        for c in 1..CELLS {
            if s.m[TIER + c] >= SUPREME {
                progress(s, r, p, obj, 1);
                return;
            }
        }
    }
}

// ── узники ──────────────────────────────────────────────────────────────────

/// Забыть метки, привязанные к узнику камеры c (охота).
#[inline]
fn forget(s: &mut State, c: i32) {
    let m = &mut s.m;
    for t in 1..C {
        if m[PREY_LEFT + u(t)] > 0 && (t == c || m[PREY_LION + u(t)] == c) {
            m[PREY_LEFT + u(t)] = 0;
        }
    }
}

/// Камера снова заперта: узник ушёл. Может выпасть новое событие за дверью.
pub fn relock(s: &mut State, r: &mut Rng, p: &Params, c: i32) {
    forget(s, c);
    let m = &mut s.m;
    let cu = u(c);
    m[BEAST + cu] = -1;
    m[TIER + cu] = -1;
    m[LOCK + cu] = 1;
    m[FLAGS + cu] = 0;
    m[NEST + cu] = 0;
    m[PRE_TIER + cu] = -1;
    m[PRE_BEAST + cu] = -1;
    if m[CHAIN_EATER] == c {
        m[CHAIN_EATER] = -1;
    }
    if m[EVT + cu] == 0 && r.chance(p.relock_event) {
        put_event(s, r, p, c);
    }
}

/// Привести редкость узника в порядок после изменения: Величайшая — к цели,
/// Призматическая — в хранилище (камера запирается, сокровищница), при
/// полном хранилище рост стоит на Величайшей.
pub fn settle_tier(s: &mut State, r: &mut Rng, p: &Params, c: i32) {
    let t = s.m[TIER + u(c)];
    if t < SUPREME {
        return;
    }
    progress(s, r, p, objective::APEX, 1);
    if t < PRISMATIC {
        return;
    }
    if s.m[STORAGE] < p.storage {
        s.m[STORAGE] += 1;
        s.m[PICK_GIFTS] += if s.m[START] == start_event::ABUNDANCE { p.abundance_gifts } else { 1 };
        relock(s, r, p, c);
    } else {
        s.m[TIER + u(c)] = SUPREME;
    }
}

/// +n к редкости узника камеры c (если он там есть).
pub fn raise(s: &mut State, r: &mut Rng, p: &Params, c: i32, n: i32) {
    if s.m[TIER + u(c)] < 0 || n <= 0 {
        return;
    }
    s.m[TIER + u(c)] += n;
    settle_tier(s, r, p, c);
}

/// Посадить узника в камеру (она становится открытой).
pub fn place(s: &mut State, r: &mut Rng, p: &Params, c: i32, beast: i32, tier: i32) {
    let m = &mut s.m;
    let cu = u(c);
    m[LOCK + cu] = 0;
    m[NEST + cu] = 0;
    m[PRE_TIER + cu] = -1;
    m[PRE_BEAST + cu] = -1;
    m[FLAGS + cu] = 0;
    m[BEAST + cu] = beast;
    m[TIER + cu] = tier;
    settle_tier(s, r, p, c);
}

/// Закрепить узника за запертой дверью (без показа игроку).
pub fn preset(s: &mut State, r: &mut Rng, p: &Params, c: i32) {
    let cu = u(c);
    if s.m[LOCK + cu] != 1 || s.m[PRE_TIER + cu] >= 0 {
        return;
    }
    let floor = door_floor(s, c);
    s.m[PRE_TIER + cu] = roll_tier(r, &p.door_odds, floor);
    s.m[PRE_BEAST + cu] = r.int(4);
}

/// Открыть запертую камеру: узник — закреплённый или по шансам двери.
pub fn open_cell(s: &mut State, r: &mut Rng, p: &Params, c: i32) {
    let cu = u(c);
    if s.m[LOCK + cu] != 1 {
        return;
    }
    let mut tier = s.m[PRE_TIER + cu];
    let mut beast = s.m[PRE_BEAST + cu];
    if tier < 0 {
        let floor = door_floor(s, c);
        tier = roll_tier(r, &p.door_odds, floor);
        beast = r.int(4);
    }
    s.m[STAT_OPENED] += 1;
    place(s, r, p, c, beast, tier);
    progress(s, r, p, objective::DOORS, 1);
}

/// Открыть случайную запертую камеру; −1 — запертых нет.
pub fn open_random(s: &mut State, r: &mut Rng, p: &Params) -> i32 {
    let c = pick_bit(locked_mask(s), r);
    if c >= 0 {
        open_cell(s, r, p, c);
    }
    c
}

/// Убрать узника: камера запирается.
pub fn remove(s: &mut State, r: &mut Rng, p: &Params, c: i32) {
    if s.m[TIER + u(c)] >= 0 {
        relock(s, r, p, c);
    }
}

/// +gain случайным n разным узникам.
fn raise_random(s: &mut State, r: &mut Rng, p: &Params, n: i32, gain: i32) {
    let mut mask = inmate_mask(s);
    let mut i = 0;
    while i < n && mask != 0 {
        let c = pick_bit(mask, r);
        mask &= !(1 << c);
        raise(s, r, p, c, gain);
        i += 1;
    }
}

// ── кто кого ест ────────────────────────────────────────────────────────────

#[inline]
fn is_food(eater: i32, food: i32) -> bool {
    food == FIRST_FOOD[u(eater)] || food == SECOND_FOOD[u(eater)]
}

/// Может ли узник камеры e съесть камеру t.
pub fn can_eat(s: &State, e: i32, t: i32) -> bool {
    (targets(s, e) >> t) & 1 == 1
}

/// Маска целей едока e. Самая горячая функция плейаута.
#[inline]
pub fn targets(s: &State, e: i32) -> i32 {
    let m = &s.m;
    let te = m[TIER + u(e)];
    if !(0..PRISMATIC).contains(&te) {
        return 0;
    }
    let be = m[BEAST + u(e)];
    let tools = m[TOOLS];
    let f1 = FIRST_FOOD[u(be)];
    let f2 = SECOND_FOOD[u(be)];
    let any_tier = m[START] == start_event::CAGEFIGHT;
    let has = |t: u32| (tools >> t) & 1 == 1;
    let skewer = be == RAT && has(tool::SKEWER);
    let crown = be == LION && has(tool::CROWN);
    let maul = be == MAMMOTH && has(tool::MAUL);
    let guillotine = be == WOLF && has(tool::GUILLOTINE);
    // Грязные когти: волк той же редкости ест сквозь поцарапанную дверь.
    let claws = be == WOLF && has(tool::CLAWS);
    let mut mask = 0;
    for t in 1..C {
        if t == e {
            continue;
        }
        let tu = u(t);
        if m[LOCK + tu] == 1 {
            if claws && m[FLAGS + tu] & F_SCRATCH != 0 && m[PRE_TIER + tu] == te {
                mask |= 1 << t;
            }
            continue;
        }
        let tt = m[TIER + tu];
        if tt < 0 {
            continue;
        }
        let bt = m[BEAST + tu];
        let food = bt == f1 || bt == f2;
        if (food && (tt == te || (any_tier && tt < te)))
            || (crown && tt == te)
            || (skewer && bt == RAT && tt == te)
            || (maul && food && tt > te)
            || (guillotine && bt == RAT && tt < te)
        {
            mask |= 1 << t;
        }
    }
    mask
}

/// Укус (одно Пожирание): e съедает t.
pub fn bite(s: &mut State, r: &mut Rng, p: &Params, e: i32, t: i32, in_chain: bool) {
    let eu = u(e);
    let tu = u(t);
    let te = s.m[TIER + eu];
    let be = s.m[BEAST + eu];
    let door = s.m[LOCK + tu] == 1;
    let tt = if door { s.m[PRE_TIER + tu] } else { s.m[TIER + tu] };
    let bt = if door { s.m[PRE_BEAST + tu] } else { s.m[BEAST + tu] };
    // Намордник (101): «на поле не более 4 узников» — до укуса, считая едока и жертву.
    let field_before = popcount(inmate_mask(s));

    let mut gain = 1;
    if door {
        gain = p.claws_gain;
    } else if be == MAMMOTH && s.has_tool(tool::MAUL) && is_food(be, bt) {
        gain = (tt + 2 - te).max(1);
    } else if tt < te
        && be == WOLF
        && bt == RAT
        && s.has_tool(tool::GUILLOTINE)
        && !r.chance(1.0 - p.oppress_step * (te - tt) as f64)
    {
        // Клиент 146: после поедания ниже по редкости Гильотиной — «Подавление».
        gain = 0;
    }
    // Шампур (148): +2 за поедание крысы крысой.
    if be == RAT && bt == RAT && s.has_tool(tool::SKEWER) {
        gain += p.skewer_bonus;
    }
    if be == LION && s.has_tool(tool::CROWN) {
        gain += p.crown_bonus;
    }
    if s.has_tool(tool::WHIP) && r.chance(p.whip_chance) {
        gain += 1;
    }
    if in_chain && s.has_tool(tool::COLLAR) && r.chance(p.collar_chance) {
        gain += 1;
    }

    if !door && s.has_tool(tool::CAGE) && r.chance(p.cage_chance) {
        forget(s, t);
        s.m[LOCK + tu] = 1;
        s.m[PRE_TIER + tu] = -1;
        s.m[FLAGS + tu] = 0;
        s.m[TIER + tu] = -1;
        s.m[BEAST + tu] = -1;
        open_cell(s, r, p, t);
    } else {
        relock(s, r, p, t);
    }

    if s.has_tool(tool::MUZZLE) && field_before <= p.muzzle_left {
        gain += p.muzzle_bonus;
    }

    s.m[STAT_BITES] += 1;
    if p.jungle_counts_bites {
        progress(s, r, p, objective::JUNGLE, 1);
    }

    raise(s, r, p, e, gain);
    if s.m[TIER + eu] < 0 {
        return;
    }
    if s.m[START] != start_event::CAGEFIGHT {
        let nb = r.int(4);
        if nb != be {
            forget(s, e);
        }
        s.m[BEAST + eu] = nb;
    }
    after_devour(s, r, p, e);
}

/// «Ржавая пила» и «Железная дева мстителя»: после каждого Пожирания.
fn after_devour(s: &mut State, r: &mut Rng, p: &Params, e: i32) {
    if s.has_tool(tool::SAW) && r.chance(p.saw_chance) {
        raise(s, r, p, e, 1);
    }
    if s.m[TIER + u(e)] >= 0 && s.has_tool(tool::REVENGE) && r.chance(p.revenge_chance) {
        open_food(s, r, p, e);
    }
}

/// Открыть случайную запертую камеру с узником, которого e гарантированно может съесть.
fn open_food(s: &mut State, r: &mut Rng, p: &Params, e: i32) {
    let c = pick_bit(locked_mask(s), r);
    if c < 0 {
        return;
    }
    let be = s.m[BEAST + u(e)];
    let second = SECOND_FOOD[u(be)];
    s.m[PRE_TIER + u(c)] = s.m[TIER + u(e)];
    s.m[PRE_BEAST + u(c)] = if second >= 0 && r.int(2) == 1 { second } else { FIRST_FOOD[u(be)] };
    open_cell(s, r, p, c);
}

/// Копия узника камеры c в случайной запертой камере, она открывается.
fn copy_inmate(s: &mut State, r: &mut Rng, p: &Params, c: i32) {
    let d = pick_bit(locked_mask(s), r);
    if d < 0 {
        return;
    }
    s.m[PRE_TIER + u(d)] = s.m[TIER + u(c)];
    s.m[PRE_BEAST + u(d)] = s.m[BEAST + u(c)];
    open_cell(s, r, p, d);
}

/// Пожирание из ряда решений: первый укус, дальше цепочка.
pub fn devour(s: &mut State, r: &mut Rng, p: &Params, t: i32) {
    let e = s.m[WARDEN];
    s.m[USED] = USED_DEVOUR;
    s.m[STAT_DEVOURS] += 1;
    if !p.jungle_counts_bites {
        progress(s, r, p, objective::JUNGLE, 1);
    }
    bite(s, r, p, e, t, false);
    after_bite(s, e);
}

/// Укус в цепочке.
pub fn chain_bite(s: &mut State, r: &mut Rng, p: &Params, t: i32) {
    let e = s.m[CHAIN_EATER];
    bite(s, r, p, e, t, true);
    after_bite(s, e);
}

fn after_bite(s: &mut State, e: i32) {
    if s.m[TIER + u(e)] >= 0 && targets(s, e) != 0 {
        s.m[CHAIN_EATER] = e;
        s.m[NEXT] = P_CHAIN;
    } else {
        finish_devour(s);
    }
}

/// Конец Пожирания (цепочка кончилась или «Отменить»): день завершается.
pub fn finish_devour(s: &mut State) {
    s.m[CHAIN_EATER] = -1;
    s.m[NEXT] = P_DAYEND;
}

// ── утро ────────────────────────────────────────────────────────────────────

/// Утро: утренние орудия, затем пункты назначения.
pub fn morning(s: &mut State, r: &mut Rng, p: &Params) {
    if s.m[START_PENDING] == 1 {
        s.m[START_PENDING] = 0;
        start_board(s, r, p);
    }

    // Гнёзда «Железного ящика»: через ironboxDays утр — Редкая крыса.
    for c in 1..C {
        let cu = u(c);
        if s.m[NEST + cu] > 0 {
            s.m[NEST + cu] -= 1;
            if s.m[NEST + cu] == 0 && s.m[LOCK + cu] == 1 {
                s.m[PRE_TIER + cu] = p.ironbox_tier;
                s.m[PRE_BEAST + cu] = RAT;
                open_cell(s, r, p, c);
            }
        }
    }

    if s.has_tool(tool::RAVENOUS) {
        ravenous(s, r, p);
    }
    if s.has_tool(tool::COIN) && r.chance(p.coin_chance) {
        coin(s, r, p);
    }

    if s.has_tool(tool::TONGS) {
        let mut mammoths = 0;
        for c in 1..CELLS {
            if s.m[TIER + c] >= 0 && s.m[BEAST + c] == MAMMOTH && s.m[FLAGS + c] & F_TUSK == 0 {
                mammoths |= 1 << c;
            }
        }
        let c = pick_bit(mammoths, r);
        if c >= 0 {
            s.m[FLAGS + u(c)] |= F_TUSK;
        }
    }

    if s.has_tool(tool::CLAWS) {
        let mut free = 0;
        for c in 1..CELLS {
            if s.m[LOCK + c] == 1 && s.m[FLAGS + c] & F_SCRATCH == 0 {
                free |= 1 << c;
            }
        }
        let c = pick_bit(free, r);
        if c >= 0 {
            preset(s, r, p, c);
            s.m[FLAGS + u(c)] |= F_SCRATCH | F_VIS_TIER;
        }
    }

    if s.has_tool(tool::IRONBOX) {
        let mut free = 0;
        for c in 1..CELLS {
            if s.m[LOCK + c] == 1 && s.m[NEST + c] == 0 {
                free |= 1 << c;
            }
        }
        let c = pick_bit(free, r);
        if c >= 0 {
            s.m[NEST + u(c)] = p.ironbox_days;
        }
    }

    // Метка «Добыча»: 3 хода, в начале каждого дня лев и добыча +1.
    for prey in 1..C {
        let pu = u(prey);
        if s.m[PREY_LEFT + pu] <= 0 {
            continue;
        }
        let lion = s.m[PREY_LION + pu];
        s.m[PREY_LEFT + pu] -= 1;
        raise(s, r, p, prey, 1);
        if lion > 0 {
            raise(s, r, p, lion, 1);
        }
    }

    deal_doors(s, r, p);
    s.m[NEXT] = P_PATROL;
}

/// Голодная удушающая цепь: самый редкий узник убирает соседей, +1 за каждого с шансом.
fn ravenous(s: &mut State, r: &mut Rng, p: &Params) {
    let mut best = -1;
    let mut mask = 0;
    for c in 1..CELLS {
        let t = s.m[TIER + c];
        if t > best {
            best = t;
            mask = 1 << c;
        } else if t == best && t >= 0 {
            mask |= 1 << c;
        }
    }
    let e = pick_bit(mask, r);
    if e < 0 {
        return;
    }
    let mut gain = 0;
    for d in [-1, 1] {
        let c = (e + d + C) & CM;
        if c == OFFICE || s.m[TIER + u(c)] < 0 {
            continue;
        }
        remove(s, r, p, c);
        if r.chance(p.ravenous_chance) {
            gain += 1;
        }
    }
    raise(s, r, p, e, gain);
}

/// Монета участи братьев: случайно убирает 1 идентичного узника, затем
/// случайный из оставшихся идентичных ему +2.
fn coin(s: &mut State, r: &mut Rng, p: &Params) {
    let m = &s.m;
    let mut twins = 0;
    for a in 1..CELLS {
        if m[TIER + a] < 0 {
            continue;
        }
        for b in 1..CELLS {
            if b != a && m[TIER + b] == m[TIER + a] && m[BEAST + b] == m[BEAST + a] {
                twins |= 1 << a;
            }
        }
    }
    let gone = pick_bit(twins, r);
    if gone < 0 {
        return;
    }
    let gu = u(gone);
    let mut rest = 0;
    for b in 1..CELLS {
        if b != gu && s.m[TIER + b] == s.m[TIER + gu] && s.m[BEAST + b] == s.m[BEAST + gu] {
            rest |= 1 << b;
        }
    }
    remove(s, r, p, gone);
    let c = pick_bit(rest, r);
    if c >= 0 {
        raise(s, r, p, c, p.coin_bonus);
    }
}

/// Кого лев в камере надзирателя может пометить «Выслеживанием».
pub fn hunt_targets(s: &State) -> i32 {
    let m = &s.m;
    let w = u(m[WARDEN]);
    if !s.has_tool(tool::TRAP) || m[BEAST + w] != LION || m[TIER + w] < 0 {
        return 0;
    }
    let mut marked = 0;
    for c in 1..CELLS {
        if m[PREY_LEFT + c] > 0 {
            marked |= 1 << c;
        }
    }
    targets(s, w as i32) & !locked_mask(s) & !marked
}

/// «Выслеживание»: лев надзирателя метит добычу; это действие дня.
pub fn hunt(s: &mut State, prey: i32, p: &Params) {
    let m = &mut s.m;
    m[PREY_LION + u(prey)] = m[WARDEN];
    m[PREY_LEFT + u(prey)] = p.trap_mornings;
    m[USED] = USED_HUNT;
    m[NEXT] = P_DAYEND;
}

/// Пункты назначения: 3 минус орудия; не меньше одной.
pub fn deal_doors(s: &mut State, r: &mut Rng, p: &Params) {
    let mut n = p.door_cards;
    let minus = s.has_tool(tool::FORGED) as i32 + s.has_tool(tool::WHIP) as i32 + s.has_tool(tool::STAKE) as i32;
    if minus > 0 {
        n -= minus;
    } else if r.chance(p.one_less_door) {
        n -= 1;
    }
    n = n.clamp(1, 3);
    let mut steps = ((1 << p.door_reach) - 1) << 1;
    let mut chosen = 0;
    let mut i = 0;
    while i < n && steps != 0 {
        let k = pick_bit(steps, r);
        steps &= !(1 << k);
        chosen |= 1 << k;
        i += 1;
    }
    let w = s.m[WARDEN];
    let mut i = 0;
    for k in 1..=p.door_reach {
        if (chosen >> k) & 1 == 1 {
            s.m[DOORS + i] = (w + k) & CM;
            i += 1;
        }
    }
    s.m[DOORS_N] = i as i32;
}

/// Конец дня.
pub fn end_day(s: &mut State) {
    let m = &mut s.m;
    m[DAY] += 1;
    m[USED] = 0;
    m[CHAIN_EATER] = -1;
    m[DOORS_N] = 0;
    m[NEXT] = if m[DAY] >= m[DAY_LIMIT] { P_END } else { P_MORNING };
}

// ── обход ───────────────────────────────────────────────────────────────────

/// Обход: надзиратель идёт по часовой стрелке к камере cell.
pub fn patrol(s: &mut State, r: &mut Rng, p: &Params, cell: i32) {
    let from = s.m[WARDEN];
    let steps = dist(from, cell);
    if s.m[CHAIN_REACTION] == 1 {
        s.m[CHAIN_REACTION] = 0;
        for k in 1..steps {
            open_cell(s, r, p, (from + k) & CM);
        }
    }
    s.m[WARDEN] = cell;
    s.m[STAT_STEPS] += steps;
    progress(s, r, p, objective::WALK, steps);

    if cell != OFFICE && s.m[LOCK + u(cell)] == 1 {
        let locked_before = popcount(locked_mask(s));
        open_cell(s, r, p, cell);
        door_tools(s, r, p, cell, locked_before);
    }
    if s.has_tool(tool::SHORTCHAIN) && steps <= p.shortchain_steps && r.chance(p.shortchain_chance) {
        open_random(s, r, p);
    }
    if s.has_tool(tool::SHACKLES) && steps >= p.shackles_steps && r.chance(p.shackles_chance) {
        raise_random(s, r, p, 1, 1);
    }
    arrive(s, r, p, cell);
}

/// «Крысиная тропа»: переход без шагов; дверь открывается, орудия обхода не срабатывают.
pub fn ratway(s: &mut State, r: &mut Rng, p: &Params, cell: i32) {
    s.m[WARDEN] = cell;
    open_cell(s, r, p, cell);
    arrive(s, r, p, cell);
}

fn arrive(s: &mut State, r: &mut Rng, p: &Params, cell: i32) {
    if cell == OFFICE {
        tyranny_arrive(s, r, p);
        s.m[OFFICE_LEFT] = office_rounds(s, p);
        s.m[NEXT] = P_OFFICE;
    } else if s.m[EVT + u(cell)] != 0 {
        s.m[NEXT] = P_EVENT;
    } else {
        s.m[NEXT] = P_MANAGE;
    }
}

/// Орудия при открытии двери обходом; locked_before — запертых до открытия этой двери.
fn door_tools(s: &mut State, r: &mut Rng, p: &Params, c: i32, locked_before: i32) {
    if s.has_tool(tool::DOGTAG) && r.chance(p.dogtag_chance) {
        raise(s, r, p, c, 1);
    }
    if s.has_tool(tool::KEYS) && r.chance(p.keys_chance) {
        open_cell(s, r, p, (c + C - 1) & CM);
        open_cell(s, r, p, (c + 1) & CM);
    }
    if s.has_tool(tool::FORGED) && r.chance(p.forged_chance) {
        open_random(s, r, p);
    }
    if s.has_tool(tool::FIRE) && s.m[TIER + u(c)] >= 0 && r.chance(p.fire_chance) {
        copy_inmate(s, r, p, c);
    }
    // «Неоткрытых не больше 5» — до открытия этой двери [факты 21.10: при 6
    // запертых до обхода — ни разу, 9 из 9; при ≤ 5 — срабатывает].
    let locked = if p.blacklist_before_open { locked_before } else { popcount(locked_mask(s)) };
    if s.has_tool(tool::BLACKLIST) && locked <= p.blacklist_locked {
        raise_random(s, r, p, p.blacklist_count, 1);
    }
}

// ── допросная ───────────────────────────────────────────────────────────────

/// Выборов в допросной за визит: один.
#[inline]
pub fn office_rounds(_s: &State, _p: &Params) -> i32 {
    1
}

/// «Подстрекнуть»: событие в случайную камеру без события.
fn instigate(s: &mut State, r: &mut Rng, p: &Params) {
    let mut mask = no_event_mask(s);
    let mut i = 0;
    while i < p.instigate_doors && mask != 0 {
        let c = pick_bit(mask, r);
        mask &= !(1 << c);
        put_event(s, r, p, c);
        i += 1;
    }
}

/// «Злоупотребление властью»: при каждой остановке в допросной ещё
/// tyrannyExtra раз автоматически Подстрекнуть и Наградить.
pub fn tyranny_arrive(s: &mut State, r: &mut Rng, p: &Params) {
    if s.m[START] != start_event::TYRANNY {
        return;
    }
    for _ in 0..p.tyranny_extra {
        instigate(s, r, p);
        raise_random(s, r, p, 1, 1);
    }
}

pub fn office(s: &mut State, r: &mut Rng, p: &Params, choose: bool) {
    if choose {
        instigate(s, r, p);
    } else {
        raise_random(s, r, p, 1, 1);
    }
    s.m[OFFICE_LEFT] -= 1;
    s.m[NEXT] = if s.m[OFFICE_LEFT] > 0 { P_OFFICE } else { P_MANAGE };
}

// ── события ─────────────────────────────────────────────────────────────────

/// Решение по событию в камере надзирателя.
pub fn event_decision(s: &mut State, r: &mut Rng, p: &Params, accept: bool) {
    let c = s.m[WARDEN];
    let ev = s.m[EVT + u(c)] - 1;
    s.m[EVT + u(c)] = 0;
    s.m[NEXT] = P_MANAGE;
    if accept && ev >= 0 {
        s.m[STAT_EVENTS] += 1;
        run_event(s, r, p, c, ev);
        progress(s, r, p, objective::CURRENTS, 1);
    }
    top_up_events(s, r, p);
}

/// Действие события ev в камере c.
pub fn run_event(s: &mut State, r: &mut Rng, p: &Params, c: i32, ev: i32) {
    let cu = u(c);
    match ev {
        event::CHAIN => s.m[CHAIN_REACTION] = 1,
        event::EXECUTION => {
            // Клиент 222: все низшие узники, над которыми у текущего преимущество; +1 за каждого.
            let te = s.m[TIER + cu];
            if te < 0 {
                return;
            }
            let be = s.m[BEAST + cu];
            let mut n = 0;
            for t in 1..C {
                let tu = u(t);
                if t != c && s.m[TIER + tu] >= 0 && s.m[TIER + tu] < te && is_food(be, s.m[BEAST + tu]) {
                    remove(s, r, p, t);
                    n += 1;
                }
            }
            raise(s, r, p, c, n);
        }
        event::BEHIND => {
            if s.m[TIER + cu] < 0 {
                return;
            }
            // «Дефицитное кодовое имя»: меньше всех на поле, считая 0 и самого узника.
            let mut cnt = [0i32; 4];
            for t in 1..CELLS {
                if s.m[TIER + t] < 0 {
                    continue;
                }
                let b = s.m[BEAST + t];
                cnt[if (0..3).contains(&b) { u(b) } else { 3 }] += 1;
            }
            let min = cnt[0].min(cnt[1]).min(cnt[2]).min(cnt[3]);
            let mut mask = 0;
            for (i, &k) in cnt.iter().enumerate() {
                if k == min {
                    mask |= 1 << i;
                }
            }
            let nb = pick_bit(mask, r);
            if nb != s.m[BEAST + cu] {
                forget(s, c);
            }
            s.m[BEAST + cu] = nb;
            if s.m[TIER + cu] < SUPREME {
                s.m[TIER + cu] = SUPREME;
            }
            settle_tier(s, r, p, c);
        }
        event::APPETITE => {
            let mut low = 99;
            let mut mask = 0;
            for t in 1..CELLS {
                let tt = s.m[TIER + t];
                if tt < 0 {
                    continue;
                }
                if tt < low {
                    low = tt;
                    mask = 1 << t;
                } else if tt == low {
                    mask |= 1 << t;
                }
            }
            let t = pick_bit(mask, r);
            if t < 0 {
                return;
            }
            let mut gain = 0;
            for _ in 0..p.appetite_tries {
                if r.chance(p.appetite_chance) {
                    gain += 1;
                }
            }
            raise(s, r, p, t, gain);
        }
        event::CONVICT => {
            // Клиент 201: камера с узником не ниже Легендарной.
            let t = pick_bit(locked_mask(s), r);
            if t < 0 {
                return;
            }
            s.m[PRE_TIER + u(t)] = roll_tier(r, &p.door_odds, LEGENDARY);
            s.m[PRE_BEAST + u(t)] = r.int(4);
            open_cell(s, r, p, t);
        }
        event::RAVEN => {
            let w = s.m[WARDEN];
            let mut n = 0;
            let mut k = 1;
            while k < C && n < p.raven_cells {
                let t = (w + k) & CM;
                k += 1;
                if s.m[LOCK + u(t)] != 1 {
                    continue;
                }
                preset(s, r, p, t);
                s.m[FLAGS + u(t)] |= F_VIS_TIER;
                n += 1;
            }
        }
        event::FREEDOM => {
            for _ in 0..p.freedom_opens {
                open_random(s, r, p);
            }
        }
        event::USURP1 | event::USURP2 | event::USURP3 | event::USURP4 => {
            // Клиент 252–255: ступень заменяет себя в пуле следующей, IV уходит из пула.
            let level = u(ev - event::USURP1);
            let n = p.usurp_count[level];
            raise_random(s, r, p, if n < 0 { C } else { n }, p.usurp_gain[level]);
            if s.m[USURP] == ev {
                s.m[USURP] = if ev == event::USURP4 { -1 } else { ev + 1 };
            }
        }
        event::RATWAY => s.m[NEXT] = P_RATWAY,
        event::TREASURE => {
            // Карта разведывает камеры ниже Редкой; про остальные игрок выводит «не ниже Редкой».
            for t in 1..C {
                let tu = u(t);
                if s.m[LOCK + tu] != 1 {
                    continue;
                }
                preset(s, r, p, t);
                s.m[FLAGS + tu] |= if s.m[PRE_TIER + tu] <= 1 { F_VIS_TIER } else { F_VIS_RARE };
            }
        }
        event::RIVAL => {
            if s.m[TIER + cu] >= 0 {
                open_food(s, r, p, c);
            }
        }
        event::LUCK => {
            if s.m[TIER + cu] < 0 {
                return;
            }
            let mut n = 0;
            for t in 1..CELLS {
                if t != cu && s.m[TIER + t] >= 0 && s.m[BEAST + t] == s.m[BEAST + cu] {
                    n += 1;
                }
            }
            raise(s, r, p, c, n);
        }
        event::ESCORT => {
            if s.m[TIER + cu] >= 0 {
                copy_inmate(s, r, p, c);
            }
        }
        event::EXTRA => raise_random(s, r, p, p.extra_count, 1),
        _ => {}
    }
}

// ── замена ──────────────────────────────────────────────────────────────────

/// Замена узника в камере надзирателя. После неё ряд решений открывается
/// снова, уже без «Заменить»: новый узник может съесть кого-то в тот же день
/// [кадры 21.10: 56 из 126 замен].
pub fn swap(s: &mut State, r: &mut Rng, p: &Params) {
    let c = s.m[WARDEN];
    let cu = u(c);
    s.m[USED] = USED_SWAP;
    s.m[NEXT] = if p.swap_then_devour { P_MANAGE } else { P_DAYEND };
    let old_tier = s.m[TIER + cu];
    let old_beast = s.m[BEAST + cu];
    if old_tier < 0 {
        return;
    }
    let tusk = s.m[FLAGS + cu] & F_TUSK != 0;
    forget(s, c);

    // Петля: Обычные и Магические больше не появляются. Крюк замену не трогает.
    let mut floor = if s.has_tool(tool::ROPE) { 2 } else { 0 };
    let others = inmate_mask(s) & !(1 << c);
    if s.has_tool(tool::SCAPEGOAT) && popcount(others) >= p.scapegoat_removes {
        let mut mask = others;
        for _ in 0..p.scapegoat_removes {
            let t = pick_bit(mask, r);
            mask &= !(1 << t);
            remove(s, r, p, t);
        }
        // Клиент 121: «редкость после замены гарантированно повышается».
        floor = floor.max((old_tier + 1).min(PRISMATIC));
    }
    let mut tier = roll_tier(r, &p.replace_odds, floor);
    let mut beast = r.int(4);
    if s.has_tool(tool::STAKE) && old_tier < LEGENDARY && tier < LEGENDARY && r.chance(p.stake_chance) {
        tier = LEGENDARY;
    }
    if s.has_tool(tool::PESTLE) {
        beast = old_beast;
    }

    // «Бивень» — метка камеры: сходит, только когда в камере нет узника.
    s.m[FLAGS + cu] = if tusk { F_TUSK } else { 0 };
    s.m[BEAST + cu] = beast;
    s.m[TIER + cu] = tier;
    settle_tier(s, r, p, c);

    if s.has_tool(tool::MAIDEN) && s.m[TIER + cu] >= 0 {
        let mut above = 0;
        let mut below = 0;
        for t in 1..CELLS {
            if t == cu || s.m[TIER + t] < 0 {
                continue;
            }
            if s.m[TIER + t] > tier {
                above += 1;
            } else if s.m[TIER + t] < tier {
                below += 1;
            }
        }
        if above == below {
            raise(s, r, p, c, p.maiden_bonus);
        }
    }
    if s.has_tool(tool::AXE) && tier > old_tier {
        for _ in 0..p.axe_opens {
            open_random(s, r, p);
        }
    }
    if s.has_tool(tool::SCALES) && r.chance(p.scales_chance) {
        reveal_best(s, r, p);
    }
    if tusk && s.has_tool(tool::TONGS) {
        raise_random(s, r, p, p.tongs_targets, 1);
    }
}

/// Весы тюремщика: показать лучшего узника за запертыми дверями.
pub fn reveal_best(s: &mut State, r: &mut Rng, p: &Params) {
    let mut best = -1;
    let mut mask = 0;
    for c in 1..C {
        if s.m[LOCK + u(c)] != 1 {
            continue;
        }
        preset(s, r, p, c);
        let t = s.m[PRE_TIER + u(c)];
        if t > best {
            best = t;
            mask = 1 << c;
        } else if t == best {
            mask |= 1 << c;
        }
    }
    let c = pick_bit(mask, r);
    if c >= 0 {
        s.m[FLAGS + u(c)] |= F_VIS_TIER | F_VIS_BEAST;
    }
}

// ── выбор орудий и подарков ─────────────────────────────────────────────────

/// Вес орудия t в выборе: 0 — уже есть или исключено имеющимся (в обе стороны).
pub fn tool_weight(s: &State, t: usize) -> i32 {
    let own = s.m[TOOLS];
    if (own >> t) & 1 != 0 {
        return 0;
    }
    if TOOL_EXCL[t] & own != 0 {
        return 0;
    }
    for (o, &excl) in TOOL_EXCL.iter().enumerate() {
        if (own >> o) & 1 != 0 && (excl >> t) & 1 != 0 {
            return 0;
        }
    }
    TOOL_W[t]
}

/// Предложить выбор: подарки (все 4) раньше орудий. false — предлагать нечего.
pub fn make_offer(s: &mut State, r: &mut Rng) -> bool {
    if s.m[PICK_GIFTS] > 0 {
        s.m[OFFER_KIND] = OFFER_GIFT;
        s.m[OFFER_N] = 4;
        for i in 0..4 {
            s.m[OFFER + i] = i as i32;
        }
        return true;
    }
    let mut offer_w = [0i32; TOOL_COUNT];
    let mut total = 0;
    for (t, w) in offer_w.iter_mut().enumerate() {
        *w = tool_weight(s, t);
        total += *w;
    }
    if total == 0 {
        s.m[PICK_TOOLS] = 0;
        return false;
    }
    s.m[OFFER_KIND] = OFFER_TOOL;
    let mut n = 0;
    while n < TOOL_OFFER && total > 0 {
        let mut x = r.int(total);
        let mut t = 0;
        while t < TOOL_COUNT - 1 {
            x -= offer_w[t];
            if x < 0 {
                break;
            }
            t += 1;
        }
        total -= offer_w[t];
        offer_w[t] = 0;
        s.m[OFFER + n as usize] = t as i32;
        n += 1;
    }
    s.m[OFFER_N] = n;
    true
}

/// Взять орудие.
pub fn take_tool(s: &mut State, t: i32) {
    s.m[TOOLS] |= 1 << t;
    s.m[PICK_TOOLS] -= 1;
}

/// Взять подарок сокровищницы.
pub fn take_gift(s: &mut State, r: &mut Rng, p: &Params, gift: i32) {
    s.m[PICK_GIFTS] -= 1;
    match gift {
        // Трёхглавое кольцо
        0 => {
            for _ in 0..p.ring_opens {
                open_random(s, r, p);
            }
        }
        // Медный боевой рог: каждый с целью кусает один раз
        1 => {
            for c in 1..C {
                if s.m[TIER + u(c)] < 0 {
                    continue;
                }
                let t = pick_bit(targets(s, c), r);
                if t >= 0 {
                    bite(s, r, p, c, t, false);
                }
            }
        }
        // Багряное вино
        2 => {
            for c in 1..C {
                let t = s.m[TIER + u(c)];
                if t >= 0 && t < LEGENDARY {
                    raise(s, r, p, c, 1);
                }
            }
        }
        // Клеймо первородного греха
        3 => brand(s, r, p),
        _ => {}
    }
}

/// До brandCount узников без еды меняют зверя так, чтобы еда появилась.
fn brand(s: &mut State, r: &mut Rng, p: &Params) {
    let mut hungry = 0;
    for c in 1..C {
        if s.m[TIER + u(c)] >= 0 && targets(s, c) == 0 {
            hungry |= 1 << c;
        }
    }
    let mut fed = 0;
    let mut done = 0;
    while hungry != 0 && done < p.brand_count {
        let c = pick_bit(hungry, r);
        hungry &= !(1 << c);
        if targets(s, c) != 0 {
            continue;
        }
        let cu = u(c);
        let old = s.m[BEAST + cu];
        let mut ok = 0;
        for b in 0..4 {
            if b == old {
                continue;
            }
            s.m[BEAST + cu] = b;
            if targets(s, c) != 0 && all_fed(s, fed) {
                ok |= 1 << b;
            }
        }
        let b = pick_bit(ok, r);
        s.m[BEAST + cu] = if b >= 0 { b } else { old };
        if b >= 0 {
            forget(s, c);
            fed |= 1 << c;
            done += 1;
        }
    }
}

fn all_fed(s: &State, mask: i32) -> bool {
    for c in 1..C {
        if (mask >> c) & 1 == 1 && targets(s, c) == 0 {
            return false;
        }
    }
    true
}

// ── начало забега ───────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug)]
pub struct RunSetup {
    pub start: i32,
    pub objective: i32,
    pub tools: i32,
    pub start_pick: bool,
}

/// Новый забег: надзиратель в допросной, 4 случайные камеры открыты, остальные заперты, метки событий.
pub fn new_run(s: &mut State, r: &mut Rng, p: &Params, setup: RunSetup) {
    s.m = [0; SIZE];
    for c in 0..CELLS {
        s.m[BEAST + c] = -1;
        s.m[TIER + c] = -1;
        s.m[PRE_TIER + c] = -1;
        s.m[PRE_BEAST + c] = -1;
        if c as i32 != OFFICE {
            s.m[LOCK + c] = 1;
        }
    }
    s.m[USURP] = event::USURP1;
    s.m[OBJECTIVE_ID] = -1; // цель ставится в конце: стартовые камеры её не двигают
    s.m[TOOLS] = setup.tools & ALL_TOOLS;
    s.m[START] = setup.start;
    s.m[WARDEN] = OFFICE;
    s.m[DAY_LIMIT] = if setup.start == start_event::IRONLAW { p.iron_law_days } else { p.days };
    s.m[REROLLS] = p.rerolls;
    s.m[CHAIN_EATER] = -1;
    for c in 0..CELLS {
        s.m[PREY_LEFT + c] = 0;
        s.m[PREY_LION + c] = -1;
    }
    s.m[PICK_TOOLS] = setup.start_pick as i32;
    s.m[NEXT] = P_MORNING;
    // С выбором орудия доска за ним ещё не видна: её разыграет первое утро.
    if setup.start_pick {
        s.m[START_PENDING] = 1;
    } else {
        start_board(s, r, p);
    }
    set_objective(s, r, p, setup.objective);
}

/// Стартовая доска: startOpen случайных камер открыто, метки событий до eventMin.
fn start_board(s: &mut State, r: &mut Rng, p: &Params) {
    for _ in 0..p.start_open {
        let c = pick_bit(locked_mask(s), r);
        if c < 0 {
            break;
        }
        let tier = roll_tier(r, &p.door_odds, 0);
        s.m[STAT_OPENED] += 1;
        let beast = r.int(4);
        place(s, r, p, c, beast, tier);
    }
    for c in 1..C {
        if s.m[LOCK + u(c)] == 1 && r.chance(p.door_event) {
            put_event(s, r, p, c);
        }
    }
    top_up_events(s, r, p);
}
