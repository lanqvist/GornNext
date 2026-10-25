//! Адаптер: экраны тюрьмы (ScreenView) и память забега между кадрами →
//! позиция движка для совета. Перенос `adapter.ts` (gorn-tli).
//!
//! Один кадр видит не всё: орудия (на экране цепочки панель скрыта), цель,
//! сколько дней в забеге, хранилище, что было на доске до затемнения
//! цепочки, сколько подарков подряд. Это хранит память забега — обычный
//! JSON того же вида, что у эталона (сохранённое состояние Горна читается).
//!
//! Чего игрок не видит, адаптер не выдумывает: узники за дверями не
//! закреплены (кроме показанной редкости), вид события за «!» неизвестен —
//! решатель перебрасывает это на каждом раунде.

use std::collections::HashMap;

use serde_json::{json, Map, Value};

use crate::actions::{legal_codes, CodeBuf, A_DEVOUR, A_END, A_HUNT, A_SWAP};
use crate::model::{
    event, start_event, tool, tool_index, Params, CELLS, DEFAULT_PARAMS, GIFT_IDS, LION, OBJECTIVE_IDS, OFFICE,
    START_EVENT_IDS, SUPREME, TOOL_IDS,
};
use crate::rng::Rng;
use crate::rules::{new_run, office_rounds, targets, RunSetup};
use crate::state::*;
use crate::view::{BoardView, CellKind, CellView, Decision, PickView};

/// Что ждём от игрока по последнему понятому экрану.
/// hunt — «Выберите цель» после «Выслеживания»: добыча для льва надзирателя.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Patrol,
    Manage,
    Office,
    Ratway,
    Hunt,
    Chain,
    Pick,
    Gift,
    Wait,
}

const STEPS: [(Step, &str); 9] = [
    (Step::Patrol, "patrol"),
    (Step::Manage, "manage"),
    (Step::Office, "office"),
    (Step::Ratway, "ratway"),
    (Step::Hunt, "hunt"),
    (Step::Chain, "chain"),
    (Step::Pick, "pick"),
    (Step::Gift, "gift"),
    (Step::Wait, "wait"),
];

impl Step {
    pub fn name(self) -> &'static str {
        STEPS.iter().find(|(s, _)| *s == self).unwrap().1
    }
    pub fn parse(s: &str) -> Option<Step> {
        STEPS.iter().find(|(_, n)| *n == s).map(|(s, _)| *s)
    }
    fn is_choice(self) -> bool {
        matches!(self, Step::Pick | Step::Gift | Step::Wait)
    }
}

/// Что привело к кадру (`ShotCause` эталона).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Cause {
    #[default]
    None,
    Swap,
    Consume,
    Morning,
}

impl Cause {
    pub fn name(self) -> &'static str {
        match self {
            Cause::None => "",
            Cause::Swap => "swap",
            Cause::Consume => "consume",
            Cause::Morning => "morning",
        }
    }
    pub fn parse(s: &str) -> Cause {
        match s {
            "swap" => Cause::Swap,
            "consume" => Cause::Consume,
            "morning" => Cause::Morning,
            _ => Cause::None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellK {
    Office,
    Locked,
    Empty,
    Inmate,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct CellMemory {
    pub k: Option<CellK>,
    /// Зверь 0…3.
    pub b: Option<i32>,
    pub t: Option<i32>,
    /// «!» — событие.
    pub ev: bool,
    pub nest: bool,
    /// День, когда гнездо увидели впервые.
    pub nest_day: Option<i32>,
    /// Показанная редкость за дверью.
    pub shown: Option<i32>,
    /// Метка «Царапины» («Грязные когти»).
    pub scratch: bool,
    /// Метка «Бивень» (Щипцы) на камере с узником.
    pub tusk: bool,
}

impl CellMemory {
    fn of(k: CellK) -> Self {
        CellMemory { k: Some(k), ..Default::default() }
    }
    fn locked() -> Self {
        Self::of(CellK::Locked)
    }
    fn inmate(b: i32, t: i32) -> Self {
        CellMemory { k: Some(CellK::Inmate), b: Some(b), t: Some(t), ..Default::default() }
    }
    fn kind(&self) -> CellK {
        self.k.unwrap_or(CellK::Locked)
    }

    fn to_json(&self) -> Value {
        let mut m = Map::new();
        m.insert(
            "k".into(),
            json!(match self.kind() {
                CellK::Office => "office",
                CellK::Locked => "locked",
                CellK::Empty => "empty",
                CellK::Inmate => "inmate",
            }),
        );
        if let Some(b) = self.b {
            m.insert("b".into(), json!(b));
        }
        if let Some(t) = self.t {
            m.insert("t".into(), json!(t));
        }
        if self.ev {
            m.insert("ev".into(), json!(true));
        }
        if self.nest {
            m.insert("nest".into(), json!(true));
        }
        if let Some(d) = self.nest_day {
            m.insert("nestDay".into(), json!(d));
        }
        if let Some(s) = self.shown {
            m.insert("shown".into(), json!(s));
        }
        if self.scratch {
            m.insert("scratch".into(), json!(true));
        }
        if self.tusk {
            m.insert("tusk".into(), json!(true));
        }
        Value::Object(m)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecisionMem {
    pub consume: bool,
    pub replace: bool,
    pub end: bool,
    pub hunt: bool,
    /// Ряд вернулся после «Отменить» посреди цепочки: Пожирание в этот день уже было.
    pub after_chain: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mark {
    pub cell: i32,
    pub lion: i32,
    pub day: i32,
}

/// Память забега (`RunMemory` эталона). Орудия — номера (tool::*), в порядке появления.
#[derive(Clone, Debug, PartialEq)]
pub struct RunMemory {
    pub cells: Vec<CellMemory>,
    pub warden: i32,
    /// День с нуля и дней в забеге.
    pub day: i32,
    pub day_limit: i32,
    pub tools: Vec<u32>,
    /// Номер цели (objective::*).
    pub objective: Option<i32>,
    /// Цель забега выполнена (был выбор орудия в награду).
    pub objective_done: bool,
    /// Тема (start_event::*).
    pub start: i32,
    pub storage: i32,
    pub step: Step,
    pub doors: Vec<i32>,
    /// Карточки ряда решений; None — ряда не было.
    pub decision: Option<DecisionMem>,
    /// Подсвеченные цели на экране цепочки.
    pub chain_targets: Vec<i32>,
    /// Экран «Выберите узника для пожирания» до первого укуса.
    pub first_bite: bool,
    pub offer: Vec<u32>,
    pub rerolls: i32,
    /// Сколько подарков подряд ещё выбрать.
    pub gifts: i32,
    /// Выбор орудия в начале забега: доски за ним ещё не было.
    pub run_start: bool,
    /// Тему забега увидели, а доски нового забега ещё не было.
    pub theme_fresh: bool,
    /// Последний выбор орудий, ещё не подтверждённый доской.
    pub pick_offer: Vec<u32>,
    pub pick_advised: Option<u32>,
    /// Орудия, взятые «предположительно»: с картами своего выбора.
    pub assumed: Vec<(u32, Vec<u32>)>,
    /// Куда забег вернётся после выбора.
    pub resume: Step,
    pub resume_after: Cause,
    /// Хранилище к началу дня.
    pub day_storage: i32,
    /// Нижняя граница хранилища.
    pub storage_floor: i32,
    /// Орудия, которые панель показала один раз, а выбора с ними не было.
    pub tool_sightings: Vec<u32>,
    /// Метки «Добыча».
    pub marks: Vec<Mark>,
    /// Экран выбора добычи: лев надзирателя и день.
    pub hunt_at: Option<(i32, i32)>,
}

pub fn blank_memory() -> RunMemory {
    RunMemory {
        cells: (0..CELLS).map(|c| if c as i32 == OFFICE { CellMemory::of(CellK::Office) } else { CellMemory::locked() }).collect(),
        warden: OFFICE,
        day: 0,
        day_limit: DEFAULT_PARAMS.days,
        tools: vec![],
        objective: None,
        objective_done: false,
        start: start_event::NONE,
        storage: 0,
        step: Step::Patrol,
        doors: vec![],
        decision: None,
        chain_targets: vec![],
        first_bite: false,
        offer: vec![],
        rerolls: 0,
        gifts: 0,
        run_start: false,
        theme_fresh: false,
        pick_offer: vec![],
        pick_advised: None,
        assumed: vec![],
        resume: Step::Patrol,
        resume_after: Cause::None,
        day_storage: 0,
        storage_floor: 0,
        tool_sightings: vec![],
        marks: vec![],
        hunt_at: None,
    }
}

fn tool_ids(v: &[u32]) -> Value {
    json!(v.iter().map(|&t| TOOL_IDS[t as usize]).collect::<Vec<_>>())
}

impl RunMemory {
    /// JSON как у эталона (необязательные поля — только заданные).
    pub fn to_json(&self) -> Value {
        let mut m = Map::new();
        m.insert("v".into(), json!(1));
        m.insert("cells".into(), Value::Array(self.cells.iter().map(CellMemory::to_json).collect()));
        m.insert("warden".into(), json!(self.warden));
        m.insert("day".into(), json!(self.day));
        m.insert("dayLimit".into(), json!(self.day_limit));
        m.insert("tools".into(), tool_ids(&self.tools));
        m.insert("objective".into(), json!(self.objective.map(|o| OBJECTIVE_IDS[o as usize])));
        m.insert("objectiveDone".into(), json!(self.objective_done));
        m.insert("start".into(), json!(START_EVENT_IDS[self.start as usize]));
        m.insert("storage".into(), json!(self.storage));
        m.insert("step".into(), json!(self.step.name()));
        m.insert("doors".into(), json!(self.doors));
        m.insert(
            "decision".into(),
            match self.decision {
                None => Value::Null,
                Some(d) => {
                    let mut o = Map::new();
                    o.insert("consume".into(), json!(d.consume));
                    o.insert("replace".into(), json!(d.replace));
                    o.insert("end".into(), json!(d.end));
                    if d.hunt {
                        o.insert("hunt".into(), json!(true));
                    }
                    if d.after_chain {
                        o.insert("afterChain".into(), json!(true));
                    }
                    Value::Object(o)
                }
            },
        );
        m.insert("chainTargets".into(), json!(self.chain_targets));
        m.insert("firstBite".into(), json!(self.first_bite));
        m.insert("offer".into(), tool_ids(&self.offer));
        m.insert("rerolls".into(), json!(self.rerolls));
        m.insert("gifts".into(), json!(self.gifts));
        m.insert("runStart".into(), json!(self.run_start));
        m.insert("themeFresh".into(), json!(self.theme_fresh));
        m.insert("pickOffer".into(), tool_ids(&self.pick_offer));
        m.insert("pickAdvised".into(), json!(self.pick_advised.map(|t| TOOL_IDS[t as usize])));
        m.insert(
            "assumed".into(),
            Value::Array(self.assumed.iter().map(|(t, o)| json!({ "tool": TOOL_IDS[*t as usize], "offer": tool_ids(o) })).collect()),
        );
        m.insert("resume".into(), json!(self.resume.name()));
        m.insert("resumeAfter".into(), json!(self.resume_after.name()));
        m.insert("dayStorage".into(), json!(self.day_storage));
        m.insert("storageFloor".into(), json!(self.storage_floor));
        m.insert("toolSightings".into(), tool_ids(&self.tool_sightings));
        m.insert(
            "marks".into(),
            Value::Array(self.marks.iter().map(|k| json!({ "cell": k.cell, "lion": k.lion, "day": k.day })).collect()),
        );
        m.insert("huntAt".into(), self.hunt_at.map(|(lion, day)| json!({ "lion": lion, "day": day })).unwrap_or(Value::Null));
        Value::Object(m)
    }
}

fn int_in(x: &Value, lo: i64, hi: i64) -> Option<i32> {
    let f = x.as_f64()?;
    (f.fract() == 0.0 && f >= lo as f64 && f <= hi as f64).then_some(f as i32)
}

fn truthy(x: &Value) -> bool {
    match x {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

fn tool_list(x: &Value) -> Vec<u32> {
    x.as_array().map(|a| a.iter().filter_map(|t| t.as_str().and_then(tool_index)).collect()).unwrap_or_default()
}

/// Память из сохранения (`loadMemory`); испорченное — чистый забег.
pub fn load_memory(x: &Value) -> RunMemory {
    let base = blank_memory();
    let Some(m) = x.as_object() else { return base };
    if m.get("v").and_then(Value::as_f64) != Some(1.0) {
        return base;
    }
    let Some(cells) = m.get("cells").and_then(Value::as_array).filter(|c| c.len() == CELLS) else { return base };
    let g = |k: &str| m.get(k).unwrap_or(&Value::Null);
    let cells = cells
        .iter()
        .enumerate()
        .map(|(i, c)| {
            if i as i32 == OFFICE {
                return CellMemory::of(CellK::Office);
            }
            let Some(o) = c.as_object() else { return CellMemory::locked() };
            let f = |k: &str| o.get(k).unwrap_or(&Value::Null);
            let k = f("k").as_str();
            if k == Some("inmate") {
                if let (Some(b), Some(t)) = (int_in(f("b"), 0, 3), int_in(f("t"), 0, SUPREME as i64)) {
                    let mut c = CellMemory::inmate(b, t);
                    c.ev = truthy(f("ev"));
                    c.tusk = truthy(f("tusk"));
                    return c;
                }
            }
            if k == Some("empty") {
                return CellMemory::of(CellK::Empty);
            }
            let mut out = CellMemory::locked();
            out.ev = truthy(f("ev"));
            out.nest = truthy(f("nest"));
            if out.nest {
                out.nest_day = int_in(f("nestDay"), 0, 29);
            }
            out.shown = int_in(f("shown"), 0, SUPREME as i64);
            if out.shown.is_some() && truthy(f("scratch")) {
                out.scratch = true;
            }
            out
        })
        .collect();
    let step = |v: &Value| v.as_str().and_then(Step::parse);
    let cell_list = |v: &Value| -> Vec<i32> {
        v.as_array().map(|a| a.iter().filter_map(|n| int_in(n, 0, CELLS as i64 - 1)).collect()).unwrap_or_default()
    };
    let decision = g("decision").as_object().map(|d| DecisionMem {
        consume: truthy(d.get("consume").unwrap_or(&Value::Null)),
        replace: truthy(d.get("replace").unwrap_or(&Value::Null)),
        end: truthy(d.get("end").unwrap_or(&Value::Null)),
        hunt: truthy(d.get("hunt").unwrap_or(&Value::Null)),
        after_chain: truthy(d.get("afterChain").unwrap_or(&Value::Null)),
    });
    let assumed = g("assumed")
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| {
                    let t = x.get("tool")?.as_str().and_then(tool_index)?;
                    let o = x.get("offer")?.as_array()?;
                    Some((t, o.iter().filter_map(|t| t.as_str().and_then(tool_index)).collect()))
                })
                .collect()
        })
        .unwrap_or_default();
    let marks = g("marks")
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|k| {
                    Some(Mark {
                        cell: int_in(k.get("cell")?, 1, CELLS as i64 - 1)?,
                        lion: int_in(k.get("lion")?, -1, CELLS as i64 - 1)?,
                        day: int_in(k.get("day")?, -1, 29)?,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let hunt_at = g("huntAt").as_object().and_then(|h| {
        Some((int_in(h.get("lion")?, 0, CELLS as i64 - 1)?, int_in(h.get("day")?, 0, 29)?))
    });
    RunMemory {
        cells,
        warden: int_in(g("warden"), 0, CELLS as i64 - 1).unwrap_or(OFFICE),
        day_limit: if g("dayLimit").as_f64() == Some(30.0) { 30 } else { DEFAULT_PARAMS.days },
        day: int_in(g("day"), 0, 29).unwrap_or(0),
        tools: tool_list(g("tools")),
        objective: g("objective").as_str().and_then(|o| OBJECTIVE_IDS.iter().position(|&x| x == o)).map(|i| i as i32),
        objective_done: truthy(g("objectiveDone")),
        start: g("start").as_str().and_then(|o| START_EVENT_IDS.iter().position(|&x| x == o)).map(|i| i as i32).unwrap_or(0),
        storage: int_in(g("storage"), 0, 99).unwrap_or(0),
        step: step(g("step")).unwrap_or(Step::Patrol),
        doors: cell_list(g("doors")),
        decision,
        chain_targets: cell_list(g("chainTargets")),
        first_bite: truthy(g("firstBite")),
        offer: tool_list(g("offer")),
        rerolls: int_in(g("rerolls"), 0, 9).unwrap_or(0),
        gifts: int_in(g("gifts"), 0, 30).unwrap_or(0),
        run_start: truthy(g("runStart")),
        theme_fresh: truthy(g("themeFresh")),
        pick_offer: tool_list(g("pickOffer")),
        pick_advised: g("pickAdvised").as_str().and_then(tool_index),
        assumed,
        resume: step(g("resume")).unwrap_or(Step::Patrol),
        resume_after: g("resumeAfter").as_str().map(Cause::parse).unwrap_or_default(),
        day_storage: int_in(g("dayStorage"), 0, 99).unwrap_or(0),
        storage_floor: int_in(g("storageFloor"), 0, 99).unwrap_or(0),
        tool_sightings: tool_list(g("toolSightings")),
        marks,
        hunt_at,
    }
}

/// Цель с панели распознавания → цель движка.
fn objective_from_view(s: &str) -> Option<i32> {
    let id = match s {
        "beastking" | "apex" => "apex",
        "jungle" => "jungle",
        "doors" => "doors",
        "walk" => "walk",
        "currents" => "currents",
        _ => return None,
    };
    OBJECTIVE_IDS.iter().position(|&x| x == id).map(|i| i as i32)
}

/// Тема забега по номеру озвучки окна темы («Play_VO_Story_S16_Special_0N»).
pub fn theme_by_voice(voice: i32) -> Option<i32> {
    match voice {
        1 => Some(start_event::TYRANNY),
        2 => Some(start_event::ABUNDANCE),
        3 => Some(start_event::IRONLAW),
        4 => Some(start_event::CAGEFIGHT),
        _ => None,
    }
}

/// «Бивень» на камере с узником: виден на портрете или был у прежнего узника —
/// метка сходит, только когда камера пустеет, и переживает Замену [клиент,
/// глоссарий 16000113]; на тёмных кадрах значок читается хуже.
fn tusk_kept(was: &CellMemory, seen: &CellView) -> bool {
    seen.tusk == Some(true) || (was.kind() == CellK::Inmate && was.tusk)
}

fn beast_no(b: Option<u8>) -> i32 {
    b.map(|b| b as i32).unwrap_or(0)
}

/// Какие ходы видны на экране (карточки ряда решений, подсвеченные цели).
#[derive(Clone, Debug)]
pub struct Allowed {
    targets_shown: Option<Vec<i32>>,
    hunting: bool,
    decision: Option<DecisionMem>,
    /// Не советовать «Обновить» (выбор с неузнанной картой).
    pub no_reroll: bool,
}

impl Allowed {
    pub fn allows(&self, code: i32) -> bool {
        let kind = code >> 6;
        if self.no_reroll && kind == crate::actions::A_REROLL {
            return false;
        }
        if let Some(t) = &self.targets_shown {
            if kind == A_DEVOUR {
                return t.contains(&(code & 63));
            }
        }
        if self.hunting {
            return kind == A_HUNT;
        }
        let Some(d) = self.decision else { return true };
        match kind {
            A_HUNT => d.hunt,
            A_DEVOUR => d.consume,
            A_SWAP => d.replace,
            A_END => true,
            _ => true,
        }
    }
}

pub enum Position {
    Ok { state: State, allowed: Allowed },
    /// Совета нет; fallback — ход без расчёта (код), когда модель не сходится с экраном.
    No { problem: &'static str, fallback: Option<i32> },
}

pub struct PrisonAdapter {
    pub mem: RunMemory,
    /// День этого кадра взят из лога — экранный не нужен.
    day_from_log: bool,
    /// День только начался (по логу), дверей ещё не было.
    fresh_day: bool,
    /// Сколько кадров подряд целиком узнанная панель не показывает орудие из памяти.
    tool_misses: HashMap<u32, i32>,
    p: Params,
    /// Орудия, у которых есть эталон иконки на панели.
    panel_known: Vec<u32>,
    /// Цепочка подарков (этап 7): каждая новая Призматическая — ещё один выбор,
    /// а счётчика выборов на экране нет. Остаток считается по предсказанию
    /// выбранного подарка и выправляется по призмам на доске за окном.
    /// У эталона этого нет — для сверки с ним выключено.
    pub chain_gifts: bool,
    /// День цепочки: на новый день прежний остаток не переносится.
    chain_day: i32,
    /// Призмы, ожидаемые на следующем просмотре доски в цепочке
    /// (None — доску в этой цепочке ещё не смотрели).
    chain_storage: Option<i32>,
    /// Хранилище до окна подарков (начало цепочки) и сколько подарков в ней
    /// взято: каждая новая Призматическая — выбор, так что остаток не меньше
    /// (призмы на доске − до окна − взятые). Две призмы разом (капкан, цепочка
    /// Пожирания) — два выбора [журнал фактов 22.10: 10 из 10, остаток был 1].
    chain_base: Option<i32>,
    chain_taken: i32,
    /// Сколько новых Призматических даст показанный подарок (предсказание движка).
    pub gift_prediction: i32,
    /// Последняя доска снята по кнопке «посмотреть доску» из окна подарков.
    /// Доска до окна или сразу после «Подтвердить» — со старыми призмами
    /// (анимация): по ней остаток только растёт.
    pub board_viewed: bool,
}

impl PrisonAdapter {
    pub fn new(saved: Option<&Value>) -> Self {
        let t = crate::recognize::templates();
        let panel_known =
            crate::recognize::panels::panel_tool_ids(&t.screens).iter().filter_map(|id| tool_index(id)).collect();
        PrisonAdapter {
            mem: saved.map(load_memory).unwrap_or_else(blank_memory),
            day_from_log: false,
            fresh_day: false,
            tool_misses: HashMap::new(),
            p: DEFAULT_PARAMS,
            panel_known,
            chain_gifts: false,
            chain_day: -1,
            chain_storage: None,
            chain_base: None,
            chain_taken: 0,
            gift_prediction: 0,
            board_viewed: false,
        }
    }

    fn known(&self, t: u32) -> bool {
        self.panel_known.contains(&t)
    }

    /// Доска со строкой дней, панелями и рядом карточек (или экран цепочки).
    pub fn board(&mut self, v: &BoardView) {
        let day_before = self.mem.day;
        // День — по логу, если он есть.
        if let Some(left) = v.days_left {
            if !self.day_from_log {
                let m = &mut self.mem;
                if left > m.day_limit {
                    // Больше 20 дней осталось — забег «Железного закона».
                    m.day_limit = self.p.iron_law_days;
                    if m.start == start_event::NONE {
                        m.start = start_event::IRONLAW;
                    }
                }
                m.day = 0.max((m.day_limit - 1).min(m.day_limit - left - 1));
            }
        }
        self.panel_tools(v);
        let m = &mut self.mem;
        // Первая доска забега — тоже утро: выбор орудий («день 0») и первый
        // день оба дают день 0 [журнал фактов #43: царапина Когтей на первой доске].
        let first_board = m.run_start;
        if v.chromatic >= 0 {
            m.storage = v.chromatic.max(m.storage_floor);
        }
        if let Some(o) = v.objective.as_deref().and_then(objective_from_view) {
            m.objective = Some(o);
        }
        m.run_start = false;
        m.theme_fresh = false;
        m.offer = vec![];
        // Утренняя доска: день только начался, на ней ещё ничего не нажато.
        let morning = self.fresh_day || m.day > day_before || first_board;

        let ratway = v.decision == Some(Decision::Ratway);
        if !v.doors.is_empty() || v.chain.is_some() || (v.decision.is_some() && !ratway) {
            self.fresh_day = false;
        }
        if v.chain.is_some() {
            self.chain_board(v);
            return;
        }
        let m = &mut self.mem;
        m.first_bite = false;
        // Выбор добычи «Выслеживания»: редкости с неё не перечитываем.
        let stale = self.fresh_day && ratway;
        let hunt_screen = !stale
            && ratway
            && (m.step == Step::Hunt || (m.step == Step::Manage && m.decision.is_some_and(|d| d.hunt)));
        let claws = m.tools.contains(&tool::CLAWS);
        for (n, c) in v.cells.iter().enumerate() {
            if n as i32 == OFFICE || hunt_screen {
                continue;
            }
            let was = m.cells[n].clone();
            match c.kind {
                CellKind::Inmate => {
                    // Редкость не нарисована — тот же узник сохраняет редкость из памяти.
                    let b = beast_no(c.beast);
                    let keep = c.tier_unknown == Some(true) && was.kind() == CellK::Inmate && was.b == Some(b);
                    let mut cell =
                        CellMemory::inmate(b, if keep { was.t.unwrap() } else { (c.tier.unwrap_or(0) as i32).min(SUPREME) });
                    cell.ev = c.event == Some(true);
                    cell.tusk = tusk_kept(&was, c);
                    m.cells[n] = cell;
                }
                CellKind::Locked => {
                    let mut cell = CellMemory::locked();
                    cell.ev = c.event == Some(true);
                    if c.nest == Some(true) {
                        cell.nest = true;
                        cell.nest_day = Some(if was.kind() == CellK::Locked && was.nest && was.nest_day.is_some() {
                            was.nest_day.unwrap()
                        } else {
                            m.day
                        });
                    }
                    let shown = c.shown_tier.map(|x| x as i32).or(if was.kind() == CellK::Locked { was.shown } else { None });
                    if let Some(shown) = shown {
                        cell.shown = Some(shown.min(SUPREME));
                        // «Царапины» Когти ставят утром: редкость, впервые показанная на утренней доске.
                        let first = was.kind() != CellK::Locked || was.shown.is_none();
                        if was.kind() == CellK::Locked && was.scratch {
                            cell.scratch = true;
                        } else if first && morning && claws {
                            cell.scratch = true;
                        }
                    }
                    m.cells[n] = cell;
                }
                _ => m.cells[n] = CellMemory::of(CellK::Empty),
            }
        }
        if let Some(w) = v.warden {
            m.warden = w as i32;
        }
        m.chain_targets = vec![];
        if !hunt_screen {
            self.read_marks(v);
        }
        let m = &mut self.mem;
        if hunt_screen {
            if let Some(w) = v.warden {
                m.hunt_at = Some((w as i32, m.day));
            }
            // «Выберите цель» сразу после ряда с «Выслеживанием» — добыча, не крысиная тропа.
            m.step = Step::Hunt;
            m.doors = vec![];
        } else if ratway && !stale {
            m.step = Step::Ratway;
            m.doors = vec![];
            m.decision = None;
        } else if v.decision == Some(Decision::Office) {
            m.step = Step::Office;
            m.doors = vec![];
            m.decision = None;
        } else if let Some(Decision::Manage { consume, replace, end, hunt }) = v.decision {
            // «Отменить» на экране цепочки после укуса возвращает ряд «Пожирание /
            // Завершить» в тот же день [кадры 22.10 215354 → 215355: 8 из 8]; дальше
            // по движку день кончен. Тот же ряд на следующих кадрах — тот же признак.
            let after_chain = (m.step == Step::Chain && !m.first_bite)
                || (m.step == Step::Manage && m.decision.is_some_and(|d| d.after_chain));
            m.step = Step::Manage;
            m.doors = vec![];
            m.decision = Some(DecisionMem { consume, replace, end, hunt, after_chain });
        } else if !v.doors.is_empty() {
            m.step = Step::Patrol;
            let w = m.warden;
            m.doors = v.doors.iter().map(|&d| d as i32).filter(|&n| n != w).collect();
            m.decision = None;
        }
    }

    /// Метки «Добыча» по прицелам на доске.
    fn read_marks(&mut self, v: &BoardView) {
        let m = &mut self.mem;
        let seen: Vec<i32> = v
            .cells
            .iter()
            .enumerate()
            .filter(|(n, c)| *n as i32 != OFFICE && c.kind == CellKind::Inmate && c.prey == Some(true))
            .map(|(n, _)| n as i32)
            .collect();
        m.marks.retain(|k| seen.contains(&k.cell));
        for &n in &seen {
            if m.marks.iter().any(|k| k.cell == n) {
                continue;
            }
            let h = m.hunt_at.filter(|h| h.1 < m.day);
            // Экран выбора добычи не сняли: метил лев надзирателя — утром надзиратель
            // ещё у него [журнал фактов 22.10: 15 из 15 новых меток].
            let w = m.warden;
            let lion_here = w != n && m.cells.get(w as usize).is_some_and(|c| c.kind() == CellK::Inmate && c.b == Some(LION));
            let lion = h.map(|h| h.0).unwrap_or(if lion_here { w } else { -1 });
            m.marks.push(Mark { cell: n, lion, day: h.map(|h| h.1).unwrap_or(m.day - 1) });
            if h.is_some() {
                m.hunt_at = None;
            }
        }
    }

    /// Экран цепочки: доска затемнена; едок только растёт, съеденные запираются.
    fn chain_board(&mut self, v: &BoardView) {
        let m = &mut self.mem;
        let warden = v.warden.map(|w| w as i32).unwrap_or(m.warden);
        let after = m.warden == warden && (m.step == Step::Manage || m.step == Step::Chain);
        let walked = m.step == Step::Patrol && m.doors.contains(&warden);
        let known = after || walked;
        let was = m.cells[warden as usize].clone();
        let was_cells = m.cells.clone();
        let mut eaten = 0;
        for (n, c) in v.cells.iter().enumerate() {
            if n as i32 == OFFICE || n as i32 == warden {
                continue;
            }
            let prev = m.cells[n].clone();
            if known {
                // Съеденный за этот укус — заперт; остальные — как до укуса.
                if c.kind == CellKind::Locked && prev.kind() == CellK::Inmate {
                    let mut l = CellMemory::locked();
                    l.ev = prev.ev || c.event == Some(true);
                    m.cells[n] = l;
                    eaten += 1;
                } else if c.kind == CellKind::Inmate && prev.kind() != CellK::Inmate {
                    m.cells[n] = CellMemory::inmate(beast_no(c.beast), (c.tier.unwrap_or(0) as i32).min(SUPREME));
                }
                continue;
            }
            m.cells[n] = match c.kind {
                CellKind::Inmate => CellMemory::inmate(beast_no(c.beast), (c.tier.unwrap_or(0) as i32).min(SUPREME)),
                CellKind::Locked => {
                    let mut l = CellMemory::locked();
                    l.ev = c.event == Some(true);
                    l
                }
                _ => CellMemory::of(CellK::Empty),
            };
        }
        let eater = v.cells.get(warden as usize);
        let prev_tier = if known && was.kind() == CellK::Inmate { was.t.unwrap() } else { 0 };
        if let Some(e) = eater.filter(|e| e.kind == CellKind::Inmate) {
            m.cells[warden as usize] =
                CellMemory::inmate(beast_no(e.beast), (e.tier.unwrap_or(0) as i32).max(prev_tier).min(SUPREME));
        } else if was.kind() != CellK::Inmate {
            m.cells[warden as usize] = CellMemory::inmate(0, prev_tier);
        }
        let tier = m.cells[warden as usize].t.unwrap();
        // Никого не съели, едок не вырос, а прошлый экран — ряд решений: выбор цели первого укуса.
        let unchanged = walked
            || (was.kind() == CellK::Inmate
                && eater.is_some_and(|e| e.kind == CellKind::Inmate)
                && was.t == Some(eater.unwrap().tier.unwrap_or(0) as i32));
        m.first_bite = if m.step == Step::Chain { m.first_bite && eaten == 0 } else { known && eaten == 0 && unchanged };
        // Подсвеченная цель той же редкости, что и едок (если орудие не меняет правило).
        let targets_v = v.chain.clone().unwrap_or_default();
        for &n in &targets_v {
            let c = m.cells[n].clone();
            if c.kind() == CellK::Locked {
                continue; // сквозь дверь (Когти)
            }
            let seen = v.cells.get(n);
            let seen_beast = seen.and_then(|s| s.beast);
            if c.kind() == CellK::Inmate
                && c.t.unwrap() <= tier
                && (seen_beast.is_none() || c.b == Some(beast_no(seen_beast)))
            {
                continue;
            }
            m.cells[n] = CellMemory::inmate(if seen_beast.is_some() { beast_no(seen_beast) } else { c.b.unwrap_or(0) }, tier);
        }
        // «Бивень» сходит, только когда камера пустеет: съеденные уже заперты.
        for (n, c) in v.cells.iter().enumerate() {
            if n as i32 != OFFICE && m.cells[n].kind() == CellK::Inmate {
                m.cells[n].tusk = tusk_kept(&was_cells[n], c);
            }
        }
        m.warden = warden;
        m.step = Step::Chain;
        m.chain_targets = targets_v.iter().map(|&n| n as i32).collect();
        m.doors = vec![];
        // До первого укуса это ещё выбор Пожирания: какие карточки на ряде решений, помним.
        if !m.first_bite {
            m.decision = None;
        }
        // Портрет едока на тёмном экране читается и другим зверем: берём того,
        // кто ест все подсвеченные цели (прочитанный, прежний, любой).
        if !m.chain_targets.is_empty() {
            let mut order = vec![m.cells[warden as usize].b.unwrap()];
            if was.kind() == CellK::Inmate {
                order.push(was.b.unwrap());
            }
            order.extend([0, 1, 2, 3]);
            for &b in &order {
                self.mem.cells[warden as usize].b = Some(b);
                if self.chain_fits() {
                    break;
                }
            }
            if !self.chain_fits() {
                self.mem.cells[warden as usize].b = Some(order[0]);
            }
        }
    }

    fn chain_fits(&self) -> bool {
        let s = self.build();
        let mask = targets(&s, self.mem.warden);
        self.mem.chain_targets.iter().all(|&n| (mask >> n) & 1 == 1)
    }

    /// День по логу кадра; None — строки «check cur» не было (день — с экрана).
    pub fn log_day(&mut self, d: Option<(i32, i32)>) {
        self.day_from_log = d.is_some_and(|d| d.1 > 0);
        let Some((cur, max)) = d.filter(|d| d.1 > 0) else { return };
        // cur — номер дня с единицы; 0 — выбор орудий перед первым днём.
        let day = 0.max((max - 1).min(cur - 1));
        // День ушёл назад — это новый забег, начало которого Горн не видел (открыт
        // посреди забега): память прошлого не годится [журнал фактов 24.10 10:07 —
        // дни 0–6 с хранилищем 8 и орудиями прошлого забега].
        if day + 1 < self.mem.day {
            self.start_run();
            self.mem.run_start = false;
        }
        let m = &mut self.mem;
        m.day_limit = max;
        if max == self.p.iron_law_days && m.start == start_event::NONE {
            m.start = start_event::IRONLAW;
        }
        if day > m.day {
            m.day_storage = m.storage;
            self.fresh_day = true;
            // Выбор добычи и ряд решений вчерашние: новый день начинается с обхода.
            if matches!(m.step, Step::Hunt | Step::Manage | Step::Ratway) {
                m.step = Step::Patrol;
                m.doors = vec![];
                m.decision = None;
            }
        }
        m.day = day;
    }

    /// Совет на выборе орудий: какое орудие посоветовано.
    pub fn advised(&mut self, tool: u32) {
        self.mem.pick_advised = Some(tool);
    }

    /// Орудия с панели.
    fn panel_tools(&mut self, v: &BoardView) {
        if v.tool_slots.is_none() && v.tools.is_empty() {
            return;
        }
        let vt: Vec<u32> = v.tools.iter().filter_map(|t| tool_index(t)).collect();
        let m = &mut self.mem;
        // Орудие из недавнего выбора — сразу; прочие — со второго кадра подряд.
        let seen = std::mem::take(&mut m.tool_sightings);
        for &t in &vt {
            if m.tools.contains(&t) {
                continue;
            }
            let offered = m.pick_offer.contains(&t) || m.assumed.iter().any(|a| a.1.contains(&t));
            if !offered && !seen.contains(&t) {
                m.tool_sightings.push(t);
                continue;
            }
            // Панель узнала орудие, а «предположительно» записано другое из того же выбора — исправить.
            if let Some(i) = m.assumed.iter().position(|a| a.1.contains(&t) && a.0 != t && !vt.contains(&a.0)) {
                let wrong = m.assumed[i].0;
                m.tools.retain(|&x| x != wrong);
                m.assumed[i].0 = t;
            }
            m.tools.push(t);
        }
        m.assumed.retain(|a| !vt.contains(&a.0));
        self.forget_missing_tools(v, &vt);
        let m = &mut self.mem;
        if v.tool_slots.is_none() || m.pick_offer.is_empty() {
            return;
        }
        let offer = std::mem::take(&mut m.pick_offer);
        let advised = m.pick_advised.take();
        if vt.iter().any(|t| offer.contains(t)) {
            return;
        }
        // В выборе было орудие без эталона панели: не выдумываем.
        if offer.iter().any(|&t| !self.panel_known.contains(&t)) {
            return;
        }
        if let Some(a) = advised {
            if !m.tools.contains(&a) {
                m.tools.push(a);
                m.assumed.push((a, offer));
            }
        }
    }

    /// Орудие из памяти, которого на целиком объяснённой панели нет три кадра подряд, — забыть.
    fn forget_missing_tools(&mut self, v: &BoardView, vt: &[u32]) {
        let Some(slots) = &v.tool_slots else { return };
        if slots.is_empty() {
            return;
        }
        let unknown = slots.iter().filter(|t| t.is_none()).count();
        let blind = self.mem.tools.iter().filter(|&&t| !self.known(t)).count();
        if unknown > blind {
            return;
        }
        for t in self.mem.tools.clone() {
            let present = if self.known(t) { vt.contains(&t) } else { unknown > 0 };
            if present {
                self.tool_misses.remove(&t);
                continue;
            }
            let n = self.tool_misses.get(&t).copied().unwrap_or(0) + 1;
            self.tool_misses.insert(t, n);
            if n >= 3 {
                self.mem.tools.retain(|&x| x != t);
                self.mem.assumed.retain(|a| a.0 != t);
                self.tool_misses.remove(&t);
            }
        }
    }

    /// Окно темы забега: voice — номер озвучки из лога.
    pub fn theme(&mut self, voice: Option<i32>) {
        let Some(id) = voice.and_then(theme_by_voice) else { return };
        self.mem.start = id;
        self.mem.theme_fresh = true;
        if id == start_event::IRONLAW {
            self.mem.day_limit = self.p.iron_law_days;
        }
    }

    /// Выбор пришёл утром, до обхода: после него — обход этого дня.
    fn resume_morning(&mut self) {
        let m = &mut self.mem;
        if m.step != Step::Patrol {
            m.doors = vec![];
        }
        m.resume = Step::Patrol;
        m.resume_after = Cause::Morning;
    }

    /// Новый забег: доска за выбором — чистая; цель и тему с окон перед выбором не теряем.
    pub fn start_run(&mut self) {
        let theme = self.mem.run_start || self.mem.theme_fresh;
        let objective = if self.mem.run_start { self.mem.objective } else { None };
        let start = if theme { self.mem.start } else { start_event::NONE };
        let day_limit = if theme { self.mem.day_limit } else { DEFAULT_PARAMS.days };
        let theme_fresh = self.mem.theme_fresh;
        self.mem = RunMemory { objective, start, day_limit, theme_fresh, ..blank_memory() };
        self.mem.run_start = true;
        self.tool_misses.clear();
    }

    pub fn pick(&mut self, v: &PickView, fresh: bool, rerolled: bool, after: Cause) {
        if !self.mem.step.is_choice() {
            if after == Cause::Morning && !fresh {
                self.resume_morning();
            } else {
                self.mem.resume = self.mem.step;
                self.mem.resume_after = Cause::None;
            }
        }
        if fresh && !v.reward {
            self.start_run();
        }
        let m = &mut self.mem;
        // Выбор в награду — цель выполнена (одна на забег).
        if v.reward {
            m.objective_done = true;
        }
        m.step = Step::Pick;
        m.offer = v.cards.iter().flatten().filter_map(|c| tool_index(c)).filter(|t| !m.tools.contains(t)).collect();
        m.pick_offer = m.offer.clone();
        m.pick_advised = None;
        m.rerolls = if v.reroll && !rerolled { 1 } else { 0 };
    }

    /// Подарок взят: при «Расточительности» выборов несколько — осталось на один меньше.
    pub fn gift_taken(&mut self) {
        if self.mem.gifts > 0 {
            self.mem.gifts -= 1;
        }
        self.chain_taken += 1;
        // Взятый подарок даст ещё Призматических — выборов следом по каждому.
        if self.chain_gifts && self.gift_prediction > 0 {
            self.mem.gifts += self.gift_prediction * self.gifts_per_prism();
            self.chain_storage = self.chain_storage.map(|c| c + self.gift_prediction);
        }
        // Выборы кончились — цепочка закрыта: призмы до следующего окна не в счёт.
        if self.mem.gifts <= 0 {
            self.chain_storage = None;
        }
        self.gift_prediction = 0;
    }

    /// Выборов в сокровищнице за одного Призматического: при «Расточительности» — несколько.
    fn gifts_per_prism(&self) -> i32 {
        if self.mem.start == start_event::ABUNDANCE { self.p.abundance_gifts } else { 1 }
    }

    /// Сокровищница. view — доска за окном уже видна; иначе after — что к ней привело.
    pub fn treasure(&mut self, view: bool, after: Cause) {
        if view {
            self.mem.step = Step::Gift;
            self.mem.gifts = self.mem.gifts.max(1);
            if self.chain_gifts {
                // Доска за окном: призм больше ожидаемого — выборов прибавилось
                // по каждому, меньше (только по настоящему просмотру) — подарок
                // не дал предсказанного. Первый просмотр в цепочке — отсчёт.
                let per = self.gifts_per_prism();
                let s = self.mem.storage;
                match self.chain_storage {
                    None if self.board_viewed => self.chain_storage = Some(s),
                    None => {}
                    Some(prev) => {
                        let delta = s - prev;
                        if delta > 0 || (delta < 0 && self.board_viewed) {
                            self.mem.gifts = (self.mem.gifts + delta * per).max(1);
                            self.chain_storage = Some(s);
                        }
                    }
                }
                if let Some(base) = self.chain_base {
                    let owed = (s.min(self.p.storage) - base) * per - self.chain_taken;
                    self.mem.gifts = self.mem.gifts.max(owed);
                }
            }
            return;
        }
        if self.chain_gifts && (self.mem.gifts <= 0 || self.mem.day != self.chain_day) {
            // Выборов не осталось или день другой — новая цепочка.
            self.mem.gifts = 0;
            self.chain_day = self.mem.day;
            self.chain_storage = None;
            // Утренние призмы — с начала дня, остальные — с последней доски до окна.
            let m = &self.mem;
            self.chain_base = Some(if after == Cause::Morning { m.day_storage } else { m.storage });
            self.chain_taken = 0;
        }
        if after == Cause::Morning {
            // Утренний Призматический уже в хранилище.
            let m = &mut self.mem;
            m.storage_floor = m.storage_floor.max(m.day_storage + 1);
            m.storage = m.storage.max(m.storage_floor);
        }
        if !self.mem.step.is_choice() {
            if after == Cause::Morning {
                self.resume_morning();
            } else {
                self.mem.resume = self.mem.step;
                self.mem.resume_after = after;
            }
        }
        let per = self.gifts_per_prism();
        let m = &mut self.mem;
        let w = m.warden as usize;
        if after == Cause::Swap && m.cells.get(w).is_some_and(|c| c.kind() == CellK::Inmate) {
            m.cells[w] = CellMemory::locked();
            m.storage += 1;
        }
        // Окно снова открылось, а выборы за прошлого призматического не кончились — счёт не сбрасывать.
        if m.gifts <= 0 {
            m.gifts = per;
        }
        m.step = Step::Wait;
    }

    /// Позиция движка по памяти.
    pub fn position(&self) -> Position {
        let m = &self.mem;
        match m.step {
            Step::Wait => return Position::No { problem: "сокровищница: доска за окном ещё не видна", fallback: None },
            Step::Patrol if m.doors.is_empty() => return Position::No { problem: "нет дверей", fallback: None },
            Step::Pick if m.offer.is_empty() => return Position::No { problem: "нет карт выбора", fallback: None },
            Step::Manage if m.decision.is_some_and(|d| d.after_chain) => {
                // Цепочку прервали — у движка это конец Пожирания и дня.
                return Position::No {
                    problem: "цепочку прервали — день кончается",
                    fallback: Some(crate::actions::code(A_END, 0)),
                };
            }
            Step::Chain => {
                if m.cells.get(m.warden as usize).map(CellMemory::kind) != Some(CellK::Inmate) {
                    return Position::No { problem: "нет едока", fallback: None };
                }
                if !m.first_bite && m.chain_targets.is_empty() {
                    // После укуса экран выбора цели показывается и без целей (едок стал
                    // Призматическим или есть некого) и проходит сам [кадры 21.10: 10 из 25].
                    return Position::No { problem: "цепочка без подсвеченных целей", fallback: None };
                }
                if !m.chain_targets.is_empty() && !self.chain_fits() {
                    // Модель не объясняет подсвеченные цели: совет по самой редкой из подсвеченных.
                    let t = |n: i32| m.cells.get(n as usize).and_then(|c| c.t).unwrap_or(-1);
                    let best = m.chain_targets.iter().skip(1).fold(m.chain_targets[0], |a, &b| if t(b) > t(a) { b } else { a });
                    return Position::No {
                        problem: "модель не сходится с подсвеченными целями",
                        fallback: Some(crate::actions::code(crate::actions::A_CHAIN, best)),
                    };
                }
            }
            _ => {}
        }
        let mut state = self.build();
        let decision =
            if m.step == Step::Manage || (m.step == Step::Chain && m.first_bite) { m.decision } else { None };
        // Ряд решений без «Заменить», но с Пожиранием — замена в этот день уже была.
        if decision.is_some_and(|d| !d.replace && (d.consume || d.hunt)) {
            state.m[USED] = USED_SWAP;
        }
        let allowed = Allowed {
            targets_shown: (m.step == Step::Chain && m.first_bite).then(|| m.chain_targets.clone()),
            hunting: m.step == Step::Hunt,
            decision,
            no_reroll: false,
        };
        let mut buf: CodeBuf = [0; 32];
        let n = legal_codes(&state, &mut buf);
        if !buf[..n].iter().any(|&c| allowed.allows(c)) {
            return Position::No { problem: "ни один ход движка не виден на экране", fallback: None };
        }
        Position::Ok { state, allowed }
    }

    /// Состояние движка по памяти.
    pub fn build(&self) -> State {
        let m = &self.mem;
        let p = &self.p;
        let mut s = State::new();
        let tools = m.tools.iter().fold(0i32, |a, &t| a | (1 << t));
        // Выполненная цель: цели больше нет, как у движка.
        let objective = match m.objective {
            Some(o) if !m.objective_done => o,
            _ => -1,
        };

        if m.step == Step::Pick && m.run_start {
            // Начало забега: доска за выбором ещё не открыта — как в движке.
            new_run(&mut s, &mut Rng::new(0x9e37_79b1), p, RunSetup { start: m.start, objective, tools, start_pick: true });
            self.offer(&mut s);
            return s;
        }

        let x = &mut s.m;
        for c in 0..CELLS {
            x[BEAST + c] = -1;
            x[TIER + c] = -1;
            x[PRE_TIER + c] = -1;
            x[PRE_BEAST + c] = -1;
            let cell = &m.cells[c];
            if c as i32 == OFFICE {
                continue;
            }
            match cell.kind() {
                CellK::Locked => {
                    x[LOCK + c] = 1;
                    if cell.nest {
                        x[NEST + c] = 1.max(cell.nest_day.unwrap_or(m.day) + p.ironbox_days - m.day);
                    }
                    if let Some(shown) = cell.shown {
                        x[PRE_TIER + c] = shown;
                        x[PRE_BEAST + c] = 0;
                        x[FLAGS + c] =
                            F_VIS_TIER | if cell.scratch && (tools >> tool::CLAWS) & 1 == 1 { F_SCRATCH } else { 0 };
                    }
                }
                CellK::Inmate => {
                    x[BEAST + c] = cell.b.unwrap();
                    x[TIER + c] = cell.t.unwrap();
                    if cell.tusk && (tools >> tool::TONGS) & 1 == 1 {
                        x[FLAGS + c] = F_TUSK;
                    }
                }
                _ => {}
            }
            // Вид события неизвестен: любой из пула, решатель его перебросит.
            if cell.ev && cell.kind() != CellK::Empty {
                x[EVT + c] = event::CHAIN + 1;
            }
        }
        x[WARDEN] = m.warden;
        x[DAY] = m.day;
        x[DAY_LIMIT] = m.day_limit;
        x[START] = m.start;
        x[TOOLS] = tools;
        x[STORAGE] = m.storage.min(p.storage);
        x[OBJECTIVE_ID] = objective;
        x[OBJ_DONE] = m.objective_done as i32;
        x[USURP] = event::USURP1;
        x[CHAIN_EATER] = -1;
        // Метки «Добыча»: поставлена в день d — к утру дня d+k осталось trapMornings − k.
        for k in &m.marks {
            x[PREY_LEFT + k.cell as usize] = 1.max(p.trap_mornings - (m.day - k.day));
            x[PREY_LION + k.cell as usize] = k.lion;
        }
        x[REROLLS] = m.rerolls;

        let mut phase = P_PATROL;
        match m.step {
            Step::Patrol => {
                let n = m.doors.len().min(3);
                for i in 0..n {
                    x[DOORS + i] = m.doors[i];
                }
                x[DOORS_N] = n as i32;
            }
            // Выбор добычи — тот же ряд решений, разрешено только «Выслеживание».
            Step::Manage | Step::Hunt => phase = P_MANAGE,
            Step::Office => {
                phase = P_OFFICE;
                x[OFFICE_LEFT] = office_rounds(&State::new(), p);
            }
            Step::Ratway => phase = P_RATWAY,
            Step::Chain => {
                if m.first_bite {
                    // До первого укуса: выбор Пожирания; «Отменить» → Замена или конец дня.
                    phase = P_MANAGE;
                } else {
                    phase = P_CHAIN;
                    x[CHAIN_EATER] = m.warden;
                    x[USED] = USED_DEVOUR;
                }
            }
            Step::Pick => {
                // Награда за цель посреди дня: после выбора день доигрывается.
                self.offer(&mut s);
                self.resume_after_pick(&mut s);
                return s;
            }
            Step::Gift | Step::Wait => {
                x[OFFER_KIND] = OFFER_GIFT;
                x[OFFER_N] = GIFT_IDS.len() as i32;
                for i in 0..GIFT_IDS.len() {
                    x[OFFER + i] = i as i32;
                }
                x[PICK_GIFTS] = m.gifts.max(1);
                x[PHASE] = P_PICK;
                self.resume_after_pick(&mut s);
                return s;
            }
        }
        x[PHASE] = phase;
        x[NEXT] = phase;
        s
    }

    /// Куда забег вернётся после выбора.
    fn resume_after_pick(&self, s: &mut State) {
        let m = &self.mem;
        let x = &mut s.m;
        if m.resume_after == Cause::Morning {
            // Утренний выбор: впереди обход этого дня. Двери не видны — движок их раздаст.
            let n = m.doors.len().min(3);
            for i in 0..n {
                x[DOORS + i] = m.doors[i];
            }
            x[DOORS_N] = n as i32;
            x[NEXT] = P_PATROL;
            return;
        }
        if m.resume == Step::Patrol && !m.doors.is_empty() {
            // Награда за цель приходит по клику на дверь, раньше хода надзирателя
            // [факты 21.10: 10 выборов из 10 — надзиратель на старом месте]:
            // обход ещё впереди, цель уже выполнена.
            let n = m.doors.len().min(3);
            for i in 0..n {
                x[DOORS + i] = m.doors[i];
            }
            x[DOORS_N] = n as i32;
            x[OBJECTIVE_ID] = -1;
            x[OBJ_DONE] = 1;
            x[NEXT] = P_PATROL;
            return;
        }
        let mut next = P_DAYEND;
        if matches!(m.resume, Step::Patrol | Step::Office | Step::Ratway) {
            next = P_MANAGE;
        } else if m.resume == Step::Chain || (m.resume == Step::Manage && m.resume_after == Cause::Consume) {
            next = P_CHAIN;
        }
        if next == P_CHAIN {
            x[CHAIN_EATER] = m.warden;
            x[USED] = USED_DEVOUR;
        }
        x[NEXT] = next;
    }

    fn offer(&self, s: &mut State) {
        let x = &mut s.m;
        let ids = &self.mem.offer[..self.mem.offer.len().min(3)];
        x[OFFER_KIND] = OFFER_TOOL;
        x[OFFER_N] = ids.len() as i32;
        for (i, &id) in ids.iter().enumerate() {
            x[OFFER + i] = id as i32;
        }
        x[PICK_TOOLS] = x[PICK_TOOLS].max(1);
        x[REROLLS] = self.mem.rerolls;
        x[PHASE] = P_PICK;
    }
}
