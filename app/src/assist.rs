//! Подсказки в сезонной тюрьме (перенос `main/prison/assist.ts`).
//!
//! Ход игрока в логе сразу убирает старую стрелку и показывает «смотрю…».
//! Строки интерфейса тюрьмы (PrisonTriggers) говорят, что экран сменился;
//! кадр берётся из потока монитора, как только картинка успокоилась, и
//! уходит советнику — без очереди: новый кадр обрывает расчёт по старому.
//! Первый совет приходит через ≤ 300 мс, досчёт уточняет его.
//!
//! Снимается монитор, на котором уже нашлась доска (пока не нашлась — под
//! курсором). Окно стрелок вырезано из захвата. Память забега сохраняется на
//! диск: перезапуск посреди забега не теряет цель, орудия и ходы дня.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gorn_prison::advisor::{OwnAdvisor, Reply, Shot, ShotKind};
use gorn_prison::recognize::{Frame, Geometry};
use serde_json::{json, Value};
use tauri::AppHandle;

use crate::capture::{Image, Miss, ScreenStream};
use crate::collect::Collect;
use crate::overlay::{display_at, display_under_cursor, ArrowLayer, Display};
use crate::shots::Shots;
use crate::triggers::{
    frame_hash, player_action, screen_changed, stamp_ms, PresenceChange, PrisonPresence,
    PrisonShot, PrisonTriggers,
};

/// Экран должен столько не меняться, чтобы его снять.
const STABLE: Duration = Duration::from_millis(300);
/// Дольше этого спокойствия не ждём: снимаем как есть.
const STABLE_MAX: Duration = Duration::from_millis(2500);
/// Автокадр, который не распознался, переснимается: через это время, потом через вдвое большее.
const RETRY_MS: u64 = 700;
const RETRIES: u32 = 2;
/// Сколько решатель проверяет варианты после первого совета.
const SOLVE_BUDGET_MS: f64 = 1500.0;
/// Подсказка без новых кадров дольше этого — устарела, прячем.
const STALE: Duration = Duration::from_secs(120);
/// Последний кадр статичного экрана годится, если снят хотя бы через столько после строки лога.
const STATIC_AFTER_LINE_MS: i64 = 300;
/// Сколько раз подтолкнуть композицию, если поток застыл на старом кадре.
const NUDGES: u32 = 2;
/// Потоков решателя: ядра − 2, не больше 4 (как пул этапа 1).
const MAX_SOLVE_THREADS: usize = 4;

pub enum Event {
    Lines(Vec<String>),
    Manual,
    /// Ctrl+Alt+H: тепловизор доски поверх подсказки.
    Thermo,
    Reply(Reply),
    OverlayReady,
    SetEnabled(bool),
    SetSaveShots(bool),
    /// Сбор всех экранов тюрьмы (`collect.rs`).
    SetCollect(bool),
}

struct Job {
    shot: Shot,
    frame: Option<Arc<Image>>,
}

#[derive(Clone)]
struct Sent {
    shot: PrisonShot,
    manual: bool,
    retry: u32,
}

pub struct Paths {
    pub state: PathBuf,
    /// Журнал фактов (этап 7): база `prison-facts.db`.
    pub facts: PathBuf,
    pub display: PathBuf,
    pub shots: PathBuf,
    pub collect: PathBuf,
}

pub struct Assist {
    tx: Sender<Event>,
    layer: ArrowLayer,
    stream: ScreenStream,
    triggers: PrisonTriggers,
    presence: PrisonPresence,
    enabled: bool,
    next_id: u64,
    latest: Arc<AtomicU64>,
    /// Кадр, для которого задержка первой подсказки уже в журнале.
    hint_logged: u64,
    jobs: Option<Sender<Job>>,
    display: Option<Display>,
    game_display: Option<Display>,
    near: Option<Geometry>,
    last_posted: Option<String>,
    queue: Vec<(PrisonShot, bool)>,
    sent: Vec<(u64, Sent, Display)>,
    retry_at: Option<(Instant, u64, Sent)>,
    stale_at: Option<Instant>,
    state: Option<Value>,
    state_due: Option<Instant>,
    shots: Option<Shots>,
    collect: Option<Collect>,
    paths: Paths,
    treasure_seq: u64,
    /// Доска закрыта (итог забега, финальная комната): пересъёмок не нужно.
    board_closed: bool,
    pub log_found: bool,
}

impl Assist {
    pub fn new(
        app: AppHandle,
        tx: Sender<Event>,
        paths: Paths,
        save_shots: bool,
        collect: bool,
    ) -> Self {
        let mut a = Assist {
            tx,
            layer: ArrowLayer::new(app),
            stream: ScreenStream::default(),
            triggers: PrisonTriggers::new(),
            presence: PrisonPresence::default(),
            enabled: true,
            next_id: 0,
            latest: Arc::new(AtomicU64::new(0)),
            hint_logged: 0,
            jobs: None,
            display: None,
            game_display: None,
            near: None,
            last_posted: None,
            queue: vec![],
            sent: vec![],
            retry_at: None,
            stale_at: None,
            state: None,
            state_due: None,
            shots: save_shots.then(|| Shots::new(paths.shots.clone())),
            collect: collect.then(|| Collect::new(paths.collect.clone())),
            paths,
            treasure_seq: 0,
            board_closed: false,
            log_found: false,
        };
        // Монитор игры и где была доска — с прошлого раза.
        if let Some(v) = std::fs::read_to_string(&a.paths.display)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        {
            if let (Some(x), Some(y)) = (v["x"].as_i64(), v["y"].as_i64()) {
                a.game_display = display_at(x as i32, y as i32);
            }
            let n = &v["near"];
            if let (Some(s), Some(tx), Some(ty), Some(w), Some(h)) = (
                n["s"].as_f64(),
                n["tx"].as_f64(),
                n["ty"].as_f64(),
                n["width"].as_u64(),
                n["height"].as_u64(),
            ) {
                a.near = Some(Geometry {
                    width: w as usize,
                    height: h as usize,
                    s,
                    tx,
                    ty,
                    score: n["score"].as_f64().unwrap_or(0.0),
                });
            }
        }
        a
    }

    /// Главный цикл: события и сроки.
    pub fn run(mut self, rx: Receiver<Event>) {
        let mut prune_at = Instant::now();
        loop {
            let ev = rx.recv_timeout(Duration::from_millis(30));
            match ev {
                Ok(e) => self.on_event(e),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            let now = Instant::now();
            if let Some(shot) = self.triggers.poll(now) {
                self.capture(shot, false);
            }
            if let Some((at, id, sent)) = self.retry_at.clone() {
                if now >= at {
                    self.retry_at = None;
                    if self.next_id == id && self.presence.inside {
                        let mut s = sent;
                        s.retry += 1;
                        self.capture_with(s.shot.clone(), s.manual, s.retry);
                    } else if self.presence.inside {
                        // Ход уже сделан — новый кадр в пути, «смотрю…» остаётся по нему.
                    } else {
                        self.layer.clear();
                    }
                }
            }
            if self.stale_at.is_some_and(|t| now >= t) {
                self.stale_at = None;
                self.layer.clear();
            }
            if self.state_due.is_some_and(|t| now >= t) {
                self.flush_state();
            }
            if self.presence.inside && self.stream.monitor.is_some() {
                if let Some(c) = &mut self.collect {
                    c.tick(&self.stream, self.triggers.last_day());
                }
            }
            if now >= prune_at {
                prune_at = now + Duration::from_secs(3600);
                let roots: Vec<PathBuf> = self
                    .shots
                    .iter()
                    .map(|s| s.root.clone())
                    .chain(self.collect.iter().map(|c| c.root.clone()))
                    .collect();
                if !roots.is_empty() {
                    std::thread::spawn(move || roots.iter().for_each(|r| crate::shots::prune(r)));
                }
            }
            self.layer.tick();
        }
        self.flush_state();
    }

    fn on_event(&mut self, e: Event) {
        match e {
            Event::Lines(lines) => self.on_lines(lines),
            Event::Manual => {
                if let Some(c) = self.collect.as_mut().filter(|_| self.presence.inside) {
                    c.force();
                }
                self.capture_now()
            }
            Event::Thermo => self.layer.thermo(),
            Event::Reply(r) => self.on_reply(r),
            Event::OverlayReady => self.layer.on_ready(),
            Event::SetEnabled(on) => self.set_enabled(on),
            Event::SetSaveShots(on) => {
                self.shots = on.then(|| Shots::new(self.paths.shots.clone()))
            }
            Event::SetCollect(on) => {
                self.collect = on.then(|| Collect::new(self.paths.collect.clone()));
                if !on && !self.enabled {
                    self.stream.stop();
                }
            }
        }
    }

    fn set_enabled(&mut self, on: bool) {
        self.enabled = on;
        if !on {
            self.triggers.cancel();
            self.layer.clear();
            self.stream.stop();
            self.queue.clear();
        }
    }

    /// Монитор игры: найденный по доске, а пока не найден — под курсором.
    fn game_display(&self) -> Option<Display> {
        self.game_display.or_else(display_under_cursor)
    }

    fn on_lines(&mut self, lines: Vec<String>) {
        if !self.enabled {
            if self.collect.is_some() {
                self.collect_only(&lines);
            }
            return;
        }
        let change = self.presence.feed(&lines);
        if let Some(c) = &mut self.collect {
            c.lines(&lines);
            if change == Some(PresenceChange::Leave) {
                crate::log(&format!("[тюрьма] сбор: снято экранов {}", c.saved));
                c.reset();
            }
        }
        if self.presence.inside {
            if let Some(d) = self.game_display() {
                // В тюрьме — поток экрана открыт заранее, к первому нужному кадру.
                if let Err(e) = self.stream.ensure(d.hmonitor) {
                    crate::log(&format!("[тюрьма] {e}"));
                }
                // Игрок сходил: прежняя стрелка уже неверна — убрать сразу и показать «смотрю…».
                if lines.iter().any(|l| player_action(l)) {
                    self.stream.boost();
                    self.layer.thinking(&d);
                    // Ответы на прежние кадры больше не показывать.
                    self.next_id += 1;
                    self.latest.store(self.next_id, Ordering::Relaxed);
                }
            }
        }
        let mut ready = vec![];
        self.triggers.feed(&lines, Instant::now(), &mut ready);
        for s in ready {
            self.capture(s, false);
        }
        // Доска закрыта (итог забега, финальная комната) — советовать нечего:
        // пересъёмка и «смотрю…» по последнему ходу больше не нужны.
        if lines.iter().any(|l| l.contains("S16BoardMain Destory") || l.contains("S16GamePlayCheckOut Run")) {
            self.board_closed = true;
            self.retry_at = None;
            self.layer.clear();
        } else if lines.iter().any(|l| l.contains("S16BoardMain Run")) {
            self.board_closed = false;
        }
        if change == Some(PresenceChange::Leave) {
            // Вышли из тюрьмы: захват не нужен, ждущие кадры — тоже.
            crate::log("[тюрьма] вышли из тюрьмы — поток экрана закрыт");
            self.retry_at = None;
            self.triggers.cancel();
            self.queue.retain(|q| q.1);
            self.stream.stop();
            self.layer.clear();
        }
    }

    /// Подсказки выключены, сбор экранов — нет: только присутствие и поток экрана.
    fn collect_only(&mut self, lines: &[String]) {
        let change = self.presence.feed(lines);
        self.triggers.feed(lines, Instant::now(), &mut vec![]);
        self.triggers.cancel();
        if let Some(c) = &mut self.collect {
            c.lines(lines);
        }
        if self.presence.inside {
            if let Some(d) = self.game_display() {
                if let Err(e) = self.stream.ensure(d.hmonitor) {
                    crate::log(&format!("[тюрьма] {e}"));
                }
            }
        }
        if change == Some(PresenceChange::Leave) {
            if let Some(c) = &mut self.collect {
                crate::log(&format!("[тюрьма] сбор: снято экранов {}", c.saved));
                c.reset();
            }
            self.stream.stop();
        }
    }

    /// Снять кадр сейчас (хоткей): экран определится по картинке.
    fn capture_now(&mut self) {
        if !self.enabled {
            return;
        }
        let shot = PrisonShot {
            kind: ShotKind::Board,
            fresh: false,
            after: Default::default(),
            treasure_view: false,
            rerolled: false,
            treasure_closed: false,
            theme: None,
            day: self.triggers.last_day(),
            lines: vec![],
            settled: None,
        };
        self.capture(shot, true);
    }

    fn capture(&mut self, shot: PrisonShot, manual: bool) {
        self.capture_with(shot, manual, 0);
        // Кадры, пришедшие во время снимка, — по одному.
        while !self.queue.is_empty() {
            let (s, m) = self.queue.remove(0);
            self.capture_with(s, m, 0);
        }
    }

    fn capture_with(&mut self, shot: PrisonShot, manual: bool, retry: u32) {
        if !self.enabled || (!manual && !self.presence.inside) {
            return;
        }
        let Some(display) = (if manual {
            display_under_cursor()
        } else {
            self.game_display()
        }) else {
            return;
        };
        if shot.kind == ShotKind::Treasure && !manual {
            // Окно подарков: рамка ставится по месту карточки — без снимка.
            self.treasure_seq += 1;
            self.queue.retain(|q| q.1);
            self.post(shot, false, None, display, retry);
            return;
        }
        let t0 = Instant::now();
        let seq = self.treasure_seq;
        // Окно стрелок видно — экран пересобирается и поток получает свежие кадры.
        self.layer.thinking(&display);
        if let Err(e) = self.stream.ensure(display.hmonitor) {
            crate::log(&format!("[тюрьма] {e}"));
            self.layer.clear();
            return;
        }
        // Статичный экран новых кадров не даёт: годится последний, снятый после строки лога.
        let line_at = shot.lines.last().and_then(|l| stamp_ms(l));
        let ok_age = if manual {
            Duration::from_secs(60)
        } else {
            match line_at {
                // Метки лога игры — в UTC.
                Some(at) => Duration::from_millis(
                    (chrono::Utc::now().timestamp_millis() - at - STATIC_AFTER_LINE_MS).max(0)
                        as u64,
                ),
                None => Duration::ZERO,
            }
        };
        let (stable, max) = if manual {
            (Duration::ZERO, Duration::from_millis(600))
        } else {
            (STABLE, STABLE_MAX)
        };
        let mut frame = self.stream.grab(stable, max, ok_age);
        if matches!(frame, Err(Miss::Silent)) {
            // Поток молчит — открыть заново и спросить ещё раз.
            self.stream.stop();
            if self.stream.ensure(display.hmonitor).is_ok() {
                frame = self
                    .stream
                    .grab(Duration::ZERO, Duration::from_millis(1200), ok_age);
            }
        }
        let mut i = 0;
        while matches!(frame, Err(Miss::Stale)) && i < NUDGES {
            self.layer.nudge(&display);
            frame = self
                .stream
                .grab(Duration::ZERO, Duration::from_millis(900), ok_age);
            i += 1;
        }
        let frame = match frame {
            Ok(f) => f,
            Err(m) => {
                crate::log(&format!(
                    "[тюрьма] кадр не получен за {} мс ({m:?})",
                    t0.elapsed().as_millis()
                ));
                if seq == self.treasure_seq {
                    self.layer.clear();
                }
                return;
            }
        };
        // Поток застыл: тот же кадр, что и прошлый, хотя открылось новое окно — переоткрыть.
        let hash = frame_hash(frame.width, frame.height, &frame.data);
        if self.last_posted.as_ref() == Some(&hash) && (manual || screen_changed(&shot.lines)) {
            crate::log(&format!("[тюрьма] поток застыл: тот же кадр, что и прошлый — поток заново"));
            self.stream.stop();
        }
        // Пока снимали, открылось окно подарков: доску учесть, совет — по подаркам.
        let late = seq != self.treasure_seq;
        if (late && shot.kind != ShotKind::Board) || (!manual && !self.presence.inside) {
            if !late {
                self.layer.clear();
            }
            return;
        }
        self.last_posted = Some(hash);
        let mut s = shot;
        if !manual {
            s.settled = Some(self.stream.last_settled());
        }
        if late {
            s.kind = ShotKind::Board;
        }
        self.post_frame(s, manual, frame, display, retry, late);
    }

    fn post(
        &mut self,
        shot: PrisonShot,
        manual: bool,
        frame: Option<Arc<Image>>,
        display: Display,
        retry: u32,
    ) {
        self.post_inner(shot, manual, frame, display, retry, false);
    }

    fn post_frame(
        &mut self,
        shot: PrisonShot,
        manual: bool,
        frame: Arc<Image>,
        display: Display,
        retry: u32,
        as_treasure: bool,
    ) {
        self.post_inner(shot, manual, Some(frame), display, retry, as_treasure);
    }

    fn post_inner(
        &mut self,
        shot: PrisonShot,
        manual: bool,
        frame: Option<Arc<Image>>,
        display: Display,
        retry: u32,
        as_treasure: bool,
    ) {
        self.next_id += 1;
        let id = self.next_id;
        self.latest.store(id, Ordering::Relaxed);
        self.sent.retain(|s| s.0 + 20 >= id);
        self.sent.push((
            id,
            Sent {
                shot: shot.clone(),
                manual,
                retry,
            },
            display,
        ));
        let kind = if manual {
            "manual"
        } else {
            kind_name(shot.kind)
        };
        if let Some(s) = self.shots.as_mut() {
            s.note(id, kind, &shot, frame.clone());
        }
        self.display = Some(display);
        let msg = Shot {
            id,
            kind: if manual { ShotKind::Manual } else { shot.kind },
            fresh: shot.fresh,
            after: shot.after,
            treasure_view: shot.treasure_view,
            rerolled: shot.rerolled,
            treasure_closed: shot.treasure_closed,
            theme: shot.theme,
            day: shot.day,
            as_treasure,
            width: frame
                .as_ref()
                .map(|f| f.width)
                .unwrap_or((display.w) as usize),
            height: frame
                .as_ref()
                .map(|f| f.height)
                .unwrap_or((display.h) as usize),
            budget_ms: SOLVE_BUDGET_MS,
            settled: shot.settled,
            events: gorn_prison::logevents::compact(&shot.lines),
        };
        crate::log(&format!(
            "[тюрьма] кадр {id} ({kind}{})",
            if as_treasure {
                ", к подаркам"
            } else {
                ""
            }
        ));
        let jobs = self.ensure_worker();
        let _ = jobs.send(Job { shot: msg, frame });
    }

    /// Поток советника: пониженный приоритет, пул решателя ≤ 4 потоков.
    fn ensure_worker(&mut self) -> Sender<Job> {
        if let Some(j) = &self.jobs {
            return j.clone();
        }
        let (jtx, jrx) = std::sync::mpsc::channel::<Job>();
        let tx = self.tx.clone();
        let latest = self.latest.clone();
        let state = std::fs::read_to_string(&self.paths.state)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok());
        let near = self.near;
        let facts = crate::facts::spawn(&self.paths.facts);
        std::thread::Builder::new()
            .name("горн-советник".into())
            .spawn(move || {
                below_normal();
                let mut adv = OwnAdvisor::new();
                // Цепочка подарков (этап 7): остаток выборов по призмам.
                adv.chain_gifts = true;
                adv.init(state.as_ref(), near);
                // Карта выбора, не узнанная по картинке, — по названию (OCR Windows,
                // как у Vorax: WinRT в этом потоке).
                unsafe {
                    let _ = windows::Win32::System::WinRT::RoInitialize(windows::Win32::System::WinRT::RO_INIT_MULTITHREADED);
                }
                if let Ok(ocr) = crate::ocr::Ocr::new() {
                    adv.card_text = Some(Box::new(move |f: &Frame, b| {
                        let img = crate::ocr::light_text(f.data, f.width, f.height, b);
                        let text = ocr.read(&img).unwrap_or_default();
                        crate::log(&format!("[тюрьма] текст на кадре: «{}»", text.replace('\n', " ")));
                        text
                    }));
                }
                let threads = std::thread::available_parallelism()
                    .map(|n| n.get())
                    .unwrap_or(4)
                    .saturating_sub(2)
                    .clamp(1, MAX_SOLVE_THREADS);
                if threads > 1 {
                    adv.pool = rayon::ThreadPoolBuilder::new()
                        .num_threads(threads)
                        .thread_name(|i| format!("горн-решатель-{i}"))
                        .start_handler(|_| below_normal())
                        .build()
                        .ok();
                }
                adv.latest = Some(latest);
                let tx2 = tx.clone();
                adv.sink = Some(Box::new(move |r: &Reply| {
                    let _ = tx2.send(Event::Reply(r.clone()));
                }));
                if let Some(ftx) = facts {
                    adv.facts = Some(Box::new(move |f| {
                        let _ = ftx.send(f);
                    }));
                }
                for job in jrx {
                    let frame = job.frame.as_ref().map(|f| Frame {
                        width: f.width,
                        height: f.height,
                        data: &f.data,
                    });
                    adv.handle(&job.shot, frame.as_ref());
                    adv.replies.clear();
                }
            })
            .expect("поток советника");
        self.jobs = Some(jtx.clone());
        jtx
    }

    fn on_reply(&mut self, r: Reply) {
        match &r {
            Reply::State(m) => {
                self.state = Some(m.clone());
                if self.state_due.is_none() {
                    self.state_due = Some(Instant::now() + Duration::from_secs(1));
                }
                return;
            }
            Reply::Near(g) => {
                self.near = Some(*g);
                self.save_display();
                return;
            }
            _ => {}
        }
        let (id, done) = match &r {
            Reply::Hint { id, phase, .. } => (*id, *phase == "final"),
            Reply::Settled { id, .. } | Reply::Clear { id } => (*id, true),
            _ => return,
        };
        let sent = self.sent.iter().find(|s| s.0 == id).cloned();
        // Сколько прошло от хода игрока (метка строки лога, UTC) до этого ответа:
        // так видно, успевает ли стрелка до следующего хода.
        let after_click = sent.as_ref().and_then(|s| {
            let at = s.1.shot.lines.iter().rev().find(|l| player_action(l)).and_then(|l| stamp_ms(l))?;
            Some(chrono::Utc::now().timestamp_millis() - at)
        });
        let mut json = r.to_json();
        if let Some(ms) = after_click {
            json["afterClickMs"] = ms.into();
        }
        if let Some(s) = self.shots.as_mut() {
            s.reply(id, json, done);
        }
        if let (Reply::Hint { phase, .. }, Some(ms)) = (&r, after_click) {
            if id == self.next_id && self.hint_logged != id {
                self.hint_logged = id;
                crate::log(&format!("[тюрьма] подсказка ({phase}) через {ms} мс после хода — кадр {id}"));
            }
        }
        // Подсказка пришла с монитора — это монитор игры.
        if let (Reply::Hint { .. }, Some((_, _, d))) = (&r, &sent) {
            if self.game_display.map(|g| g.hmonitor) != Some(d.hmonitor) {
                self.game_display = Some(*d);
                self.save_display();
            }
        }
        if id == self.next_id {
            if let Some(d) = self.display {
                match &r {
                    Reply::Hint {
                        hint,
                        w,
                        h,
                        live,
                        phase,
                        ..
                    } => {
                        self.layer.show(&d, hint, *w, *h, *live, phase);
                        self.stale_at = Some(Instant::now() + STALE);
                    }
                    Reply::Settled { live, .. } => self.layer.settle(*live),
                    Reply::Clear { .. } => self.layer.clear(),
                    _ => {}
                }
            }
        }
        // Автокадр не распознался (анимация ещё шла) — переснять, до двух раз.
        if done {
            if let (Reply::Clear { .. }, Some((_, s, _))) = (&r, &sent) {
                if !s.manual
                    && s.retry < RETRIES
                    && !s.shot.treasure_closed
                    && matches!(s.shot.kind, ShotKind::Board | ShotKind::Instruments)
                    && id == self.next_id
                    && self.presence.inside
                    && !self.board_closed
                {
                    if let Some(d) = self.display {
                        self.layer.thinking(&d);
                    }
                    self.retry_at = Some((
                        Instant::now() + Duration::from_millis(RETRY_MS * (s.retry as u64 + 1)),
                        id,
                        s.clone(),
                    ));
                }
            }
        }
    }

    fn save_display(&self) {
        let d = self.game_display;
        let near = self.near.map(|g| json!({ "width": g.width, "height": g.height, "s": g.s, "tx": g.tx, "ty": g.ty, "score": g.score }));
        let v = json!({ "x": d.map(|d| d.x), "y": d.map(|d| d.y), "near": near });
        write_atomic(&self.paths.display, &v.to_string());
    }

    fn flush_state(&mut self) {
        self.state_due = None;
        if let Some(s) = &self.state {
            write_atomic(&self.paths.state, &s.to_string());
        }
    }
}

fn kind_name(k: ShotKind) -> &'static str {
    match k {
        ShotKind::Board => "board",
        ShotKind::Instruments => "instruments",
        ShotKind::Objective => "objective",
        ShotKind::Treasure => "treasure",
        ShotKind::Theme => "theme",
        ShotKind::Manual => "manual",
    }
}

/// Записать файл через временный: оборванная запись не портит прежний.
fn write_atomic(path: &std::path::Path, text: &str) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, text).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

/// Приоритет потока ниже обычного: игра не должна проседать.
fn below_normal() {
    use windows::Win32::System::Threading::{
        GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL,
    };
    unsafe {
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
    }
}

/// `gorn --prison-cards отчёт.txt кадр.png…`: названия всех трёх карт выбора
/// орудий и заголовок окна события текстом (OCR) — проверка запасного пути без игры.
pub fn read_cards(out: &str, paths: &[String]) {
    unsafe {
        let _ = windows::Win32::System::WinRT::RoInitialize(windows::Win32::System::WinRT::RO_INIT_MULTITHREADED);
    }
    let Ok(ocr) = crate::ocr::Ocr::new() else {
        let _ = std::fs::write(out, "OCR нет\n");
        return;
    };
    let mut report = format!("OCR: {}\n", ocr.language);
    for path in paths {
        let Some(img) = crate::vorax::decode_png(std::path::Path::new(path)) else {
            report += &format!("{path}: не читается\n");
            continue;
        };
        let frame = Frame { width: img.width, height: img.height, data: &img.data };
        let geo = gorn_prison::recognize::default_geometry(&frame);
        report += &format!("{path}\n");
        for card in 0..3 {
            for (k, b) in gorn_prison::recognize::pick::title_boxes(&geo, card).into_iter().enumerate() {
                let text = ocr.read(&crate::ocr::light_text(&img.data, img.width, img.height, b)).unwrap_or_default();
                let id = gorn_prison::model::tool_by_title(&text).map(|t| gorn_prison::model::TOOL_IDS[t as usize]);
                report += &format!("  [{card}{}] {id:?} «{}»\n", if k == 1 { " под курсором" } else { "" }, text.replace('\n', " / "));
            }
        }
        let b = gorn_prison::recognize::pick::event_title_box(&geo);
        let text = ocr.read(&crate::ocr::light_text(&img.data, img.width, img.height, b)).unwrap_or_default();
        let chain = gorn_prison::model::is_chain_reaction(&text);
        report += &format!("  [событие] цепная реакция: {chain} «{}»\n", text.replace('\n', " / "));
    }
    let _ = std::fs::write(out, report);
}
