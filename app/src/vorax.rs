//! Vorax («Хирургическая подготовка»): подсказки поверх игры и запись кадров.
//!
//! Когда смотреть — правила `vorax-scan/src/live.rs` из DinoRaos/vorax-solver-rs
//! (MIT, см. crates/gorn-vorax/LICENSE): ответ сервера с новой доской
//! (`ProtoId = 5619`) или открытие экрана, затем ждём, пока карты доиграют
//! анимации. Кадр читается (gorn-vorax-scan: карты по картинке, названия —
//! OCR Windows), советник выбирает ход, окно `vorax` обводит карту и баки.
//! Карта ещё летит — ещё раз через 0,7 с (до трёх раз), как у эталона.
//! Ctrl+Alt+P на экране Vorax — кадр и совет вручную. Игра в окне (16:9 на
//! мониторе 21:9) — читается только клиентская область окна.
//!
//! Кадры — «Изображения/Горн Next — vorax/ДАТА» (с «Сохранять кадры»): PNG и
//! JSON на каждую доску, все строки лога, пока открыт экран; сверх 2 ГБ
//! старые дни удаляются. Что прочитано не наверняка (unknown::unknowns) —
//! ещё и в «незнакомое/ДАТА», без срока: по ним дополняются эталоны.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gorn_vorax_scan::pipeline::{Outcome, Pipeline};
use gorn_vorax_scan::pixels::Img;
use gorn_vorax_scan::unknown::unknowns;
use gorn_vorax_scan::{scan_data, scan_data_ru, ScanData};
use serde_json::{json, Value};
use tauri::AppHandle;
use windows::Win32::Foundation::{POINT, RECT};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetClientRect, GetForegroundWindow};

use crate::capture::{Image, ScreenStream};
use crate::ocr::Ocr;
use crate::overlay::{display_under_cursor, Display};
use crate::shots::{write_info, write_png};
use crate::vorax_layer::VoraxLayer;

pub enum Event {
    Lines(Vec<String>),
    Manual,
    /// «Сохранять кадры»
    SetSave(bool),
    /// «Подсказки в Vorax»
    SetHints(bool),
    /// страница окна отметок подписалась
    Ready,
}

// Ответ сервера после «Подтвердить» и обновления: новая доска.
const BOARD: &[&str] = &["ProtoId = 5619"];
// Экран открылся: первая доска и выбор начального инструмента.
const SCREEN: &[&str] = &["S13GamePlayMain Run", "S13GamePlay_C_StartGameBtn"];
const OVER: &[&str] = &[
    "ProtoId = 5621",
    "S13GamePlayMain Hide",
    "S13GamePlayReward_C_FightBtn",
    "S13GamePlayReward_C_GiveUpBtn",
];
/// Экран закрыт совсем: дальше не снимать и лог не писать.
const CLOSED: &str = "S13GamePlayMain Hide";
const ANIMATION: &str = "Play_UI_S13_KYZ_Card_";
const PLAYER_SOUNDS: &[&str] = &["KYZ_Card_Selected'", "KYZ_Card_Selected_Confirm", "KYZ_Card_Agentia_Use"];
const DONE: &str = "KYZ_Card_Figure_Refresh";
const PLAYER_ACTIONS: &[&str] =
    &["normal-click SelectBtn", "normal-click ComfirmBtn", "normal-click RefreshBtn", "normal-click RelicBtn"];
/// Ход сделан — доска сейчас сменится: прежние отметки прочь.
const MOVES: &[&str] = &["normal-click ComfirmBtn", "normal-click RefreshBtn"];
const DONE_DELAY: Duration = Duration::from_millis(150);
const SETTLE: Duration = Duration::from_millis(400);
const MAX_WAIT: Duration = Duration::from_millis(3000);
/// Карта ещё летела: посмотреть снова через столько, не больше RETRIES раз.
const RETRY: Duration = Duration::from_millis(700);
const RETRIES: u32 = 3;
/// Ход сделан, а новой доски за столько нет — игра его не приняла (25.10: «Подтвердить» без выбранного бака):
/// отметки обратно.
const UNDIM: Duration = Duration::from_millis(1500);
/// Новая иконка инструмента ещё не встала: взглянуть ещё раз.
const RELOOK: Duration = Duration::from_millis(1500);
/// Потолок кадров по дням (без «незнакомого»).
const MAX_BYTES: u64 = 2 << 30;
const UNKNOWN_DIR: &str = "незнакомое";

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Board,
    Screen,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Board => "board",
            Kind::Screen => "screen",
        }
    }
}

/// Доска сменилась — снять, когда анимации доиграют (`Settler` эталона).
#[derive(Clone, Copy)]
struct Pending {
    kind: Kind,
    t0: Instant,
    last_anim: Instant,
    done_at: Option<Instant>,
    acted: bool,
}

/// Экран Vorax открыт: открытие, возврат на него или новая доска (Горн запущен
/// посреди забега). Не любая строка `S13GamePlay` — после закрытия идут строки
/// экрана награды (`S13GamePlayReward …`), по ним запись открывалась снова.
fn opens(l: &str) -> bool {
    SCREEN.iter().any(|p| l.contains(p))
        || BOARD.iter().any(|p| l.contains(p))
        || (l.contains("CurRunView = ") && l.contains("S13GamePlayMainCtrl"))
}

/// Строка из тех, что касаются Vorax (в описание кадра).
fn related(l: &str) -> bool {
    l.contains("S13") || l.contains("ProtoId = 5619") || l.contains("ProtoId = 5621")
}

pub struct Vorax {
    root: PathBuf,
    save: bool,
    hints: bool,
    shelley: i32,
    /// Экран Vorax открыт (по логу).
    active: bool,
    over: bool,
    pending: Option<Pending>,
    display: Option<Display>,
    stream: ScreenStream,
    /// Строки Vorax с прошлого кадра.
    recent: Vec<String>,
    log: Option<std::fs::File>,
    count: u32,
    layer: VoraxLayer,
    /// OCR и чтение создаются в своём потоке (OCR — WinRT).
    ocr: Option<Ocr>,
    data: Option<&'static ScanData>,
    pipe: Option<Pipeline>,
    /// Посмотреть снова (карта летела): когда и с каким поводом; сколько раз уже.
    retry: Option<(Instant, Option<&'static str>)>,
    retries: u32,
    relook_at: Option<Instant>,
    /// Когда вернуть приглушённые отметки, если доска так и не сменится.
    undim_at: Option<Instant>,
    /// Последнее отложенное в «незнакомое» — одну доску дважды не копить.
    last_unknown: Option<String>,
}

impl Vorax {
    pub fn new(app: AppHandle, root: PathBuf, save: bool, hints: bool, shelley: i32) -> Self {
        Vorax {
            root,
            save,
            hints,
            shelley,
            active: false,
            over: false,
            pending: None,
            display: None,
            stream: ScreenStream::default(),
            recent: vec![],
            log: None,
            count: 0,
            layer: VoraxLayer::new(app),
            ocr: None,
            data: None,
            pipe: None,
            retry: None,
            retries: 0,
            relook_at: None,
            undim_at: None,
            last_unknown: None,
        }
    }

    pub fn run(mut self, rx: Receiver<Event>) {
        unsafe {
            let _ = windows::Win32::System::WinRT::RoInitialize(windows::Win32::System::WinRT::RO_INIT_MULTITHREADED);
        }
        self.ocr = match Ocr::new() {
            Ok(o) => {
                eprintln!("[vorax] названия карт — OCR Windows ({})", o.language);
                Some(o)
            }
            Err(e) => {
                eprintln!("[vorax] OCR Windows нет ({e}): карты только по картинке");
                None
            }
        };
        // Английский OCR — английский клиент: формы английского текста помогают картинке.
        let data = if self.ocr.as_ref().is_some_and(|o| !o.russian()) { scan_data() } else { scan_data_ru() };
        self.data = Some(data);
        self.pipe = Some(Pipeline::new(data, self.shelley));
        loop {
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(Event::Lines(lines)) => self.on_lines(&lines),
                Ok(Event::Manual) => {
                    if self.active {
                        self.retries = 0;
                        self.shoot("manual", None);
                    }
                }
                Ok(Event::SetSave(on)) => {
                    self.save = on;
                    if !self.working() {
                        self.close();
                    }
                }
                Ok(Event::SetHints(on)) => {
                    self.hints = on;
                    if !on {
                        self.layer.clear();
                    }
                    if !self.working() {
                        self.close();
                    }
                }
                Ok(Event::Ready) => self.layer.on_ready(),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            let now = Instant::now();
            if let Some(kind) = self.due(now) {
                self.retries = 0;
                self.retry = None;
                self.shoot(kind.name(), Some(kind.name()));
            } else if let Some((_, trigger)) = self.retry.filter(|(t, _)| now >= *t) {
                self.retry = None;
                self.shoot("again", trigger);
            } else if self.relook_at.is_some_and(|t| now >= t) {
                self.relook_at = None;
                self.shoot("relook", None);
            }
            if self.undim_at.is_some_and(|t| now >= t) {
                self.undim_at = None;
                self.layer.undim();
            }
        }
    }

    /// Есть ради чего смотреть на экран.
    fn working(&self) -> bool {
        self.save || self.hints
    }

    fn on_lines(&mut self, lines: &[String]) {
        if !self.working() {
            return;
        }
        let now = Instant::now();
        let start = |kind| Pending { kind, t0: now, last_anim: now, done_at: None, acted: false };
        for l in lines {
            if !self.active && opens(l) {
                self.open();
            }
            if !self.active {
                continue;
            }
            self.write_log(l);
            if related(l) {
                self.recent.push(l.clone());
            }
            if MOVES.iter().any(|m| l.contains(m)) {
                self.layer.dim();
                self.undim_at = Some(now + UNDIM);
            }
            if SCREEN.iter().any(|p| l.contains(p)) || BOARD.iter().any(|p| l.contains(p)) || OVER.iter().any(|p| l.contains(p)) {
                self.undim_at = None;
            }
            if SCREEN.iter().any(|p| l.contains(p)) {
                self.over = false;
                self.pending = Some(start(Kind::Screen));
            } else if OVER.iter().any(|p| l.contains(p)) {
                self.over = true;
                self.pending = None;
                self.retry = None;
                self.layer.clear();
                if l.contains(CLOSED) {
                    self.close();
                }
            } else if BOARD.iter().any(|p| l.contains(p)) {
                if !self.over {
                    self.pending = Some(start(Kind::Board));
                }
            } else if let Some(p) = &mut self.pending {
                if PLAYER_ACTIONS.iter().any(|a| l.contains(a)) {
                    p.acted = true;
                } else if l.contains(DONE) {
                    p.done_at = Some(now);
                } else if l.contains(ANIMATION) && !PLAYER_SOUNDS.iter().any(|s| l.contains(s)) {
                    p.last_anim = now;
                }
            }
        }
    }

    fn due(&mut self, now: Instant) -> Option<Kind> {
        let p = self.pending?;
        let settled = p.acted
            || p.done_at.is_some_and(|d| now - d >= DONE_DELAY)
            || now - p.last_anim >= SETTLE;
        if settled || now - p.t0 >= MAX_WAIT {
            self.pending = None;
            return Some(p.kind);
        }
        None
    }

    fn day_dir(&self) -> PathBuf {
        self.root.join(chrono::Local::now().format("%Y-%m-%d").to_string())
    }

    fn open(&mut self) {
        // Экран открывают кликом в игре: игра — на мониторе под курсором.
        self.display = display_under_cursor();
        let Some(d) = self.display else { return };
        if let Err(e) = self.stream.ensure(d.hmonitor) {
            eprintln!("[vorax] {e}");
            return;
        }
        if self.save {
            prune(&self.root);
            let dir = self.day_dir();
            let _ = std::fs::create_dir_all(&dir);
            self.log = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("log.txt")).ok();
        }
        self.active = true;
        self.over = false;
        self.recent.clear();
        eprintln!("[vorax] экран открыт (подсказки: {}, кадры: {})", self.hints, self.save);
    }

    fn close(&mut self) {
        if self.active {
            eprintln!("[vorax] экран закрыт: кадров {}", self.count);
        }
        self.active = false;
        self.pending = None;
        self.retry = None;
        self.relook_at = None;
        self.undim_at = None;
        self.log = None;
        self.count = 0;
        self.layer.clear();
        self.stream.stop();
    }

    fn write_log(&mut self, l: &str) {
        if let Some(f) = &mut self.log {
            let _ = writeln!(f, "{l}");
        }
    }

    /// Снять экран, прочитать и посоветовать. `kind` — в имя кадра; `trigger` — повод для учёта забега
    /// ("screen" — открылся новый).
    fn shoot(&mut self, kind: &str, trigger: Option<&'static str>) {
        let frame = match self.stream.grab(
            Duration::from_millis(250),
            Duration::from_millis(2000),
            Duration::from_secs(3),
        ) {
            Ok(img) => img,
            Err(e) => {
                eprintln!("[vorax] кадра нет ({kind}): {e:?}");
                return;
            }
        };
        // Игра в окне (16:9 на мониторе 21:9) — кадр только окна, без рабочего стола.
        let window = self.display.and_then(|d| game_client(&d, &frame));
        let img = match window {
            Some(r) => Arc::new(cut(&frame, r)),
            None => frame.clone(),
        };
        let offset = window.map_or((0.0, 0.0), |r| (r[0] as f64, r[1] as f64));
        let read = if self.hints { self.advise(&img, frame.width, offset, kind, trigger) } else { Value::Null };
        if !self.save {
            return;
        }
        let now = chrono::Local::now();
        let base = self.day_dir().join(format!("{}-{kind}", now.format("%H%M%S-%3f")));
        // Метка в логе: к какому месту строк относится кадр.
        let name = base.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        self.write_log(&format!("[горн vorax] кадр {name}"));
        let info = json!({
            "kind": kind,
            "settled": self.stream.last_settled(),
            "size": [img.width, img.height],
            "monitor": self.display.map(|d| json!({ "w": d.w, "h": d.h, "scale": d.scale })),
            // Где в кадре монитора было окно игры (пиксели кадра), если вырезано.
            "window": window,
            "lines": std::mem::take(&mut self.recent),
            "read": read,
        });
        save(&base, &info, img);
        self.count += 1;
    }

    /// Прочитать кадр (окно игры) и показать совет. Возвращает прочитанное — в описание кадра.
    fn advise(&mut self, img: &Arc<Image>, frame_w: usize, offset: (f64, f64), kind: &str, trigger: Option<&'static str>) -> Value {
        let (Some(pipe), Some(data), Some(d)) = (self.pipe.as_mut(), self.data, self.display) else { return Value::Null };
        let src = Img { width: img.width, height: img.height, data: img.data.clone() };
        let t0 = Instant::now();
        let outcome = match &self.ocr {
            Some(o) => {
                let mut read = |i: &Img| o.read(i).unwrap_or_default();
                pipe.run(&src, trigger, Some(&mut read))
            }
            None => pipe.run(&src, trigger, None),
        };
        let wants_relook = pipe.wants_relook();
        match outcome {
            Outcome::Advice { reading, advice, plan, origin, read_ms, texts } => {
                self.retries = 0;
                let screen = plan.mapped(origin.0 + offset.0, origin.1 + offset.1, 1.0);
                self.layer.show(&d, &screen, frame_w);
                if wants_relook {
                    self.relook_at = Some(Instant::now() + RELOOK);
                }
                let why = unknowns(&reading, &texts, data);
                eprintln!(
                    "[vorax] {kind}: чтение {read_ms:.0} мс, совет {} мс, всего {} мс → {}{}",
                    advice.ms,
                    t0.elapsed().as_millis(),
                    plan.pill.as_ref().map_or("—".to_string(), |p| format!("{} · {}", p.main, p.sub)),
                    if why.is_empty() { String::new() } else { format!("; не узнано: {}", why.len()) }
                );
                let read = json!({ "state": reading.state, "best": advice.best, "pill": plan.pill, "texts": texts });
                if !why.is_empty() {
                    let key = format!("{:?}{:?}{why:?}", reading.state.round, reading.state.hand);
                    if self.last_unknown.as_ref() != Some(&key) {
                        self.last_unknown = Some(key);
                        self.keep_unknown(img, kind, &why, json!({ "reading": reading, "texts": texts }));
                    }
                }
                read
            }
            Outcome::Only { plan, origin, name, text } => {
                self.retries = 0;
                self.layer.show(&d, &plan.mapped(origin.0 + offset.0, origin.1 + offset.1, 1.0), frame_w);
                eprintln!("[vorax] {kind}: один инструмент — {}", name.as_deref().unwrap_or("?"));
                let texts = vec![text.clone()];
                if name.is_none() {
                    self.keep_unknown(img, kind, &[format!("один инструмент не узнан, OCR: «{}»", text.trim())], json!({ "texts": texts }));
                }
                json!({ "outcome": "one", "name": name, "texts": texts })
            }
            Outcome::Animating => {
                if self.retries < RETRIES {
                    self.retries += 1;
                    self.retry = Some((Instant::now() + RETRY, trigger));
                } else {
                    self.retries = 0;
                    self.layer.clear();
                }
                json!({ "outcome": "animating" })
            }
            Outcome::Closing => {
                self.layer.clear();
                json!({ "outcome": "closing" })
            }
            Outcome::NotGame => {
                self.layer.clear();
                // Лог говорит «новая доска», а экран не узнан — его тоже в эталоны.
                if trigger.is_some() {
                    self.keep_unknown(img, kind, &["экран не узнан".to_string()], json!({}));
                }
                json!({ "outcome": "notgame" })
            }
        }
    }

    /// Отложить кадр в «незнакомое»: PNG и JSON с тем, что не узнано.
    fn keep_unknown(&self, img: &Arc<Image>, kind: &str, why: &[String], detail: Value) {
        let now = chrono::Local::now();
        let base = self
            .root
            .join(UNKNOWN_DIR)
            .join(now.format("%Y-%m-%d").to_string())
            .join(format!("{}-{kind}", now.format("%H%M%S-%3f")));
        eprintln!("[vorax] в незнакомое: {}", why.join("; "));
        let mut info = json!({ "why": why, "size": [img.width, img.height] });
        if let (Some(a), Some(b)) = (info.as_object_mut(), detail.as_object()) {
            a.extend(b.clone());
        }
        save(&base, &info, img.clone());
    }
}

/// PNG в фоне, описание — сразу.
fn save(base: &Path, info: &Value, img: Arc<Image>) {
    write_info(base, info);
    let png = base.with_extension("png");
    std::thread::spawn(move || {
        if let Err(e) = write_png(&png, &img) {
            eprintln!("[vorax] кадр не сохранён: {e}");
        }
    });
}

/// Сверх MAX_BYTES — удалить самые старые дни (кроме «незнакомого»).
fn prune(root: &Path) {
    let Ok(rd) = std::fs::read_dir(root) else { return };
    let mut days: Vec<(PathBuf, u64)> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.file_name().is_some_and(|n| n != UNKNOWN_DIR))
        .map(|p| {
            let size = std::fs::read_dir(&p)
                .map(|r| r.flatten().filter_map(|e| e.metadata().ok()).map(|m| m.len()).sum())
                .unwrap_or(0);
            (p, size)
        })
        .collect();
    days.sort();
    let mut total: u64 = days.iter().map(|d| d.1).sum();
    for (dir, size) in days {
        if total <= MAX_BYTES {
            break;
        }
        if std::fs::remove_dir_all(&dir).is_ok() {
            eprintln!("[vorax] очистка кадров: удалён день {}", dir.display());
            total -= size;
        }
    }
}

/// `gorn --vorax-read отчёт.txt кадр.png…`: кадры по порядку, как в забеге, — через OCR, чтение и советника;
/// что прочитано, совет и не узнанное — в отчёт (у приложения нет консоли). Проверка без игры.
pub fn read_files(out: &str, paths: &[String]) {
    unsafe {
        let _ = windows::Win32::System::WinRT::RoInitialize(windows::Win32::System::WinRT::RO_INIT_MULTITHREADED);
    }
    let ocr = Ocr::new().ok();
    let data = if ocr.as_ref().is_some_and(|o| !o.russian()) { scan_data() } else { scan_data_ru() };
    let mut pipe = Pipeline::new(data, 4);
    let mut report = format!("OCR: {}\n", ocr.as_ref().map_or("нет".to_string(), |o| o.language.clone()));
    for path in paths {
        let Some(src) = decode_png(Path::new(path)) else {
            report += &format!("{path}: не читается\n");
            continue;
        };
        let t0 = Instant::now();
        let outcome = match &ocr {
            Some(o) => {
                let mut read = |i: &Img| o.read(i).unwrap_or_default();
                pipe.run(&src, None, Some(&mut read))
            }
            None => pipe.run(&src, None, None),
        };
        let ms = t0.elapsed().as_millis();
        report += &match outcome {
            Outcome::Advice { reading, plan, origin, texts, .. } => {
                let marks = serde_json::to_string(&plan.mapped(origin.0, origin.1, 1.0)).unwrap_or_default();
                let sure: Vec<bool> = (0..reading.state.hand.len()).map(|j| reading.fields.get(&format!("hand{j}")).is_some_and(|f| f.sure)).collect();
                format!(
                    "{path} ({ms} мс): {:?} раунд {:?}, рука {:?} {sure:?}, инструменты {:?}\n  → {}\n  не узнано: {:?}\n  отметки: {marks}\n",
                    reading.state.phase,
                    reading.state.round,
                    reading.state.hand,
                    reading.state.instruments,
                    plan.pill.map_or("—".to_string(), |p| format!("{} · {}", p.main, p.sub)),
                    unknowns(&reading, &texts, data),
                )
            }
            Outcome::Only { plan, name, .. } => format!(
                "{path} ({ms} мс): один инструмент {name:?}\n  → {}\n",
                plan.pill.map_or("—".to_string(), |p| format!("{} · {}", p.main, p.sub))
            ),
            Outcome::Animating => format!("{path}: карта летит\n"),
            Outcome::Closing => format!("{path}: забег закрывается\n"),
            Outcome::NotGame => format!("{path}: не экран Vorax\n"),
        };
    }
    let _ = std::fs::write(out, report);
}

fn decode_png(path: &Path) -> Option<Img> {
    let mut dec = png::Decoder::new(std::fs::File::open(path).ok()?);
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut r = dec.read_info().ok()?;
    let mut buf = vec![0; r.output_buffer_size()];
    let info = r.next_frame(&mut buf).ok()?;
    let (w, h) = (info.width as usize, info.height as usize);
    let ch = info.color_type.samples();
    let mut img = Img::new(w, h);
    for i in 0..w * h {
        let p = &buf[i * ch..i * ch + ch];
        let px = match ch {
            1 => [p[0], p[0], p[0], 255],
            2 => [p[0], p[0], p[0], p[1]],
            3 => [p[0], p[1], p[2], 255],
            _ => [p[0], p[1], p[2], p[3]],
        };
        img.data[i * 4..i * 4 + 4].copy_from_slice(&px);
    }
    Some(img)
}

/// Клиентская область окна игры (на переднем плане) в пикселях кадра монитора:
/// x0, y0, x1, y1. None — игры впереди нет или она во весь монитор.
fn game_client(d: &Display, img: &Image) -> Option<[usize; 4]> {
    unsafe {
        let hwnd = GetForegroundWindow();
        let mut class = [0u16; 64];
        let n = GetClassNameW(hwnd, &mut class) as usize;
        if String::from_utf16_lossy(&class[..n]) != "UnrealWindow" {
            return None;
        }
        let mut r = RECT::default();
        GetClientRect(hwnd, &mut r).ok()?;
        let mut p = POINT { x: 0, y: 0 };
        let _ = ClientToScreen(hwnd, &mut p);
        // Кадр мог быть уменьшен: пиксели монитора → пиксели кадра.
        let k = img.width as f64 / d.w as f64;
        let px = |v: i32, max: usize| ((v as f64 * k).round().max(0.0) as usize).min(max);
        let x0 = px(p.x - d.x, img.width);
        let y0 = px(p.y - d.y, img.height);
        let x1 = px(p.x - d.x + r.right, img.width);
        let y1 = px(p.y - d.y + r.bottom, img.height);
        if x1 <= x0 + 100 || y1 <= y0 + 100 {
            return None;
        }
        let whole = x0 == 0 && y0 == 0 && x1 == img.width && y1 == img.height;
        (!whole).then_some([x0, y0, x1, y1])
    }
}

/// Прямоугольник из кадра.
fn cut(img: &Image, [x0, y0, x1, y1]: [usize; 4]) -> Image {
    let w = x1 - x0;
    let mut data = Vec::with_capacity(w * (y1 - y0) * 4);
    for y in y0..y1 {
        let row = (y * img.width + x0) * 4;
        data.extend_from_slice(&img.data[row..row + w * 4]);
    }
    Image::whole(w, y1 - y0, data)
}

#[cfg(test)]
mod tests {
    use super::opens;

    /// Строки из забега 25.10: запись открывается на экране Vorax, но не на
    /// экране награды и не на закрытии.
    #[test]
    fn opens_only_on_the_vorax_screen() {
        for l in [
            "[Game] S13GamePlayMain Run",
            "[Game] PageStack@ CurRunView = 6650_S13GamePlayMainCtrl",
            "[Game] PushMgr:RecvPushCombineMsg ProtoId = 5619, SynId = 283 ",
        ] {
            assert!(opens(l), "{l}");
        }
        for l in [
            "[Game] UILevel@ CheckLoadSubLevel S13GamePlayReward S13GamePlayMain",
            "[Game] S13GamePlayReward Run",
            "[Game] S13GamePlayMainCtrl Calling OnLeaveHide",
            "[Game] S13GamePlayReward Destory",
            "[Game] S13GamePlayMain Destory",
            "[Game] PushMgr:RecvPushCombineMsg ProtoId = 5621, SynId = 309 ",
        ] {
            assert!(!opens(l), "{l}");
        }
    }
}
