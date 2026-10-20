//! Проверка вариантами: Монте-Карло по кандидатам-ходам с общими костями.
//! Перенос `solver/solve.ts` (gorn-tli) и схемы пула этапа 1 (beaccdd).
//!
//! Раунд: для каждого живого кандидата берётся копия позиции, скрытое
//! будущее перебрасывается одинаково для всех (неоткрытые двери, события за
//! «!», дальнейший ГСЧ), ход применяется, забег доигрывается мгновенной
//! стратегией. Кости общие: ГСЧ перезаправляется в начале каждого дня от
//! (зерно раунда, день) и перед каждым решением от (день, фаза, номер
//! решения за день), поэтому после разных ходов будущее кандидатов снова
//! совпадает. Сравнение — по парам раундов: SE разницы с лидером в разы
//! меньше SE каждого среднего.
//!
//! Остановка (не раньше minRounds = 256): каждый соперник позади лидера на
//! ≥ 4 SE либо не может обогнать его больше чем на 0,01 призм. (разница +
//! 2 SE < 0,01). Явно худшие (≥ 5 SE позади, со 128 раундов) отсекаются.
//! Потолок — время, которое даёт вызывающий (`step(ms)`).
//!
//! Раунд — чистая функция (позиция, зерно, номер, ход), поэтому раунды
//! можно играть в пуле rayon в любом порядке; слияние — строго по порядку
//! номеров с проверкой после каждого, как в одном потоке. При том же числе
//! раундов оценки до бита те же, что в одном потоке и в эталоне на TS.
//!
//! Счёт доигрывания — призматические плюс малая добавка за выполненные цели
//! и прогресс цели: при полном хранилище лучшие ходы часто равны точно.

use std::collections::VecDeque;
use std::time::{Duration, Instant as Clock};

use rayon::prelude::*;
use rayon::ThreadPool;

use crate::actions::{apply_code, apply_trusted, legal_codes, CodeBuf};
use crate::model::{Params, CELLS};
use crate::policy::{Instant, Weights};
use crate::rng::{mix_pair as mix, Rng};
use crate::rules::random_event;
use crate::state::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StopRule {
    /// Раундов до первой проверки остановки (256). `u64::MAX` — без остановки.
    pub min_rounds: u64,
    /// Потолок раундов (по умолчанию нет — потолок временем).
    pub max_rounds: u64,
    /// Остановка: соперник позади на столько SE (4).
    pub z_stop: f64,
    /// Отсечение: соперник позади на столько SE (5)…
    pub z_prune: f64,
    /// …начиная с этого раунда (128).
    pub prune_from: u64,
    /// Остановка: соперник не лучше лидера больше чем на столько призм. (0,01).
    pub eps: f64,
}

pub const STOP_DEFAULTS: StopRule =
    StopRule { min_rounds: 256, max_rounds: u64::MAX, z_stop: 4.0, z_prune: 5.0, prune_from: 128, eps: 0.01 };

/// Прежнее правило (`--mode old` в prison-solve-bench).
pub const STOP_OLD: StopRule =
    StopRule { min_rounds: 64, max_rounds: 1500, z_stop: 3.0, z_prune: 4.0, prune_from: 32, eps: 0.1 };

/// Без остановки — только скорость (`--rate`); отсечение как в приложении.
pub const STOP_FREE: StopRule =
    StopRule { min_rounds: u64::MAX, max_rounds: u64::MAX, z_stop: f64::INFINITY, z_prune: 5.0, prune_from: 128, eps: -1.0 };

impl Default for StopRule {
    fn default() -> Self {
        STOP_DEFAULTS
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SolveOptions {
    pub seed: u32,
    pub stop: StopRule,
    /// Виды событий за «!» известны (по умолчанию нет — перебрасываются).
    pub known_events: bool,
}

impl SolveOptions {
    pub fn new(seed: u32) -> Self {
        SolveOptions { seed, stop: STOP_DEFAULTS, known_events: false }
    }
}

/// Сменить показанный ход на другой — только если тот заметно лучше:
/// отрыв ≥ max(2 SE, 0,02 призм.).
pub const SWITCH_SE: f64 = 2.0;
pub const SWITCH_GAP: f64 = 0.02;

#[derive(Clone, Debug, PartialEq)]
pub struct SolveOption {
    pub code: i32,
    /// Среднее призматических к концу забега.
    pub mean: f64,
    /// Отставание от лидера в SE парной разницы (у лидера 0).
    pub gap_se: f64,
    pub rounds: f64,
    pub pruned: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SolveStep {
    pub best: i32,
    pub options: Vec<SolveOption>,
    pub done: bool,
    pub rounds: u64,
}

/// Добавка к счёту за выполненную цель (и её квадрат — за единицу прогресса цели).
const TIE: f64 = 1e-3;

/// Перебросить то, чего игрок не видит: закреплённых без показа узников за
/// дверями, зверя при показанной только редкости, виды событий. Событие в
/// окне (фаза события, камера надзирателя) игрок видит — оно остаётся.
pub fn redeterminize(s: &mut State, r: &mut Rng, known_events: bool, p: &Params) {
    let shown = if s.m[PHASE] == P_EVENT { s.m[WARDEN] } else { -1 };
    for c in 1..CELLS {
        if s.m[LOCK + c] == 1 && s.m[PRE_TIER + c] >= 0 {
            let fl = s.m[FLAGS + c];
            if fl & F_VIS_TIER == 0 {
                s.m[PRE_TIER + c] = -1;
                s.m[PRE_BEAST + c] = -1;
            } else if fl & F_VIS_BEAST == 0 {
                s.m[PRE_BEAST + c] = r.int(4);
            }
        }
        if !known_events && c as i32 != shown && s.m[EVT + c] != 0 {
            let ev = random_event(s, r, p, c as i32);
            if ev >= 0 {
                s.m[EVT + c] = ev + 1;
            }
        }
    }
}

/// Игрок раундов: рабочая копия, ГСЧ и таблицы стратегии (по одному на поток).
pub struct Player {
    work: State,
    rng: Rng,
    det: Rng,
    inst: Instant,
}

impl Default for Player {
    fn default() -> Self {
        Self::new()
    }
}

impl Player {
    pub fn new() -> Self {
        Player { work: State::new(), rng: Rng::day(1, 0), det: Rng::new(1), inst: Instant::new() }
    }

    /// Сыграть раунд n за живых кандидатов (active[i]), счёт — в xs[i].
    #[allow(clippy::too_many_arguments)]
    pub fn play(
        &mut self,
        root: &State,
        codes: &[i32],
        seed0: u32,
        known_events: bool,
        p: &Params,
        w: &Weights,
        n: u64,
        active: &[bool],
        xs: &mut [f64],
    ) {
        let seed = mix(seed0, n as u32);
        for (i, &code) in codes.iter().enumerate() {
            if !active[i] {
                continue;
            }
            self.work = *root;
            self.det.seed(seed);
            redeterminize(&mut self.work, &mut self.det, known_events, p);
            xs[i] = self.playout(seed, code, p, w);
        }
    }

    /// Ход-кандидат и доигрывание с костями, общими по дням.
    fn playout(&mut self, seed: u32, first: i32, p: &Params, w: &Weights) -> f64 {
        let work = &mut self.work;
        let rng = &mut self.rng;
        rng.set_round(seed);
        rng.seed(mix(seed, 0x5bd1_e995));
        if let Err(e) = apply_code(work, first, rng, p) {
            panic!("решатель: {e}");
        }
        // Кости решения зависят от (день, фаза, номер решения за день).
        let mut day = -1;
        let mut k: i32 = 0;
        while work.m[PHASE] != P_END {
            if work.m[DAY] != day {
                day = work.m[DAY];
                k = 0;
            }
            let c = self.inst.code(work, p, w);
            rng.seed(mix(seed, ((day << 12) | (work.m[PHASE] << 8) | k) as u32));
            k += 1;
            apply_trusted(work, c, rng, p);
        }
        work.m[STORAGE] as f64 + TIE * work.m[OBJ_DONE] as f64 + TIE * TIE * work.m[OBJ_PROGRESS] as f64
    }
}

/// Поиск по одной позиции. Тот же кадр снова — продолжаем его (см. [`SolveCache`]).
pub struct Search {
    root: State,
    codes: Vec<i32>,
    k: usize,
    sum: Vec<f64>,
    cross: Vec<f64>,
    count: Vec<f64>,
    active: Vec<bool>,
    xs: Vec<f64>,
    p: Params,
    w: Weights,
    opts: SolveOptions,
    player: Player,
    n: u64,
    done: bool,
    leader: usize,
}

impl Search {
    pub fn new(s: &State, opts: SolveOptions, p: &Params, w: &Weights) -> Search {
        assert!(s.m[PHASE] != P_END, "тюрьма: забег окончен, проверять нечего");
        let mut buf: CodeBuf = [0; 32];
        let k = legal_codes(s, &mut buf);
        Search {
            root: *s,
            codes: buf[..k].to_vec(),
            k,
            sum: vec![0.0; k],
            cross: vec![0.0; k * k],
            count: vec![0.0; k],
            active: vec![true; k],
            xs: vec![0.0; k],
            p: p.clone(),
            w: *w,
            opts,
            player: Player::new(),
            n: 0,
            done: k <= 1,
            leader: 0,
        }
    }

    pub fn codes(&self) -> &[i32] {
        &self.codes
    }

    pub fn rounds(&self) -> u64 {
        self.n
    }

    pub fn is_done(&self) -> bool {
        self.done
    }

    /// Слить раунд: счёт живых кандидатов xs (раунды — строго по порядку номеров).
    fn add(&mut self, xs: &[f64]) {
        let k = self.k;
        for i in 0..k {
            if !self.active[i] {
                continue;
            }
            let x = xs[i];
            self.sum[i] += x;
            self.count[i] += 1.0;
            for j in 0..k {
                if self.active[j] {
                    self.cross[i * k + j] += x * xs[j];
                }
            }
        }
        self.n += 1;
    }

    #[inline]
    fn mean(&self, i: usize) -> f64 {
        if self.count[i] > 0.0 {
            self.sum[i] / self.count[i]
        } else {
            0.0
        }
    }

    /// Ход a (код) против хода b: разница средних и её парная SE (отсечённый — SE 0).
    pub fn compare(&self, a: i32, b: i32) -> (f64, f64) {
        let i = self.codes.iter().position(|&c| c == a);
        let j = self.codes.iter().position(|&c| c == b);
        let (Some(i), Some(j)) = (i, j) else { return (0.0, f64::INFINITY) };
        if self.n == 0 {
            return (0.0, f64::INFINITY);
        }
        if !self.active[i] || !self.active[j] {
            return (self.mean(i) - self.mean(j), 0.0);
        }
        self.gap(i, j)
    }

    /// Ход a заметно лучше хода b: отрыв ≥ max(2 SE, 0,02).
    pub fn clearly_better(&self, a: i32, b: i32) -> bool {
        let (d, se) = self.compare(a, b);
        d >= SWITCH_GAP && d >= SWITCH_SE * se
    }

    /// Отставание кандидата i от лидера L и SE парной разницы.
    fn gap(&self, l: usize, i: usize) -> (f64, f64) {
        let k = self.k;
        let n = self.n as f64;
        let d = self.mean(l) - self.mean(i);
        let sq = (self.cross[l * k + l] - 2.0 * self.cross[l * k + i] + self.cross[i * k + i]) / n;
        let v = sq - d * d;
        (d, (if v > 0.0 { v } else { 0.0 } / n).sqrt())
    }

    /// Лидер, отсечение, проверка остановки.
    fn check(&mut self) {
        let k = self.k;
        let mut l: Option<usize> = None;
        for i in 0..k {
            if self.active[i] && l.is_none_or(|l| self.mean(i) > self.mean(l)) {
                l = Some(i);
            }
        }
        let l = l.unwrap_or(0);
        self.leader = l;
        let st = self.opts.stop;
        if self.n >= st.max_rounds {
            self.done = true;
            return;
        }
        let enough = self.n >= st.min_rounds;
        let can_prune = self.n >= st.prune_from;
        let mut settled = true;
        for i in 0..k {
            if !self.active[i] || i == l {
                continue;
            }
            let (d, se) = self.gap(l, i);
            if can_prune && d > 0.0 && d >= st.z_prune * se {
                self.active[i] = false;
                continue;
            }
            if !(d >= st.z_stop * se && d > 0.0) && !(-d + 2.0 * se < st.eps) {
                settled = false;
            }
        }
        if enough && settled {
            self.done = true;
        }
    }

    /// Считать раунды до остановки или до истечения ms (`f64::INFINITY` — без срока).
    /// С пулом раунды играются пачками в его потоках, сливаются по порядку.
    pub fn step(&mut self, ms: f64, pool: Option<&ThreadPool>) -> SolveStep {
        let t0 = Clock::now();
        let deadline = if ms.is_finite() { Some(t0 + Duration::from_secs_f64(ms.max(0.0) / 1000.0)) } else { None };
        let expired = || deadline.is_some_and(|d| Clock::now() >= d);
        match pool {
            Some(pool) if !self.done && pool.current_num_threads() > 1 => {
                let threads = pool.current_num_threads() as u64;
                while !self.done {
                    let left = self.opts.stop.max_rounds.saturating_sub(self.n);
                    let batch = (threads * 2).min(left);
                    if batch == 0 {
                        break;
                    }
                    let active = self.active.clone();
                    let (root, codes, seed, ke, p, w) =
                        (&self.root, &self.codes[..], self.opts.seed, self.opts.known_events, &self.p, &self.w);
                    let k = self.k;
                    let n0 = self.n;
                    let rounds: Vec<Vec<f64>> = pool.install(|| {
                        (0..batch as usize)
                            .into_par_iter()
                            .with_max_len(1)
                            .map_init(Player::new, |pl, i| {
                                let mut xs = vec![0.0; k];
                                pl.play(root, codes, seed, ke, p, w, n0 + i as u64, &active, &mut xs);
                                xs
                            })
                            .collect()
                    });
                    for xs in &rounds {
                        if self.done {
                            break;
                        }
                        self.add(xs);
                        self.check();
                    }
                    if expired() {
                        break;
                    }
                }
            }
            _ => {
                while !self.done {
                    let mut xs = std::mem::take(&mut self.xs);
                    self.player.play(
                        &self.root,
                        &self.codes,
                        self.opts.seed,
                        self.opts.known_events,
                        &self.p,
                        &self.w,
                        self.n,
                        &self.active,
                        &mut xs,
                    );
                    self.add(&xs);
                    self.xs = xs;
                    self.check();
                    if expired() {
                        break;
                    }
                }
            }
        }
        self.result()
    }

    pub fn result(&self) -> SolveStep {
        let l = self.leader;
        let options = (0..self.k)
            .map(|i| {
                let mut gap_se = 0.0;
                if i != l && self.n > 0 && self.active[i] {
                    let (d, se) = self.gap(l, i);
                    gap_se = if se > 0.0 {
                        d / se
                    } else if d > 0.0 {
                        f64::INFINITY
                    } else {
                        0.0
                    };
                } else if i != l && !self.active[i] {
                    gap_se = f64::INFINITY;
                }
                SolveOption {
                    code: self.codes[i],
                    mean: self.mean(i),
                    gap_se,
                    rounds: self.count[i],
                    pruned: !self.active[i],
                }
            })
            .collect();
        SolveStep { best: self.codes[l], options, done: self.done, rounds: self.n }
    }
}

// ── кэш поисков ─────────────────────────────────────────────────────────────

/// Ключ позиции: два независимых 32-битных хеша содержимого (`stateKey` эталона).
pub fn state_key(s: &State) -> (u32, u32) {
    let mut a: u32 = 0x811c_9dc5;
    let mut b: u32 = 0x0100_0193;
    for &v in &s.m {
        let v = v as u32;
        a = (a ^ v).wrapping_mul(0x0100_0193);
        b = (b ^ v).wrapping_mul(0x5bd1_e995) ^ (b >> 15);
    }
    (a, b)
}

#[derive(Clone, Copy, PartialEq, Debug)]
struct CacheKey {
    state: (u32, u32),
    seed: u32,
    stop: StopRule,
    known_events: bool,
}

/// Последние поиски: тот же кадр (позиция, зерно, правило остановки) —
/// продолжаем накопленный поиск. Параметры и веса — одни на кэш.
pub struct SolveCache {
    size: usize,
    items: VecDeque<(CacheKey, Search)>,
}

impl Default for SolveCache {
    fn default() -> Self {
        Self::new(16)
    }
}

impl SolveCache {
    pub fn new(size: usize) -> Self {
        SolveCache { size: size.max(1), items: VecDeque::new() }
    }

    /// Начать (или продолжить для того же кадра) проверку вариантами.
    pub fn solve(&mut self, s: &State, opts: SolveOptions, p: &Params, w: &Weights) -> &mut Search {
        let key = CacheKey { state: state_key(s), seed: opts.seed, stop: opts.stop, known_events: opts.known_events };
        let item = match self.items.iter().position(|(k, _)| *k == key) {
            Some(i) => self.items.remove(i).unwrap(),
            None => {
                if self.items.len() >= self.size {
                    self.items.pop_front();
                }
                (key, Search::new(s, opts, p, w))
            }
        };
        self.items.push_back(item);
        &mut self.items.back_mut().unwrap().1
    }

    /// Забыть накопленные поиски (новый забег, смена настроек).
    pub fn clear(&mut self) {
        self.items.clear();
    }
}
