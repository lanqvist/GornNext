//! Свой путь подсказок в тюрьме: распознавание → память забега (adapter) →
//! проверка вариантами (solver) → метки (hint). Перенос
//! `src/main/prison/ownAdvisor.ts` (gorn-tli) без потоков и Electron:
//! кадр обрабатывается целиком, ответы складываются в список.
//!
//! Показывается сразу проверенный ход: решатель считает до quick_ms (обычно
//! останавливается сам); не уложился — уходит лучший на этот момент, а
//! решатель досчитывает до budget_ms и меняет стрелку, только если новый
//! лидер заметно лучше показанного (≥ max(2 SE, 0,02), «уточнено»). То же
//! между кадрами одного решения: поиск продолжается, показанный ход держится.
//!
//! С quick_ms = ∞ и потолком раундов (`stop.max_rounds`) советы
//! детерминированы — так их сверяют с эталоном (`gorn-cli verify --replay`).

use std::time::Instant as Clock;

use serde_json::{json, Value};

use crate::actions::{describe, A_TOOL};
use crate::adapter::{Cause, Position, PrisonAdapter, Step};
use crate::hint::{hint, HintContext, HintMark};
use crate::policy::{default_weights, Instant};
use crate::recognize::{default_geometry, read, reroll_box, treasure_boxes, Frame, Geometry, ReadOptions};
use crate::solver::{state_key, SolveCache, SolveOptions, SolveStep, StopRule, STOP_DEFAULTS};
use crate::state::{State, OFFER_KIND, OFFER_TOOL, PHASE, P_PICK};
use crate::view::ScreenView;
use crate::DEFAULT_PARAMS;

/// Столько решатель ищет ответ до первого показа (мс).
pub const QUICK_MS: f64 = 300.0;
/// Выбор орудий: досчёт до этого срока и не меньше PICK_ROUNDS раундов до остановки.
pub const PICK_BUDGET_MS: f64 = 4000.0;
pub const PICK_ROUNDS: u64 = 1500;
/// Столько кадров подряд прежняя геометрия не нашла доску — искать заново.
pub const GEO_MISSES: u32 = 3;

const TREASURE_ASK: &str = "Подарок: нажми «Посмотреть состояние тюрьмы» и вернись — подскажу, какой взять";
const PICK_UNKNOWN: &str = "Не узнал карты выбора — выбери сам";

/// Вид кадра по логу (`PrisonFrameKind | 'manual'`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShotKind {
    Board,
    Instruments,
    Objective,
    Treasure,
    Theme,
    Manual,
}

impl ShotKind {
    pub fn parse(s: &str) -> Option<ShotKind> {
        Some(match s {
            "board" => ShotKind::Board,
            "instruments" => ShotKind::Instruments,
            "objective" => ShotKind::Objective,
            "treasure" => ShotKind::Treasure,
            "theme" => ShotKind::Theme,
            "manual" => ShotKind::Manual,
            _ => return None,
        })
    }
}

/// Кадр от потока экрана (`PrisonShotMessage` без пикселей).
#[derive(Clone, Debug)]
pub struct Shot {
    pub id: u64,
    pub kind: ShotKind,
    pub fresh: bool,
    pub after: Cause,
    pub treasure_view: bool,
    pub rerolled: bool,
    pub treasure_closed: bool,
    /// Окно темы забега: номер озвучки Special_0N из лога.
    pub theme: Option<i32>,
    /// День по логу (cur с единицы, max).
    pub day: Option<(i32, i32)>,
    /// Доска, снятая, когда уже открылось окно подарков.
    pub as_treasure: bool,
    pub width: usize,
    pub height: usize,
    /// Сколько мс решатель проигрывает варианты после первого совета.
    pub budget_ms: f64,
    /// Экран успокоился к снимку; false — снят посреди анимации, None —
    /// неизвестно (ручной снимок, кадр без снимка, старые кадры).
    pub settled: Option<bool>,
    /// События лога при кадре (`logevents::compact`).
    pub events: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Reply {
    Hint { id: u64, phase: &'static str, w: usize, h: usize, text: String, marks: Vec<HintMark>, live: bool },
    Settled { id: u64, live: bool },
    Clear { id: u64 },
    /// Память забега после кадра — сохранить.
    State(Value),
    /// Где доска на кадре — сохранить между запусками.
    Near(Geometry),
}

impl Reply {
    pub fn to_json(&self) -> Value {
        match self {
            Reply::Hint { id, phase, w, h, text, marks, live } => json!({
                "type": "hint", "id": id, "phase": phase, "frame": { "w": w, "h": h }, "text": text,
                "marks": marks.iter().map(HintMark::to_json).collect::<Vec<_>>(), "live": live,
            }),
            Reply::Settled { id, live } => json!({ "type": "settled", "id": id, "live": live }),
            Reply::Clear { id } => json!({ "type": "clear", "id": id }),
            Reply::State(m) => json!({ "type": "state", "id": 0, "model": m }),
            Reply::Near(g) => json!({ "type": "near", "id": 0, "near": {
                "width": g.width, "height": g.height, "s": g.s, "tx": g.tx, "ty": g.ty, "score": g.score,
            }}),
        }
    }
}

/// Зерно поиска от ключа позиции (`seedOf` эталона: FNV по строке ключа).
fn seed_of(key: &str) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for u in key.encode_utf16() {
        h = (h ^ u as u32).wrapping_mul(0x0100_0193);
    }
    h
}

fn radix36(mut x: u32) -> String {
    if x == 0 {
        return "0".into();
    }
    let mut d = vec![];
    while x > 0 {
        d.push(b"0123456789abcdefghijklmnopqrstuvwxyz"[(x % 36) as usize]);
        x /= 36;
    }
    d.reverse();
    String::from_utf8(d).unwrap()
}

/// Ключ позиции строкой, как `stateKey` эталона.
pub fn state_key_str(s: &State) -> String {
    let (a, b) = state_key(s);
    format!("{}.{}", radix36(a), radix36(b))
}

/// Причина хода от стратегии — хвост её подписи, только у обхода, «Отменить» и «Обновить».
fn short_reason(text: &str, c: i32) -> String {
    let kind = c >> 6;
    if kind != crate::actions::A_PATROL && kind != crate::actions::A_STOPCHAIN && kind != crate::actions::A_REROLL {
        return String::new();
    }
    match text.find(": ") {
        Some(i) => text[i + 2..].to_string(),
        None => String::new(),
    }
}

struct Held {
    key: String,
    state: State,
    code: i32,
    step: Step,
    warden: i32,
    day: i32,
}

pub struct OwnAdvisor {
    pub adapter: PrisonAdapter,
    geo: Option<Geometry>,
    geo_misses: u32,
    /// Доска, увиденная сразу после окна сокровищницы: по ней советуется подарок.
    board_behind_treasure: bool,
    held: Option<Held>,
    /// Варианты последней проверки (код, среднее).
    pub last_options: Vec<(i32, f64)>,
    pub quick_ms: f64,
    pub stop: StopRule,
    cache: SolveCache,
    inst: Instant,
    /// Пул решателя (None — один поток).
    pub pool: Option<rayon::ThreadPool>,
    pub replies: Vec<Reply>,
    /// Позиция и зерно последнего поиска (для разбора расхождений).
    pub last_search: Option<(State, u32, StopRule)>,
    /// Ответы сразу, по мере готовности (поток подсказок); иначе — только в replies.
    pub sink: Option<Box<dyn FnMut(&Reply) + Send>>,
    /// Номер последнего кадра снаружи: сменился — расчёт по этому кадру бросить
    /// (`current !== msg.id` эталона). None — не бросать.
    pub latest: Option<std::sync::Arc<std::sync::atomic::AtomicU64>>,
    /// Журнал фактов (этап 7): каждое решение — позиция, оценки решателя по
    /// всем ходам, быстрый и показанный совет. Отдельно от ответов: поток
    /// ответов сверяется с эталоном и не меняется.
    pub facts: Option<Box<dyn FnMut(Value) + Send>>,
    /// Цепочка подарков в адаптере (этап 7); у эталона нет — для сверки выключено.
    pub chain_gifts: bool,
}

impl Default for OwnAdvisor {
    fn default() -> Self {
        Self::new()
    }
}

impl OwnAdvisor {
    pub fn new() -> Self {
        OwnAdvisor {
            adapter: PrisonAdapter::new(None),
            geo: None,
            geo_misses: 0,
            board_behind_treasure: false,
            held: None,
            last_options: vec![],
            quick_ms: QUICK_MS,
            stop: STOP_DEFAULTS,
            cache: SolveCache::default(),
            inst: Instant::new(),
            pool: None,
            replies: vec![],
            last_search: None,
            sink: None,
            latest: None,
            facts: None,
            chain_gifts: false,
        }
    }

    pub fn init(&mut self, state: Option<&Value>, near: Option<Geometry>) {
        self.adapter = PrisonAdapter::new(state);
        self.adapter.chain_gifts = self.chain_gifts;
        self.geo = near;
    }

    fn reply(&mut self, r: Reply) {
        if let Some(sink) = self.sink.as_mut() {
            sink(&r);
        }
        self.replies.push(r);
    }

    /// Кадр уже не последний: ответы по нему не нужны.
    fn stale(&self, id: u64) -> bool {
        self.latest.as_ref().is_some_and(|l| l.load(std::sync::atomic::Ordering::Relaxed) != id)
    }

    fn save_state(&mut self) {
        self.reply(Reply::State(self.adapter.mem.to_json()));
    }

    /// Кадр: frame — RGBA (None — окно подарков без снимка). Ответы — в self.replies.
    pub fn handle(&mut self, msg: &Shot, frame: Option<&Frame>) {
        self.last_options.clear();
        // Начало забега по логу — сброс памяти, даже если карты выбора не читаются.
        if msg.kind == ShotKind::Instruments && msg.fresh {
            self.adapter.start_run();
        }
        // Окно сокровищницы закрылось после «Подтвердить» — подарок взят.
        if msg.treasure_closed && !msg.treasure_view {
            self.adapter.gift_taken();
        }
        // День — по логу (главный источник), экран — запасной.
        self.adapter.log_day(msg.day);
        let (w, h) = (msg.width, msg.height);
        let mut ctx_gifts: Option<Vec<[f64; 4]>> = None;
        let mut ctx_reason = String::new();
        let mut blind_pick = false;
        let plain_geo = |geo: &Option<Geometry>| -> Geometry {
            match geo {
                Some(g) if g.width == w && g.height == h => *g,
                _ => default_geometry(&Frame { width: w, height: h, data: &[] }),
            }
        };
        let mut kind = msg.kind;
        let mut seen = frame.map(|f| self.read(f, msg.kind == ShotKind::Board || msg.kind == ShotKind::Manual));

        if msg.as_treasure && seen.is_some() {
            // Доска снята, а окно подарков уже открыто: обновить по ней доску, совет — по подаркам.
            if let Some(ScreenView::Board(b)) = &seen {
                self.adapter.board_viewed = msg.treasure_view;
                self.adapter.board(b);
                if msg.treasure_closed {
                    self.board_behind_treasure = true;
                }
            }
            kind = ShotKind::Treasure;
            seen = None;
        }
        if let Some(ScreenView::Treasure(g)) = &seen {
            kind = ShotKind::Treasure;
            ctx_gifts = Some(g.clone());
        }

        let board_view;
        let pick_view;
        let mut ctx_board = None;
        let mut ctx_pick = None;
        let mut ctx_reroll = None;
        if kind == ShotKind::Treasure {
            if ctx_gifts.is_none() {
                ctx_gifts = Some(treasure_boxes(&plain_geo(&self.geo)));
            }
            if self.board_behind_treasure {
                self.adapter.treasure(true, Cause::None);
            } else {
                self.adapter.treasure(false, msg.after);
                self.save_state();
                self.reply(Reply::Hint {
                    id: msg.id,
                    phase: "final",
                    w,
                    h,
                    text: TREASURE_ASK.into(),
                    marks: vec![],
                    live: false,
                });
                return;
            }
        } else if kind == ShotKind::Theme {
            // Окно темы забега: тема — по номеру озвучки из лога.
            self.adapter.theme(msg.theme);
            self.save_state();
            self.reply(Reply::Clear { id: msg.id });
            return;
        } else if (kind == ShotKind::Objective && !matches!(seen, Some(ScreenView::Pick(_)))) || seen.is_none() {
            self.reply(Reply::Clear { id: msg.id });
            return;
        } else if matches!(seen, Some(ScreenView::Board(_))) && kind != ShotKind::Instruments {
            let Some(ScreenView::Board(b)) = seen else { unreachable!() };
            // Доска без дверей, ряда решений и цепочки — экран между состояниями: не совет.
            if !msg.treasure_view && b.doors.is_empty() && b.decision.is_none() && b.chain.is_none() {
                if msg.treasure_closed {
                    self.adapter.board_viewed = false;
                    self.adapter.board(&b);
                    self.board_behind_treasure = true;
                    self.save_state();
                }
                self.reply(Reply::Clear { id: msg.id });
                return;
            }
            self.adapter.board_viewed = msg.treasure_view;
            self.adapter.board(&b);
            self.board_behind_treasure = msg.treasure_closed;
            // Просмотр доски из сокровищницы: совет по подарку сразу.
            if msg.treasure_view {
                self.adapter.treasure(true, Cause::None);
            }
            board_view = b;
            ctx_board = Some(&board_view);
        } else if matches!(seen, Some(ScreenView::Pick(_))) && kind != ShotKind::Board {
            let Some(ScreenView::Pick(p)) = seen else { unreachable!() };
            // Неузнанная карта не глушит совет по двум остальным и по «Обновить».
            let known = p.cards.iter().filter(|c| c.is_some()).count();
            if known < 2 {
                self.reply(Reply::Hint { id: msg.id, phase: "final", w, h, text: PICK_UNKNOWN.into(), marks: vec![], live: false });
                return;
            }
            // Хоткей на стартовом выборе (день 0 по логу) — тоже начало забега.
            let fresh = msg.fresh
                || ((msg.kind == ShotKind::Manual || msg.kind == ShotKind::Objective)
                    && msg.day.is_some_and(|d| d.0 == 0)
                    && !p.reward);
            self.adapter.pick(&p, fresh, msg.rerolled, msg.after);
            ctx_reroll = Some(reroll_box(&plain_geo(&self.geo)));
            if known < p.cards.len() {
                ctx_reason = "одну карту не узнал".into();
                blind_pick = true;
            }
            pick_view = p;
            ctx_pick = Some(&pick_view);
        } else {
            self.reply(Reply::Clear { id: msg.id });
            return;
        }
        // Новый забег с выбора орудий сбрасывает память — день из лога ещё раз.
        self.adapter.log_day(msg.day);
        self.save_state();
        let mem = &self.adapter.mem;
        let ctx = HintContext {
            board: ctx_board,
            pick: ctx_pick,
            gifts: ctx_gifts,
            reroll: ctx_reroll,
            warden: Some(mem.warden),
            ratway: mem.step == Step::Ratway,
            cancel_first: mem.step == Step::Chain && mem.first_bite,
            reason: ctx_reason,
        };

        match self.adapter.position() {
            Position::No { fallback, .. } => {
                if let Some(c) = fallback {
                    let hn = hint(c, &ctx);
                    self.reply(Reply::Hint { id: msg.id, phase: "final", w, h, text: hn.text, marks: hn.marks, live: false });
                } else {
                    self.reply(Reply::Clear { id: msg.id });
                }
            }
            Position::Ok { state, mut allowed } => {
                // Выбор цели до первого укуса — то же решение, что ряд решений перед ним.
                let mem = &self.adapter.mem;
                let state = match &self.held {
                    Some(hd)
                        if mem.step == Step::Chain
                            && mem.first_bite
                            && hd.step == Step::Manage
                            && hd.warden == mem.warden
                            && hd.day == mem.day =>
                    {
                        hd.state
                    }
                    _ => state,
                };
                allowed.no_reroll = blind_pick;
                self.advise(msg, &state, &allowed, &ctx, w, h);
            }
        }
    }

    fn read(&mut self, frame: &Frame, expect_board: bool) -> ScreenView {
        let recalibrate = expect_board || self.geo_misses >= GEO_MISSES;
        let (view, g) = read(frame, self.geo.as_ref(), ReadOptions { recalibrate });
        // Промах — «другой экран» при геометрии этого размера.
        let same_size = self.geo.is_some_and(|o| o.width == frame.width && o.height == frame.height);
        self.geo_misses = if view == ScreenView::Other && same_size && !recalibrate { self.geo_misses + 1 } else { 0 };
        if let Some(ng) = g {
            if Some(ng) != self.geo {
                let moved = match self.geo {
                    None => true,
                    Some(o) => {
                        o.width != ng.width
                            || o.height != ng.height
                            || (o.s - ng.s).abs() > 0.002
                            || (o.tx - ng.tx).abs() > 2.0
                            || (o.ty - ng.ty).abs() > 2.0
                    }
                };
                self.geo = Some(ng);
                if moved {
                    self.reply(Reply::Near(ng));
                }
            }
        }
        view
    }

    /// Лучший ход решателя среди видимых на экране.
    fn pick_best(&mut self, res: &SolveStep, allowed: &crate::adapter::Allowed) -> Option<i32> {
        self.last_options = res.options.iter().filter(|o| allowed.allows(o.code)).map(|o| (o.code, o.mean)).collect();
        let mut ranked: Vec<(i32, f64)> = res.options.iter().map(|o| (o.code, o.mean)).collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        if allowed.allows(res.best) {
            return Some(res.best);
        }
        ranked.iter().find(|o| allowed.allows(o.0)).map(|o| o.0)
    }

    fn advise(&mut self, msg: &Shot, state: &State, allowed: &crate::adapter::Allowed, ctx: &HintContext, w: usize, h: usize) {
        let t0 = Clock::now();
        let ms = || t0.elapsed().as_secs_f64() * 1000.0;
        let p = &DEFAULT_PARAMS;
        let wts = default_weights();
        // Зерно — от самой позиции: тот же кадр снова продолжает накопленный поиск.
        let key = state_key_str(state);
        let tool_pick = state.m[PHASE] == P_PICK && state.m[OFFER_KIND] == OFFER_TOOL;
        let stop = if tool_pick && self.stop.min_rounds == STOP_DEFAULTS.min_rounds {
            StopRule { min_rounds: PICK_ROUNDS, ..self.stop }
        } else {
            self.stop
        };
        let opts = SolveOptions { seed: seed_of(&key), stop, known_events: false };
        self.last_search = Some((*state, opts.seed, stop));
        let (step, warden, day) = (self.adapter.mem.step, self.adapter.mem.warden, self.adapter.mem.day);
        let incumbent = match &self.held {
            Some(hd) if hd.key == key && allowed.allows(hd.code) => Some(hd.code),
            _ => None,
        };
        let (instant_code, instant_text) = self.inst.advise(state, p, wts);
        let pool = self.pool.take();
        let quick = self.quick_ms;

        // Первый совет — не позже quick_ms: решатель обычно успевает остановиться сам.
        let search = self.cache.solve(state, opts, p, wts);
        let mut res = search.step(quick.min(50.0), pool.as_ref());
        while !res.done && ms() < quick {
            if self.latest.as_ref().is_some_and(|l| l.load(std::sync::atomic::Ordering::Relaxed) != msg.id) {
                self.pool = pool;
                return;
            }
            res = search.step(50.0, pool.as_ref());
        }
        let Some(leader) = self.pick_best(&res, allowed) else {
            self.pool = pool;
            self.reply(Reply::Clear { id: msg.id });
            return;
        };
        let search = self.cache.solve(state, opts, p, wts);
        // Тот же ход, что уже показан для этого решения, — пока лидер не стал заметно лучше.
        let shown = match incumbent {
            Some(i) if i != leader && !search.clearly_better(leader, i) => i,
            _ => leader,
        };
        self.hold(&key, state, shown, step, warden, day);
        if res.done {
            self.show(msg, shown, "final", res.rounds > 0, "", ctx, instant_code, &instant_text, w, h);
            self.pool = pool;
            self.fact(msg, state, allowed, &res, instant_code, shown, ms());
            return;
        }
        self.show(msg, shown, "instant", false, "", ctx, instant_code, &instant_text, w, h);
        // Досчёт, пока этот кадр на экране. Стрелка меняется, только если новый лидер заметно лучше.
        let until = quick.max(if tool_pick { PICK_BUDGET_MS.max(msg.budget_ms) } else { msg.budget_ms });
        let search = self.cache.solve(state, opts, p, wts);
        while !res.done && ms() < until {
            if self.latest.as_ref().is_some_and(|l| l.load(std::sync::atomic::Ordering::Relaxed) != msg.id) {
                self.pool = pool;
                // Игрок сходил раньше конца досчёта — факт с тем, что успели.
                self.fact(msg, state, allowed, &res, instant_code, shown, ms());
                return;
            }
            res = search.step(100.0, pool.as_ref());
        }
        self.pool = pool;
        if self.stale(msg.id) {
            self.fact(msg, state, allowed, &res, instant_code, shown, ms());
            return;
        }
        let best = self.pick_best(&res, allowed);
        let search = self.cache.solve(state, opts, p, wts);
        let fin = match best {
            Some(b) if b != shown && search.clearly_better(b, shown) => {
                self.hold(&key, state, b, step, warden, day);
                self.show(msg, b, "final", true, "уточнено", ctx, instant_code, &instant_text, w, h);
                b
            }
            _ => {
                self.reply(Reply::Settled { id: msg.id, live: true });
                shown
            }
        };
        self.fact(msg, state, allowed, &res, instant_code, fin, ms());
    }

    /// Факт решения для журнала (этап 7). Ходы, которых нет на экране, тоже
    /// в списке — с пометкой: решатель считает и их.
    #[allow(clippy::too_many_arguments)]
    fn fact(&mut self, msg: &Shot, state: &State, allowed: &crate::adapter::Allowed, res: &SolveStep, instant: i32, advised: i32, ms: f64) {
        if self.facts.is_none() {
            return;
        }
        let mem = &self.adapter.mem;
        let options: Vec<Value> = res
            .options
            .iter()
            .map(|o| {
                json!({ "code": o.code, "mean": o.mean, "gapSe": o.gap_se, "rounds": o.rounds, "pruned": o.pruned, "shown": allowed.allows(o.code) })
            })
            .collect();
        let v = json!({
            "shot": msg.id,
            "fresh": msg.kind == ShotKind::Instruments && msg.fresh,
            "day": mem.day,
            "dayLimit": mem.day_limit,
            "warden": mem.warden,
            "step": format!("{:?}", mem.step),
            "storage": state.m[crate::state::STORAGE],
            "objective": mem.objective,
            "theme": mem.start,
            "tools": mem.tools,
            "state": state.m.to_vec(),
            "key": state_key_str(state),
            "instant": instant,
            "advised": advised,
            "best": res.best,
            "done": res.done,
            "rounds": res.rounds,
            "ms": ms,
            "options": options,
            "settled": msg.settled,
            "events": msg.events,
        });
        if let Some(f) = self.facts.as_mut() {
            f(v);
        }
    }

    fn hold(&mut self, key: &str, state: &State, code: i32, step: Step, warden: i32, day: i32) {
        let st = match &self.held {
            Some(hd) if hd.key == key => hd.state,
            _ => *state,
        };
        self.held = Some(Held { key: key.to_string(), state: st, code, step, warden, day });
    }

    #[allow(clippy::too_many_arguments)]
    fn show(
        &mut self,
        msg: &Shot,
        c: i32,
        phase: &'static str,
        live: bool,
        note: &str,
        ctx: &HintContext,
        instant_code: i32,
        instant_text: &str,
        w: usize,
        h: usize,
    ) {
        if c >> 6 == A_TOOL {
            self.adapter.advised((c & 63) as u32);
            self.save_state();
        }
        if self.chain_gifts && c >> 6 == crate::actions::A_GIFT {
            if let Some((state, _, _)) = &self.last_search {
                self.adapter.gift_prediction = gift_prisms(state, c);
            }
        }
        let reason = if c == instant_code { short_reason(instant_text, c) } else { String::new() };
        let parts: Vec<&str> = [ctx.reason.as_str(), reason.as_str(), note].into_iter().filter(|s| !s.is_empty()).collect();
        let ctx2 = HintContext { reason: parts.join(" · "), ..ctx.clone() };
        let hn = hint(c, &ctx2);
        self.reply(Reply::Hint { id: msg.id, phase, w, h, text: hn.text, marks: hn.marks, live });
    }
}

/// Сколько новых Призматических даст подарок c в позиции s: среднее по
/// розыгрышам (скрытое — заново, как у решателя), округлённое.
fn gift_prisms(s: &State, c: i32) -> i32 {
    use crate::state::STORAGE;
    let p = &DEFAULT_PARAMS;
    const N: u32 = 64;
    let mut sum = 0i64;
    for k in 0..N {
        let mut r = crate::rng::Rng::new(crate::rng::mix_pair(k * 7919 + 11, c as u32));
        let mut w = *s;
        crate::solver::redeterminize(&mut w, &mut r, false, p);
        let before = w.m[STORAGE];
        if crate::actions::apply_code(&mut w, c, &mut r, p).is_ok() {
            sum += (w.m[STORAGE] - before) as i64;
        }
    }
    (sum as f64 / N as f64).round() as i32
}

/// Ход из текста подсказки — общий ключ для прежнего и своего пути (`moveKey` эталона).
pub fn move_key(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    let t = text.split(" · ").next().unwrap_or("").trim();
    let cell = |s: &str| if s == "допросную" { "0".to_string() } else { s.replace('№', "") };
    let num_after = |prefix: &str| -> Option<String> {
        let rest = t.strip_prefix(prefix)?;
        if let Some(r) = rest.strip_prefix("допросную") {
            let _ = r;
            return Some("допросную".into());
        }
        let r = rest.strip_prefix('№')?;
        let d: String = r.chars().take_while(|c| c.is_ascii_digit()).collect();
        (!d.is_empty()).then(|| format!("№{d}"))
    };
    if let Some(c) = num_after("Иди в ") {
        return format!("обход {}", cell(&c));
    }
    if let Some(c) = num_after("Выбери ").filter(|c| c.starts_with('№')) {
        return format!("тропа {}", cell(&c));
    }
    if let Some(c) = num_after("Пожирание: съесть ").filter(|c| c.starts_with('№')) {
        return format!("съесть {}", cell(&c));
    }
    if let Some(c) = num_after("Цепочка: съесть ").filter(|c| c.starts_with('№')) {
        return format!("съесть {}", cell(&c));
    }
    if let Some(c) = num_after("Метка добычи: ").filter(|c| c.starts_with('№')) {
        return format!("метка {}", cell(&c));
    }
    if t.starts_with("Остановить цепочку") || t.starts_with("Отменить") {
        return "отменить".into();
    }
    if t.starts_with("Заменить") {
        return "заменить".into();
    }
    if t.starts_with("Завершить день") {
        return "конец дня".into();
    }
    if t.starts_with("Наградить") {
        return "наградить".into();
    }
    if t.starts_with("Подстрекнуть") {
        return "подстрекнуть".into();
    }
    if t.starts_with("Обновить") {
        return "обновить".into();
    }
    if t.contains("нажми «Посмотреть состояние тюрьмы»") {
        return "показать доску".into();
    }
    if let Some(rest) = t.strip_prefix("Взять: ") {
        if !rest.is_empty() {
            let words: Vec<&str> = rest.split(' ').take(2).collect();
            return format!("взять {}", words.join(" ").to_lowercase());
        }
    }
    t.to_string()
}

/// Последняя показанная подсказка (окончательная, иначе мгновенная).
pub fn last_hint(replies: &[Reply]) -> String {
    let mut text = String::new();
    for r in replies {
        if let Reply::Hint { text: t, .. } = r {
            text = t.clone();
        }
    }
    text
}

/// Для отладки: ход текстом.
pub fn describe_code(c: i32) -> String {
    describe(c)
}
