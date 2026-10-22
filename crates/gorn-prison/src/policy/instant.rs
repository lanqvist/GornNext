//! Мгновенная стратегия: ход без перебора вариантов. Перенос `policy/instant.ts`.
//!
//! Доска оценивается суммой ценностей узников по редкости (веса, настроенные
//! самоигрой) плюс мелкие слагаемые. Пожирание считается точно: цепочка у
//! едока идёт только вверх по редкости, поэтому матожидание цепочки зависит
//! лишь от (редкость, зверь) едока и состава доски — таблица 6 × 4 × 2.
//!
//! Стратегия видит только то, что видит игрок.
//!
//! Ради совпадения с эталоном до бита арифметика f64 идёт в том же порядке,
//! что в TS, а `Math.max` заменён на [`jmax`] с его правилами для NaN и нулей.
//! Рабочие таблицы, которые в эталоне — модульные переменные, здесь живут в
//! [`Instant`]: по одному на поток.

use crate::actions::{
    apply_trusted, code, A_ACCEPT, A_BESTOW, A_CHAIN, A_DEVOUR, A_END, A_GIFT, A_HUNT, A_INSTIGATE, A_PATROL,
    A_REROLL, A_STOPCHAIN, A_SWAP, A_TOOL,
};
use crate::model::{start_event, tool, Params, CELLS, LEGENDARY, LION, MAMMOTH, OFFICE, RAT, WOLF};
use crate::rng::Rng;
use crate::rules::{hunt_targets, office_rounds, targets, FIRST_FOOD, SECOND_FOOD};
use crate::state::*;

use super::weights::*;

const C: i32 = CELLS as i32;
const CM: i32 = C - 1;

/// `Math.max(a, b)` из JS: NaN заразен, +0 больше −0.
#[inline(always)]
pub fn jmax(a: f64, b: f64) -> f64 {
    if a > b {
        a
    } else if b > a {
        b
    } else if a == b {
        if a == 0.0 && a.is_sign_negative() {
            b
        } else {
            a
        }
    } else {
        f64::NAN
    }
}

#[inline(always)]
fn u(c: i32) -> usize {
    c as usize
}

/// Рабочие таблицы стратегии (переиспользуются между вызовами).
#[derive(Clone)]
pub struct Instant {
    /// Ценность узника по редкости 0…4; [5] — ценность ухода в хранилище.
    val: [f64; 6],
    /// Узники доски: [редкость * 4 + зверь].
    cnt: [i32; 24],
    /// F[(t * 4 + b) * 2 + c]: ценность едока (t, b), который может есть дальше; c — укус в цепочке.
    f: [f64; 48],
    /// Среднее F по новому зверю: A[t * 2 + c].
    a: [f64; 12],
    /// Кто выйдет из двери, по редкости (с учётом «Крюка»).
    p_door: [f64; 6],
    /// Кто выйдет при замене (с учётом «Петли»).
    p_rep: [f64; 6],
    /// Выгода Замены гипотетического узника по редкости. [5] — NaN: в эталоне
    /// это чтение за концом Float64Array(5) (undefined → NaN).
    swap_new: [f64; 6],
    swap_ready: bool,
    frac: f64,
    prism_v: f64,
    tools: i32,
    any_tier: bool,
    inmate_count: i32,
    locked_count: i32,
    val_sum: f64,
    /// Средний прирост от +1 случайному узнику.
    avg_rise: f64,
    /// Ценность нового узника из двери, по шансам.
    door_new: f64,
    /// Открыть случайную запертую дверь: новый узник минус потерянная дверь.
    open_v: f64,
    crown: bool,
    skewer: bool,
    maul: bool,
    guillotine: bool,
    chain_differs: bool,
    chain_ready: bool,
    p_whip: f64,
    p_collar: f64,
    muzzle: i32,
    cage_bonus: f64,
    /// Притягательность камер для обхода.
    attr: [f64; CELLS],
    hunt_j: i32,
    hunt_gain: f64,
    best_j: i32,
    best_gain: f64,
    /// Почему выбран последний ход (R_*): для подписи.
    why: u8,
}

/// Почему выбран ход: для подписи.
pub const R_NONE: u8 = 0;
pub const R_TOOL: u8 = 1;
pub const R_REROLL: u8 = 2;
pub const R_GIFT: u8 = 3;
pub const R_HUNT: u8 = 4;
pub const R_DOOR_EAT: u8 = 5;
pub const R_DOOR_NEW: u8 = 6;
pub const R_DOOR_EVENT: u8 = 7;
pub const R_DOOR_OFFICE: u8 = 8;
pub const R_DOOR_PRISM: u8 = 9;
pub const R_DEVOUR: u8 = 10;
pub const R_DEVOUR_PRISM: u8 = 11;
pub const R_CHAIN: u8 = 12;
pub const R_STOP: u8 = 13;
pub const R_SWAP: u8 = 14;
pub const R_END: u8 = 15;
pub const R_BESTOW: u8 = 16;
pub const R_INSTIGATE: u8 = 17;
pub const R_ACCEPT: u8 = 18;
pub const R_SKIP: u8 = 19;
pub const R_RATWAY: u8 = 20;

impl Default for Instant {
    fn default() -> Self {
        Self::new()
    }
}

/// Один вызов стратегии: состояние, правила, веса и рабочие таблицы.
struct Ctx<'a> {
    s: &'a State,
    m: &'a [i32; SIZE],
    p: &'a Params,
    w: &'a Weights,
    k: &'a mut Instant,
}

impl Instant {
    pub fn new() -> Self {
        Instant {
            val: [0.0; 6],
            cnt: [0; 24],
            f: [0.0; 48],
            a: [0.0; 12],
            p_door: [0.0; 6],
            p_rep: [0.0; 6],
            swap_new: [0.0, 0.0, 0.0, 0.0, 0.0, f64::NAN],
            swap_ready: false,
            frac: 1.0,
            prism_v: 1.0,
            tools: 0,
            any_tier: false,
            inmate_count: 0,
            locked_count: 0,
            val_sum: 0.0,
            avg_rise: 0.0,
            door_new: 0.0,
            open_v: 0.0,
            crown: false,
            skewer: false,
            maul: false,
            guillotine: false,
            chain_differs: false,
            chain_ready: false,
            p_whip: 0.0,
            p_collar: 0.0,
            muzzle: 0,
            cage_bonus: 0.0,
            attr: [0.0; CELLS],
            hunt_j: -1,
            hunt_gain: f64::NEG_INFINITY,
            best_j: -1,
            best_gain: f64::NEG_INFINITY,
            why: R_NONE,
        }
    }

    /// Код хода мгновенной стратегии. Без аллокаций.
    /// Паника — если забег окончен (в эталоне — исключение).
    pub fn code(&mut self, s: &State, p: &Params, w: &Weights) -> i32 {
        let mut x = Ctx { s, m: &s.m, p, w, k: self };
        x.prepare();
        match x.m[PHASE] {
            P_PICK => {
                if x.m[OFFER_KIND] == OFFER_GIFT {
                    x.pick_gift()
                } else {
                    x.pick_tool()
                }
            }
            P_PATROL => x.pick_door(),
            P_EVENT => {
                // Событие пропустить нельзя (в игре только «Подтвердить»).
                x.k.why = R_ACCEPT;
                code(A_ACCEPT, 0)
            }
            P_OFFICE => x.pick_office(),
            P_RATWAY => x.pick_ratway(),
            P_MANAGE => x.pick_manage(),
            P_CHAIN => x.pick_chain(),
            _ => panic!("тюрьма: забег окончен, ходов нет"),
        }
    }

    /// Ход и короткое объяснение (≤ 40 символов) для подписи в Горне (`advise` эталона).
    pub fn advise(&mut self, s: &State, p: &Params, w: &Weights) -> (i32, String) {
        let c = self.code(s, p, w);
        let arg = c & 63;
        let n = arg + 1; // игроку камеры с 1
        let short_tool = |i: i32| crate::model::TOOL_NAMES[i as usize].split(':').next().unwrap_or("").to_string();
        let text = match self.why {
            R_TOOL => format!("Взять «{}»", short_tool(arg)),
            R_REROLL => "Обновить: все три карты слабые".to_string(),
            R_GIFT => format!("Подарок: {}", crate::model::GIFT_NAMES[arg as usize]),
            R_HUNT => format!("Выслеживание: узник в камере {n}"),
            R_DOOR_PRISM => format!("Камера {n}: цепочка к призматическому"),
            R_DOOR_EAT => format!("Камера {n}: узник съест соседа"),
            R_DOOR_EVENT => format!("Камера {n}: событие"),
            R_DOOR_OFFICE => "В допросную".to_string(),
            R_DOOR_NEW => {
                if arg == OFFICE {
                    "В допросную".to_string()
                } else {
                    format!("Камера {n}: новый узник")
                }
            }
            R_DEVOUR_PRISM => format!("Съесть {n} → Призматический"),
            R_DEVOUR => format!("Съесть узника в камере {n}"),
            R_CHAIN => format!("Есть дальше: камера {n}"),
            R_STOP => "Отменить: дальше невыгодно".to_string(),
            R_SWAP => "Заменить узника".to_string(),
            R_END => "Завершить день".to_string(),
            R_BESTOW => "Наградить случайного узника".to_string(),
            R_INSTIGATE => "Подстрекнуть: события за дверями".to_string(),
            R_ACCEPT => "Принять событие".to_string(),
            R_SKIP => "Пропустить событие".to_string(),
            R_RATWAY => format!("Тропа: в камеру {n}"),
            _ => String::new(),
        };
        (c, fit(&text, 40))
    }

    /// Оценки четырёх подарков на позиции (для разбора и тестов).
    pub fn gift_values(&mut self, s: &State, p: &Params, w: &Weights) -> [f64; 4] {
        let mut x = Ctx { s, m: &s.m, p, w, k: self };
        x.prepare();
        [x.gift_value(0), x.gift_value(1), x.gift_value(2), x.gift_value(3)]
    }

    /// Доиграть забег мгновенной стратегией; вернуть счёт.
    pub fn playout(&mut self, s: &mut State, r: &mut Rng, p: &Params, w: &Weights) -> i32 {
        while s.m[PHASE] != P_END {
            let c = self.code(s, p, w);
            apply_trusted(s, c, r, p);
        }
        s.m[STORAGE]
    }
}

/// Не длиннее max кодовых единиц UTF-16 (как length в JS): иначе обрезать с «…».
pub fn fit(text: &str, max: usize) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    if units.len() <= max {
        return text.to_string();
    }
    let mut out = String::from_utf16_lossy(&units[..max - 1]);
    out.push('…');
    out
}

fn normalize(out: &mut [f64; 6], odds: &[f64; 6], floor: usize) {
    let mut sum = 0.0;
    for (t, &o) in odds.iter().enumerate() {
        sum += if t >= floor { o } else { 0.0 };
    }
    for t in 0..6 {
        out[t] = if t >= floor && sum > 0.0 { odds[t] / sum } else { 0.0 };
    }
}

impl Ctx<'_> {
    #[inline(always)]
    fn has(&self, t: u32) -> bool {
        (self.k.tools >> t) & 1 == 1
    }

    /// val[t] с чтением за границей как в JS: Float64Array[−1] — undefined → NaN.
    /// Бывает у Замены, когда адаптер ставит ряд решений на камеру без узника.
    #[inline(always)]
    fn val_js(&self, t: i32) -> f64 {
        if (0..6).contains(&t) {
            self.k.val[t as usize]
        } else {
            f64::NAN
        }
    }

    #[inline(always)]
    fn rise_v(&self, t: i32, n: i32) -> f64 {
        let uu = t + n;
        if uu >= 5 {
            self.k.prism_v
        } else {
            self.k.val[u(uu)]
        }
    }

    /// Подготовить таблицы для состояния.
    fn prepare(&mut self) {
        let m = self.m;
        let w = self.w;
        let p = self.p;
        let k = &mut *self.k;
        let limit = m[DAY_LIMIT];
        k.frac = if limit > 0 { (limit - m[DAY]) as f64 / limit as f64 } else { 0.0 };
        for t in 0..5 {
            k.val[t] = w[W_VAL1 + t] + (w[W_VAL0 + t] - w[W_VAL1 + t]) * k.frac;
        }
        k.prism_v = if m[STORAGE] >= p.storage { k.val[4] } else { 1.0 + w[W_GIFT] };
        k.val[5] = k.prism_v;
        k.tools = m[TOOLS];
        k.any_tier = m[START] == start_event::CAGEFIGHT;

        k.cnt = [0; 24];
        k.inmate_count = 0;
        k.locked_count = 0;
        k.val_sum = 0.0;
        let mut rise_sum = 0.0;
        for c in 1..CELLS {
            if m[LOCK + c] == 1 {
                k.locked_count += 1;
            }
            let t = m[TIER + c];
            if t < 0 {
                continue;
            }
            k.cnt[u(t * 4 + m[BEAST + c])] += 1;
            k.inmate_count += 1;
            k.val_sum += k.val[u(t)];
            let up = if t + 1 >= 5 { k.prism_v } else { k.val[u(t + 1)] };
            rise_sum += up - k.val[u(t)];
        }
        k.avg_rise = if k.inmate_count > 0 { rise_sum / k.inmate_count as f64 } else { 0.0 };

        let has = |t: u32| (k.tools >> t) & 1 == 1;
        k.crown = has(tool::CROWN);
        k.skewer = has(tool::SKEWER);
        k.maul = has(tool::MAUL);
        k.guillotine = has(tool::GUILLOTINE);
        k.p_whip = if has(tool::WHIP) { p.whip_chance } else { 0.0 };
        k.p_collar = if has(tool::COLLAR) { p.collar_chance } else { 0.0 };
        // Намордник: на поле до укуса (с едоком и жертвой) не больше muzzleLeft узников.
        k.muzzle = if has(tool::MUZZLE) && k.inmate_count <= p.muzzle_left { p.muzzle_bonus } else { 0 };
        k.chain_differs = k.p_collar > 0.0;
        k.chain_ready = false;
    }

    /// Шансы дверей и замены, таблица цепочки — по требованию.
    fn ensure_chain(&mut self) {
        if self.k.chain_ready {
            return;
        }
        self.k.chain_ready = true;
        let hook = self.has(tool::HOOK);
        let rope = self.has(tool::ROPE);
        let cage = self.has(tool::CAGE);
        let k = &mut *self.k;
        normalize(&mut k.p_door, &self.p.door_odds, if hook { 1 } else { 0 });
        // Крюк — только двери; замену ограничивает Петля.
        normalize(&mut k.p_rep, &self.p.replace_odds, if rope { 2 } else { 0 });
        k.door_new = 0.0;
        for t in 0..6 {
            k.door_new += k.p_door[t] * k.val[t];
        }
        k.open_v = k.door_new - self.w[W_DOOR] * k.frac;
        k.cage_bonus = if cage { self.p.cage_chance * k.door_new } else { 0.0 };
        self.fill_chain();
        self.k.swap_ready = false;
    }

    /// Ценность после укуса: едок поднялся до base (+ случайные добавки).
    #[inline(always)]
    fn after_bite(&self, t: i32, b: i32, base: i32, chain: usize) -> f64 {
        let pc = if chain != 0 { self.k.p_collar } else { 0.0 };
        let pw = self.k.p_whip;
        let p0 = (1.0 - pw) * (1.0 - pc);
        let p2 = pw * pc;
        let p1 = 1.0 - p0 - p2;
        p0 * self.next(t + base, b) + p1 * self.next(t + base + 1, b) + p2 * self.next(t + base + 2, b)
    }

    #[inline(always)]
    fn next(&self, t: i32, b: i32) -> f64 {
        if t >= 5 {
            return self.k.prism_v;
        }
        if self.k.any_tier {
            self.k.f[u((t * 4 + b) * 2 + 1)]
        } else {
            self.k.a[u(t * 2 + 1)]
        }
    }

    /// Базовый прирост укуса: 1, Молот, добавки Шампура, Короны и Намордника.
    #[inline(always)]
    fn base_gain(&self, be: i32, te: i32, food: bool, tt: i32, victim_rat: bool) -> i32 {
        let mut g = if be == MAMMOTH && food && self.k.maul { (tt + 2 - te).max(1) } else { 1 };
        if be == RAT && victim_rat && self.k.skewer {
            g += self.p.skewer_bonus;
        }
        if be == LION && self.k.crown {
            g += self.p.crown_bonus;
        }
        g + self.k.muzzle
    }

    /// Вариант укуса: съесть узника редкости uu.
    #[inline(always)]
    fn bite_option(&self, t: i32, b: i32, c: usize, uu: i32, is_food: bool, victim_rat: bool) -> f64 {
        -self.k.val[u(uu)] + self.k.cage_bonus + self.after_bite(t, b, self.base_gain(b, t, is_food, uu, victim_rat), c)
    }

    #[inline(always)]
    fn has_food(&self, uu: i32, f1: i32, f2: i32) -> bool {
        self.k.cnt[u(uu * 4 + f1)] > 0 || (f2 >= 0 && self.k.cnt[u(uu * 4 + f2)] > 0)
    }

    /// Лучший вариант укуса для едока (t, b) по составу доски; −∞, если есть некого.
    fn best_bite(&self, t: i32, b: i32, c: usize) -> f64 {
        let f1 = FIRST_FOOD[u(b)];
        let f2 = SECOND_FOOD[u(b)];
        let mut best = f64::NEG_INFINITY;
        let row = u(t * 4);
        let cnt = &self.k.cnt;
        if self.has_food(t, f1, f2) {
            best = self.bite_option(t, b, c, t, true, false);
        } else if b == LION && self.k.crown && cnt[row] + cnt[row + 1] + cnt[row + 2] + cnt[row + 3] > 0 {
            best = self.bite_option(t, b, c, t, false, false);
        }
        if b == RAT && self.k.skewer && cnt[row + RAT as usize] > 0 {
            best = jmax(best, self.bite_option(t, b, c, t, false, true));
        }
        // «Схватка узников» (клиент 304) — еда ниже по редкости; Молот (142) — выше.
        if self.k.any_tier {
            for uu in 0..t {
                if self.has_food(uu, f1, f2) {
                    best = jmax(best, self.bite_option(t, b, c, uu, true, false));
                }
            }
        }
        if b == MAMMOTH && self.k.maul {
            for uu in t + 1..5 {
                if self.has_food(uu, f1, f2) {
                    best = jmax(best, self.bite_option(t, b, c, uu, true, false));
                }
            }
        }
        if b == WOLF && self.k.guillotine {
            for uu in 0..t {
                if cnt[u(uu * 4 + RAT)] > 0 {
                    best = jmax(best, self.bite_option(t, b, c, uu, false, false));
                }
            }
        }
        best
    }

    fn fill_chain(&mut self) {
        let pv = self.k.prism_v;
        for b in 0..4 {
            for c in 0..2 {
                self.k.f[(5 * 4 + b) * 2 + c] = pv;
            }
        }
        self.k.a[10] = pv;
        self.k.a[11] = pv;
        for t in (0..5i32).rev() {
            // c = 1 первым: варианты c = 0 смотрят только на ступени выше
            for c in [1usize, 0] {
                if c == 0 && !self.k.chain_differs {
                    for b in 0..4 {
                        let i = u((t * 4 + b) * 2);
                        self.k.f[i] = self.k.f[i + 1];
                    }
                    self.k.a[u(t * 2)] = self.k.a[u(t * 2 + 1)];
                    continue;
                }
                let mut sum = 0.0;
                for b in 0..4 {
                    let v = jmax(self.k.val[u(t)], self.best_bite(t, b, c));
                    self.k.f[u((t * 4 + b) * 2) + c] = v;
                    sum += v;
                }
                self.k.a[u(t * 2) + c] = sum / 4.0;
            }
        }
    }

    // ── оценки ходов ────────────────────────────────────────────────────────

    /// Выгода укуса e → j (от ценности едока до укуса).
    fn bite_gain(&self, e: i32, j: i32, chain: usize) -> f64 {
        let m = self.m;
        let te = m[TIER + u(e)];
        let be = m[BEAST + u(e)];
        let cost;
        let base;
        if m[LOCK + u(j)] == 1 {
            cost = 0.0; // Когти: сквозь дверь, камера остаётся запертой
            base = self.p.claws_gain + self.k.muzzle;
        } else {
            let tt = m[TIER + u(j)];
            let bt = m[BEAST + u(j)];
            let food = bt == FIRST_FOOD[u(be)] || bt == SECOND_FOOD[u(be)];
            cost = self.k.val[u(tt)] - self.k.cage_bonus;
            base = self.base_gain(be, te, food, tt, bt == RAT);
        }
        -cost + self.after_bite(te, be, base, chain) - self.k.val[u(te)]
    }

    /// Выгода Замены узника редкости t (в камере c, −1 — гипотетический).
    fn swap_gain(&self, t: i32, c: i32) -> f64 {
        let k = &*self.k;
        let p = self.p;
        let mut v;
        let mut p_up = 0.0;
        let others = k.inmate_count - if c >= 0 { 1 } else { 0 };
        if self.has(tool::SCAPEGOAT) && others >= p.scapegoat_removes {
            // Бросок не ниже прежней + 1 и не ниже порога Петли — как в движке.
            let avg_other =
                if others > 0 { (k.val_sum - if c >= 0 { self.val_js(t) } else { 0.0 }) / others as f64 } else { 0.0 };
            let floor = (t + 1).max(if self.has(tool::ROPE) { 2 } else { 0 });
            v = self.rise_v(t, floor - t) - p.scapegoat_removes as f64 * avg_other;
            p_up = 1.0;
        } else {
            v = 0.0;
            let stake = if self.has(tool::STAKE) && t < LEGENDARY { p.stake_chance } else { 0.0 };
            for uu in 0..6i32 {
                let q = k.p_rep[u(uu)];
                if q == 0.0 {
                    continue;
                }
                if uu < LEGENDARY && stake > 0.0 {
                    v += q * ((1.0 - stake) * k.val[u(uu)] + stake * k.val[LEGENDARY as usize]);
                    if uu > t {
                        p_up += q * (1.0 - stake);
                    }
                    if LEGENDARY > t {
                        p_up += q * stake;
                    }
                } else {
                    v += q * k.val[u(uu)];
                    if uu > t {
                        p_up += q;
                    }
                }
            }
        }
        if self.has(tool::AXE) {
            v += p_up * p.axe_opens as f64 * k.open_v;
        }
        if c >= 0 && self.has(tool::TONGS) && self.m[FLAGS + u(c)] & F_TUSK != 0 {
            v += p.tongs_targets as f64 * k.avg_rise;
        }
        v - self.val_js(t) + self.w[W_SWAP]
    }

    /// Лучшее действие дня для узника в камере c: 0 — конец дня.
    fn manage_value(&self, c: i32) -> f64 {
        let t = self.m[TIER + u(c)];
        if t < 0 {
            return 0.0;
        }
        let mut best = jmax(0.0, self.swap_gain(t, c));
        let mask = targets(self.s, c);
        for j in 1..C {
            if (mask >> j) & 1 == 1 {
                best = jmax(best, self.bite_gain(c, j, 0));
            }
        }
        best
    }

    /// Ценность нового узника (t, b) в камере и его лучшего действия сегодня.
    fn new_inmate_value(&mut self, t: i32, b: i32) -> f64 {
        let dev = self.k.f[u((t * 4 + b) * 2)] - self.k.val[u(t)];
        if !self.k.swap_ready {
            for uu in 0..5 {
                self.k.swap_new[uu] = self.swap_gain(uu as i32, -1);
            }
            self.k.swap_ready = true;
        }
        self.k.val[u(t)] + jmax(jmax(0.0, dev), self.k.swap_new[u(t)])
    }

    /// Узник за запертой дверью d: ожидаемая ценность с действием дня.
    fn door_inmate_value(&mut self, d: i32) -> f64 {
        let fl = self.m[FLAGS + u(d)];
        if fl & F_VIS_TIER != 0 {
            let t = self.m[PRE_TIER + u(d)];
            if fl & F_VIS_BEAST != 0 {
                return self.new_inmate_value(t, self.m[PRE_BEAST + u(d)]);
            }
            let mut v = 0.0;
            for b in 0..4 {
                v += self.new_inmate_value(t, b);
            }
            return v / 4.0;
        }
        let mut sum = 0.0;
        let mut norm = 0.0;
        let floor = if fl & F_VIS_RARE != 0 { 2 } else { 0 };
        for t in floor..5 {
            let q = self.k.p_door[u(t)];
            if q == 0.0 {
                continue;
            }
            let mut v = 0.0;
            for b in 0..4 {
                v += self.new_inmate_value(t, b);
            }
            sum += (q * v) / 4.0;
            norm += q;
        }
        if norm > 0.0 {
            sum / norm
        } else {
            0.0
        }
    }

    #[inline]
    fn bestow_value(&self) -> f64 {
        self.k.avg_rise
    }

    fn instigate_value(&self) -> f64 {
        let mut free = 0;
        for c in 1..CELLS {
            if self.m[EVT + c] == 0 {
                free += 1;
            }
        }
        self.p.instigate_doors.min(free) as f64 * self.w[W_INSTIGATE]
    }

    /// Ценность прихода в камеру c (без орудий хода).
    fn cell_value(&mut self, c: i32) -> f64 {
        let p = self.p;
        if c == OFFICE {
            let bv = self.bestow_value();
            let iv = self.instigate_value();
            // «Злоупотребление властью» (клиент 301): ещё автоматически Подстрекнуть и Наградить
            let auto = if self.m[START] == start_event::TYRANNY { p.tyranny_extra as f64 * (bv + iv) } else { 0.0 };
            return office_rounds(self.s, p) as f64 * jmax(bv, iv) + auto;
        }
        let mut v = if self.m[EVT + u(c)] != 0 { self.w[W_EVENT] } else { 0.0 };
        if self.m[LOCK + u(c)] != 1 {
            return v + self.manage_value(c);
        }
        v += self.door_inmate_value(c) - self.w[W_DOOR] * self.k.frac;
        // орудия при открытии обходом
        let k = &*self.k;
        if self.has(tool::DOGTAG) {
            v += p.dogtag_chance * k.avg_rise;
        }
        if self.has(tool::KEYS) {
            let l = if self.m[LOCK + u((c + C - 1) & CM)] == 1 { 1 } else { 0 };
            let r = if self.m[LOCK + u((c + 1) & CM)] == 1 { 1 } else { 0 };
            v += p.keys_chance * (l + r) as f64 * k.open_v;
        }
        if self.has(tool::FORGED) {
            v += p.forged_chance * k.open_v;
        }
        if self.has(tool::FIRE) {
            v += p.fire_chance * k.open_v;
        }
        // «Чёрный список»: запертых до открытия этой двери (прежняя модель — после).
        let locked = if p.blacklist_before_open { k.locked_count } else { k.locked_count - 1 };
        if self.has(tool::BLACKLIST) && locked <= p.blacklist_locked {
            v += p.blacklist_count as f64 * k.avg_rise;
        }
        v
    }

    fn fill_attr(&mut self) {
        for c in 0..C {
            let v = jmax(0.0, self.cell_value(c));
            self.k.attr[u(c)] = v;
        }
    }

    /// Ценность хода к двери c: сама камера, что окажется впереди и что ход проскочит.
    fn door_value(&mut self, from: i32, c: i32) -> f64 {
        let steps = dist(from, c);
        let w = self.w;
        let p = self.p;
        let mut v = self.cell_value(c) + w[W_FAR] * steps as f64;
        if w[W_AHEAD] != 0.0 || w[W_SKIP] != 0.0 {
            let mut ahead = 0.0;
            for k in 1..=p.door_reach {
                ahead += self.k.attr[u((c + k) & CM)];
            }
            let mut skipped = 0.0;
            for k in 1..steps {
                skipped += self.k.attr[u((from + k) & CM)];
            }
            v += (w[W_AHEAD] * ahead) / p.door_reach as f64 - w[W_SKIP] * skipped;
        }
        if self.has(tool::SHORTCHAIN) && steps <= p.shortchain_steps {
            v += p.shortchain_chance * self.k.open_v;
        }
        if self.has(tool::SHACKLES) && steps >= p.shackles_steps {
            v += p.shackles_chance * self.k.avg_rise;
        }
        v
    }

    // ── решения ─────────────────────────────────────────────────────────────

    fn pick_tool(&mut self) -> i32 {
        let m = self.m;
        let mut best = -1;
        let mut best_v = f64::NEG_INFINITY;
        for i in 0..m[OFFER_N] as usize {
            let id = m[OFFER + i];
            let v = self.w[W_TOOL + u(id)];
            if v > best_v {
                best_v = v;
                best = id;
            }
        }
        if m[REROLLS] > 0 && best_v < self.w[W_REROLL] {
            self.k.why = R_REROLL;
            return code(A_REROLL, 0);
        }
        self.k.why = R_TOOL;
        code(A_TOOL, best)
    }

    /// Укус «Медного рога»: без цепочки, цель — та, что дана. Скачок Молота
    /// (мамонт — на 2 выше съеденного) Рогу не засчитывается: тот же укус даёт
    /// и обычное Пожирание мамонта (gorn-tli 1.0.32, 0201290).
    fn horn_bite(&self, e: i32, j: i32) -> f64 {
        let m = self.m;
        let te = m[TIER + u(e)];
        let be = m[BEAST + u(e)];
        if m[LOCK + u(j)] == 1 {
            return self.rise_v(te, self.p.claws_gain) - self.k.val[u(te)];
        }
        let tt = m[TIER + u(j)];
        let bt = m[BEAST + u(j)];
        -self.k.val[u(tt)] + self.rise_v(te, self.base_gain(be, te, false, tt, bt == RAT)) - self.k.val[u(te)]
    }

    /// Ценность подарка по доске — без пробного применения. «Рог» — по доске до
    /// первого укуса: кусают те, у кого цель уже есть (правило движка с 22.10).
    fn gift_value(&mut self, g: i32) -> f64 {
        self.ensure_chain();
        let m = self.m;
        let p = self.p;
        let mut v = 0.0;
        match g {
            0 => return self.k.locked_count.min(p.ring_opens) as f64 * (self.k.open_v + self.w[W_RING]),
            1 => {
                for c in 1..C {
                    if m[TIER + u(c)] < 0 {
                        continue;
                    }
                    let mask = targets(self.s, c);
                    if mask == 0 {
                        continue;
                    }
                    let mut sum = 0.0;
                    let mut k = 0;
                    for j in 1..C {
                        if (mask >> j) & 1 == 0 {
                            continue;
                        }
                        sum += self.horn_bite(c, j);
                        k += 1;
                    }
                    v += sum / k as f64;
                }
            }
            2 => {
                for c in 1..CELLS {
                    let t = m[TIER + c];
                    if t >= 0 && t < LEGENDARY {
                        v += self.rise_v(t, 1) - self.k.val[u(t)];
                    }
                }
            }
            3 => {
                let mut hungry = 0;
                for c in 1..C {
                    if m[TIER + u(c)] >= 0 && targets(self.s, c) == 0 {
                        hungry += 1;
                    }
                }
                return p.brand_count.min(hungry) as f64 * self.w[W_BRAND] * self.k.frac;
            }
            _ => {}
        }
        v
    }

    fn pick_gift(&mut self) -> i32 {
        let mut best = 0;
        let mut best_v = f64::NEG_INFINITY;
        for i in 0..self.m[OFFER_N] as usize {
            let g = self.m[OFFER + i];
            let v = self.gift_value(g);
            if v > best_v {
                best_v = v;
                best = g;
            }
        }
        self.k.why = R_GIFT;
        code(A_GIFT, best)
    }

    /// Лучшая добыча для «Выслеживания» львом надзирателя.
    fn best_hunt(&mut self) {
        self.k.hunt_j = -1;
        self.k.hunt_gain = f64::NEG_INFINITY;
        let mask = hunt_targets(self.s);
        if mask == 0 {
            return;
        }
        let m = self.m;
        let tl = m[TIER + u(m[WARDEN])];
        let n = self.p.trap_mornings;
        for c in 1..C {
            if (mask >> c) & 1 == 0 {
                continue;
            }
            let tp = m[TIER + u(c)];
            let v = self.rise_v(tp, n) - self.k.val[u(tp)] + self.rise_v(tl, n) - self.k.val[u(tl)];
            if v > self.k.hunt_gain {
                self.k.hunt_gain = v;
                self.k.hunt_j = c;
            }
        }
    }

    fn pick_door(&mut self) -> i32 {
        self.ensure_chain();
        if self.w[W_AHEAD] != 0.0 || self.w[W_SKIP] != 0.0 {
            self.fill_attr();
        }
        let m = self.m;
        let from = m[WARDEN];
        let mut best = m[DOORS];
        let mut best_v = f64::NEG_INFINITY;
        for i in 0..m[DOORS_N] as usize {
            let c = m[DOORS + i];
            let v = self.door_value(from, c);
            if v > best_v {
                best_v = v;
                best = c;
            }
        }
        self.k.why = self.door_reason(best);
        code(A_PATROL, best)
    }

    fn door_reason(&self, c: i32) -> u8 {
        let m = self.m;
        if c == OFFICE {
            return R_DOOR_OFFICE;
        }
        if m[EVT + u(c)] != 0 {
            return R_DOOR_EVENT;
        }
        if m[LOCK + u(c)] != 1 && m[TIER + u(c)] >= 0 && targets(self.s, c) != 0 {
            let t = m[TIER + u(c)];
            return if self.k.f[u((t * 4 + m[BEAST + u(c)]) * 2)] >= self.k.prism_v * 0.5 { R_DOOR_PRISM } else { R_DOOR_EAT };
        }
        R_DOOR_NEW
    }

    fn pick_ratway(&mut self) -> i32 {
        self.ensure_chain();
        let w = self.m[WARDEN];
        let mut best = -1;
        let mut best_v = f64::NEG_INFINITY;
        for c in 1..C {
            if c == w {
                continue;
            }
            let v = self.cell_value(c);
            if v > best_v {
                best_v = v;
                best = c;
            }
        }
        self.k.why = R_RATWAY;
        code(A_PATROL, best)
    }

    /// Лучший укус едока e: камера и выгода (в best_j / best_gain).
    fn best_target(&mut self, e: i32, chain: usize) {
        self.ensure_chain();
        self.k.best_j = -1;
        self.k.best_gain = f64::NEG_INFINITY;
        let mask = targets(self.s, e);
        for j in 1..C {
            if (mask >> j) & 1 == 0 {
                continue;
            }
            let g = self.bite_gain(e, j, chain);
            if g > self.k.best_gain {
                self.k.best_gain = g;
                self.k.best_j = j;
            }
        }
    }

    fn pick_manage(&mut self) -> i32 {
        let e = self.m[WARDEN];
        let t = self.m[TIER + u(e)];
        self.best_target(e, 0); // и таблицы
        // После замены «Заменить» больше нет — только Пожирание или конец дня.
        let sw = if self.m[USED] == USED_SWAP { f64::NEG_INFINITY } else { self.swap_gain(t, e) };
        self.best_hunt();
        let claws_gain = self.p.claws_gain;
        let k = &mut *self.k;
        if k.hunt_j >= 0 && k.hunt_gain > k.best_gain && k.hunt_gain > sw && k.hunt_gain > 0.0 {
            k.why = R_HUNT;
            return code(A_HUNT, k.hunt_j);
        }
        if k.best_j >= 0 && k.best_gain >= sw && k.best_gain >= 0.0 {
            k.why = if t + 1 >= 5 || (self.m[LOCK + u(k.best_j)] == 1 && t + claws_gain >= 5) { R_DEVOUR_PRISM } else { R_DEVOUR };
            return code(A_DEVOUR, k.best_j);
        }
        if sw > 0.0 {
            k.why = R_SWAP;
            return code(A_SWAP, 0);
        }
        k.why = R_END;
        code(A_END, 0)
    }

    fn pick_chain(&mut self) -> i32 {
        let e = self.m[CHAIN_EATER];
        self.best_target(e, 1);
        if self.k.best_j >= 0 && self.k.best_gain + self.w[W_CHAIN] > 0.0 {
            self.k.why = R_CHAIN;
            return code(A_CHAIN, self.k.best_j);
        }
        self.k.why = R_STOP;
        code(A_STOPCHAIN, 0)
    }

    fn pick_office(&mut self) -> i32 {
        if self.bestow_value() >= self.instigate_value() {
            self.k.why = R_BESTOW;
            code(A_BESTOW, 0)
        } else {
            self.k.why = R_INSTIGATE;
            code(A_INSTIGATE, 0)
        }
    }
}
