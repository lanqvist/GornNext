//! Когда снимать кадр сезонной тюрьмы и в тюрьме ли игрок — по строкам лога.
//! Перенос `core/prisonTriggers.ts`, `core/prison/presence.ts` и
//! `core/prison/frameHash.ts` (gorn-tli).
//!
//! Игра пишет в лог строки своего интерфейса: открылось окно цели, выбор
//! орудий, начался день, сыграла карточка действия. Строки идут пачкой, кадр
//! снимается, когда лог затих на самую долгую задержку из пачки; пачка
//! получает самый «старший» вид кадра. Время — из меток в строках, срок
//! пачки — момент по часам процесса (Instant), его проверяет вызывающий.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use gorn_prison::adapter::Cause;
use gorn_prison::advisor::ShotKind;
use regex::Regex;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum RuleKind {
    Shot(ShotKind),
    Skip,
    Speed,
}

struct Rule {
    kind: RuleKind,
    re: Regex,
    settle_ms: f64,
}

fn rules() -> &'static [Rule] {
    static R: OnceLock<Vec<Rule>> = OnceLock::new();
    R.get_or_init(|| {
        let r = |kind, re: &str, settle_ms| Rule {
            kind,
            re: Regex::new(&format!("(?i){re}")).unwrap(),
            settle_ms,
        };
        use RuleKind::*;
        use ShotKind::*;
        vec![
            r(Shot(Objective), r"S16BoardTaskPop Run", 600.0),
            // Окно особой темы забега в начале (за ним — озвучка Special_0N с номером темы).
            r(Shot(Theme), r"S16BoardThemePop Run", 600.0),
            // Карты орудий влетают дольше прочего.
            r(Shot(Instruments), r"S16BoardRelicPop Run", 900.0),
            r(Shot(Instruments), r"Btn_S16Board_RelicPop_Reroll", 900.0),
            // День начался: баннер закрылся, игра пишет номер дня (на итогах — max=0).
            r(
                Shot(Board),
                r"\[S16LastDay\] check cur=\d+ max=[1-9]",
                500.0,
            ),
            r(Shot(Board), r"UI_S16_Card_Up", 400.0),
            r(Shot(Board), r"Btn_S16Board_ActionCell", 600.0),
            // Выбрана дверь: надзиратель идёт.
            r(Shot(Board), r"Btn_S16Board_RouteCell", 800.0),
            r(Shot(Board), r"UI_S16_Click_Eat", 800.0),
            r(Skip, r"S16BoardNewDayPop Run", 0.0),
            // Подарки рисуются по месту карточек, картинка не нужна.
            r(Shot(Treasure), r"S16BoardColoredPop Run", 250.0),
            // «Destory» — так в самой игре.
            r(Shot(Board), r"S16BoardColoredPop Destory", 300.0),
            r(Speed, r"Btn_S16Board_Top_Speed", 0.0),
            r(Shot(Board), r"UI_S16_Click_Cancel", 500.0),
            r(Shot(Board), r"Btn_S16Board_EventPop_Confirm", 500.0),
        ]
    })
}

fn rank(k: ShotKind) -> u8 {
    match k {
        ShotKind::Board => 1,
        ShotKind::Instruments => 2,
        _ => 3,
    }
}

fn re(s: &'static str) -> &'static Regex {
    // Небольшой кэш регулярок по тексту: их всего несколько.
    static C: OnceLock<std::sync::Mutex<Vec<(&'static str, &'static Regex)>>> = OnceLock::new();
    let c = C.get_or_init(|| std::sync::Mutex::new(vec![]));
    let mut g = c.lock().unwrap();
    if let Some((_, r)) = g.iter().find(|(k, _)| *k == s) {
        return r;
    }
    let r: &'static Regex = Box::leak(Box::new(Regex::new(s).unwrap()));
    g.push((s, r));
    r
}

/// Метка времени строки лога в мс (как UTC), или None.
pub fn stamp_ms(line: &str) -> Option<i64> {
    let m =
        re(r"^\[(\d{4})\.(\d{2})\.(\d{2})-(\d{2})\.(\d{2})\.(\d{2}):(\d{3})\]").captures(line)?;
    let n = |i: usize| m[i].parse::<i64>().unwrap();
    let (y, mo, d) = (n(1), n(2), n(3));
    let (yy, mm) = if mo <= 2 {
        (y - 1, mo + 9)
    } else {
        (y, mo - 3)
    };
    let era = yy.div_euclid(400);
    let yoe = yy - era * 400;
    let doy = (153 * mm + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some((((days * 24 + n(4)) * 60 + n(5)) * 60 + n(6)) * 1000 + n(7))
}

/// Кадр, который пора снять, и то, что о нём известно из лога.
#[derive(Clone, Debug)]
pub struct PrisonShot {
    pub kind: ShotKind,
    /// Выбор орудий открывает новый забег (рядом строка дня 0).
    pub fresh: bool,
    pub after: Cause,
    /// Доска, открытая из сокровищницы кнопкой «Посмотреть состояние тюрьмы».
    pub treasure_view: bool,
    /// Выбор орудий после «Обновить».
    pub rerolled: bool,
    /// Доска сразу после окна сокровищницы.
    pub treasure_closed: bool,
    /// Окно темы: номер озвучки «Play_VO_Story_S16_Special_0N».
    pub theme: Option<i32>,
    /// День по логу: последняя строка «check cur=N max=M».
    pub day: Option<(i32, i32)>,
    pub lines: Vec<String>,
}

struct Burst {
    kind: ShotKind,
    settle: f64,
    lines: Vec<String>,
    due: Instant,
}

#[derive(Default)]
pub struct PrisonTriggers {
    burst: Option<Burst>,
    pace: f64,
    last_action: i64,
    last_eat: i64,
    last_gift: i64,
    day_start: i64,
    last_board_click: i64,
    last_route: i64,
    last_special: Option<(i32, i64)>,
    day: Option<(i32, i32)>,
}

impl PrisonTriggers {
    pub fn new() -> Self {
        PrisonTriggers {
            pace: 1.0,
            ..Default::default()
        }
    }

    /// День забега по последней строке «check cur=N max=M».
    pub fn last_day(&self) -> Option<(i32, i32)> {
        self.day
    }

    /// Сбросить ожидание.
    pub fn cancel(&mut self) {
        self.burst = None;
    }

    /// Строки по порядку; готовые кадры (сокровищница перед доской) — в out.
    pub fn feed(&mut self, lines: &[String], now: Instant, out: &mut Vec<PrisonShot>) {
        for l in lines {
            self.line(l, now, out);
        }
    }

    /// Срок пачки вышел — кадр.
    pub fn poll(&mut self, now: Instant) -> Option<PrisonShot> {
        if self.burst.as_ref().is_some_and(|b| now >= b.due) {
            return self.fire();
        }
        None
    }

    fn line(&mut self, line: &str, now: Instant, out: &mut Vec<PrisonShot>) {
        // Красный экран пожирания пишется без «S16» — его ловим до отсева.
        if line.contains("Eating_red_screen") {
            self.last_eat = stamp_ms(line).unwrap_or(0);
        }
        if !line.contains("S16") {
            return;
        }
        let at = stamp_ms(line).unwrap_or(0);
        // На итогах забега игра пишет max=0 — это не день.
        if let Some(d) = re(r"check cur=(\d+) max=(\d+)").captures(line) {
            let (c, m) = (d[1].parse().unwrap_or(0), d[2].parse().unwrap_or(0));
            if m > 0 {
                self.day = Some((c, m));
            }
        }
        if let Some(sp) = re(r"S16_Special_(\d+)").captures(line) {
            self.last_special = Some((sp[1].parse().unwrap_or(0), at));
        }
        if line.contains("Btn_S16Board_ActionCell") {
            self.last_action = at;
        }
        if line.contains("ColoredPop_Confirm") {
            self.last_gift = at;
        }
        if re(r"(?i)S16BoardNewDayPop Run").is_match(line) {
            self.day_start = at;
        }
        if re(r"Btn_S16Board_(?:RouteCell|ActionCell|Cell)\b").is_match(line) {
            self.last_board_click = at;
        }
        if re(r"(?i)Btn_S16Board_(?:RouteCell|EventPop_Confirm)\b|S16BoardNewDayPop Run")
            .is_match(line)
        {
            self.last_route = at;
        }
        let Some(rule) = rules().iter().find(|r| r.re.is_match(line)) else {
            return;
        };
        let kind = match rule.kind {
            RuleKind::Skip => {
                self.cancel();
                return;
            }
            RuleKind::Speed => {
                self.pace = if self.pace < 1.0 { 1.0 } else { 0.45 };
                return;
            }
            RuleKind::Shot(k) => k,
        };
        // Доска после закрытия сокровищницы — свой кадр: ждущая сокровищница уходит сразу.
        if self
            .burst
            .as_ref()
            .is_some_and(|b| b.kind == ShotKind::Treasure)
            && kind == ShotKind::Board
        {
            if let Some(s) = self.fire() {
                out.push(s);
            }
        }
        let b = self.burst.get_or_insert(Burst {
            kind,
            settle: 0.0,
            lines: vec![],
            due: now,
        });
        if rank(kind) > rank(b.kind) {
            b.kind = kind;
        }
        b.settle = b.settle.max((rule.settle_ms * self.pace).max(250.0));
        if b.lines.len() < 40 {
            b.lines.push(line.to_string());
        }
        b.due = now + Duration::from_secs_f64(b.settle / 1000.0);
    }

    fn fire(&mut self) -> Option<PrisonShot> {
        let b = self.burst.take()?;
        let fresh = b.lines.iter().any(|l| re(r"check cur=0\b").is_match(l));
        let find = |r: &'static str| {
            b.lines
                .iter()
                .find(|l| re(r).is_match(l))
                .and_then(|l| stamp_ms(l))
        };
        let mut after = Cause::None;
        if b.kind == ShotKind::Treasure || (b.kind == ShotKind::Instruments && !fresh) {
            let pop = find(r"(?i)S16Board(?:Colored|Relic)Pop Run").unwrap_or(0);
            // Утро: окно открылось после баннера нового дня, а на доске с тех пор ничего не нажимали.
            let morning = pop != 0
                && self.day_start != 0
                && pop >= self.day_start
                && self.day_start > self.last_board_click;
            if morning {
                after = Cause::Morning;
            } else if b.kind == ShotKind::Treasure
                && pop != 0
                && self.last_action != 0
                && pop >= self.last_action
                && pop - self.last_action <= 6000
                && self.last_route < self.last_action
            {
                after = if self.last_eat >= self.last_action - 500 {
                    Cause::Consume
                } else {
                    Cause::Swap
                };
            }
        }
        let shut = find(r"(?i)ColoredPop Destory");
        let treasure_view = match shut {
            Some(s) if s != 0 => {
                !(self.last_gift != 0 && s >= self.last_gift && s - self.last_gift < 3000)
            }
            _ => false,
        };
        let rerolled = b.kind == ShotKind::Instruments
            && b.lines
                .iter()
                .any(|l| re(r"(?i)Btn_S16Board_RelicPop_Reroll").is_match(l));
        // Озвучка темы идёт сразу за окном (через десятки мс).
        let mut theme = None;
        if b.kind == ShotKind::Theme {
            if let Some((n, at)) = self.last_special {
                let pop = find(r"(?i)S16BoardThemePop Run").unwrap_or(0);
                if (at - pop).abs() <= 3000 {
                    theme = Some(n);
                }
            }
        }
        Some(PrisonShot {
            kind: b.kind,
            fresh,
            after,
            treasure_view,
            rerolled,
            treasure_closed: shut.is_some_and(|s| s != 0),
            theme,
            day: self.day,
            lines: b.lines,
        })
    }
}

/// Строка интерфейса тюрьмы.
fn prison_ui(l: &str) -> bool {
    l.contains("S16Board") || l.contains("S16GamePlay") || l.contains("[S16LastDay]")
}

/// В тюрьме ли игрок — по строкам лога игры.
#[derive(Default)]
pub struct PrisonPresence {
    pub inside: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PresenceChange {
    Enter,
    Leave,
}

impl PrisonPresence {
    pub fn feed(&mut self, lines: &[String]) -> Option<PresenceChange> {
        let was = self.inside;
        for l in lines {
            if let Some(m) = re(r"PageStack@ CurRunView = \d+_(\w+?)Ctrl\b").captures(l) {
                self.inside = m[1].starts_with("S16");
            } else if prison_ui(l) {
                self.inside = true;
            }
        }
        match (was, self.inside) {
            (false, true) => Some(PresenceChange::Enter),
            (true, false) => Some(PresenceChange::Leave),
            _ => None,
        }
    }
}

/// В пачке строк лога открылось новое окно (или начался день): экран точно сменился.
pub fn screen_changed(lines: &[String]) -> bool {
    lines
        .iter()
        .any(|l| re(r"S16Board\w*Pop Run|check cur=\d+").is_match(l))
}

/// Ход игрока в логе: старая подсказка больше не про этот экран.
pub fn player_action(l: &str) -> bool {
    re(r"normal-click Btn_S16Board_(RouteCell|ActionCell|RelicPop|ColoredPop|EventPop)|UI_S16_Click_(Eat|Card|Go_Cell|Cancel)").is_match(l)
}

/// Отпечаток кадра: размер и хеш 4096 точек сетки (заметить застывший поток).
pub fn frame_hash(w: usize, h: usize, rgba: &[u8]) -> String {
    let n = w * h;
    let mut x: u32 = 0x811c_9dc5;
    if n > 0 && rgba.len() >= n * 4 {
        let step = (n / 4096).max(1);
        let mut i = 0;
        while i < n {
            let o = i * 4;
            for c in 0..3 {
                x = (x ^ rgba[o + c] as u32).wrapping_mul(0x0100_0193);
            }
            i += step;
        }
    }
    format!("{w}x{h}:{x:x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn l(s: &str) -> String {
        format!("[2026.10.20-12.00.00:000][  0]{s}")
    }

    #[test]
    fn board_after_route() {
        let mut t = PrisonTriggers::new();
        let now = Instant::now();
        let mut out = vec![];
        t.feed(
            &[l("LogTemp: normal-click Btn_S16Board_RouteCell_3 S16")],
            now,
            &mut out,
        );
        assert!(out.is_empty());
        assert!(t.poll(now).is_none());
        let s = t.poll(now + Duration::from_millis(900)).expect("кадр");
        assert_eq!(s.kind, ShotKind::Board);
    }

    #[test]
    fn presence() {
        let mut p = PrisonPresence::default();
        assert_eq!(
            p.feed(&[l("PageStack@ CurRunView = 4150_S16BoardMainCtrl")]),
            Some(PresenceChange::Enter)
        );
        assert_eq!(
            p.feed(&[l("PageStack@ CurRunView = 1_FightCtrl")]),
            Some(PresenceChange::Leave)
        );
    }
}
