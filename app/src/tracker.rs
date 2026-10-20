//! Трекер в приложении: движок (`gorn_core::engine`) в своём потоке, дочитывание
//! лога, пульс 5 с и автоповтор поиска игры. Окна получают срез событием
//! `snapshot` — там же, где Горн на Electron шлёт его из main-процесса.

use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gorn_core::engine::{CatalogueItem, Engine, EngineOptions, NameLanguage};
use gorn_core::labels::Lang;
use gorn_core::tailer::{TailEvent, Tailer, TailerHandle, TailerOptions};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

/// Куда уходит каждый новый срез (раздача по окнам — в layers.rs).
pub type Sink = Arc<dyn Fn(&Value) + Send + Sync>;

/// Работа над движком в его потоке.
pub type Job = Box<dyn FnOnce(&mut Engine) + Send>;

enum Msg {
    Job(Job),
    Lines(Vec<String>),
    Rotate,
    Tick,
    Boot,
}

/// Ручка для команд: всё, что трогает движок, идёт в его поток.
#[derive(Clone)]
pub struct Tracker {
    tx: Sender<Msg>,
    /// Последний срез — для `snapshot:get` при первой отрисовке окна.
    pub latest: Arc<Mutex<Value>>,
    /// Строки лога — для подсказок в тюрьме (им нужны строки интерфейса игры).
    lines_sink: Arc<Mutex<Option<Sender<Vec<String>>>>>,
    /// Папка логов, указанная вручную; None — автопоиск.
    log_dir: Arc<Mutex<Option<PathBuf>>>,
}

pub struct Config {
    pub data_dir: PathBuf,
    pub items: Vec<CatalogueItem>,
    pub options: EngineOptions,
    pub log_dir: Option<PathBuf>,
    pub sink: Sink,
}

pub fn name_language(s: Option<&str>) -> NameLanguage {
    match s {
        Some("en") => NameLanguage::En,
        Some("both") => NameLanguage::Both,
        _ => NameLanguage::Ru,
    }
}

pub fn ui_lang(s: Option<&str>) -> Lang {
    if s == Some("en") {
        Lang::En
    } else {
        Lang::Ru
    }
}

impl Tracker {
    pub fn start(app: AppHandle, cfg: Config) -> Tracker {
        let (tx, rx) = channel::<Msg>();
        let t = Tracker {
            tx: tx.clone(),
            latest: Arc::new(Mutex::new(Value::Null)),
            lines_sink: Arc::new(Mutex::new(None)),
            log_dir: Arc::new(Mutex::new(cfg.log_dir.clone())),
        };
        let me = t.clone();
        std::thread::Builder::new().name("горн-трекер".into()).spawn(move || me.run(app, cfg, rx)).expect("поток трекера");
        // Пульс: в карте таймер идёт без новых строк лога.
        let tick = tx.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(5));
            if tick.send(Msg::Tick).is_err() {
                break;
            }
        });
        let _ = tx.send(Msg::Boot);
        t
    }

    /// Выполнить над движком и дождаться результата.
    pub fn call<T: Send + 'static>(&self, f: impl FnOnce(&mut Engine) -> T + Send + 'static) -> Option<T> {
        let (rtx, rrx) = channel();
        self.tx
            .send(Msg::Job(Box::new(move |e| {
                let _ = rtx.send(f(e));
            })))
            .ok()?;
        rrx.recv().ok()
    }

    /// Выполнить без ожидания.
    pub fn post(&self, f: impl FnOnce(&mut Engine) + Send + 'static) {
        let _ = self.tx.send(Msg::Job(Box::new(f)));
    }

    /// Строки лога будут копироваться и сюда.
    pub fn forward_lines(&self, to: Sender<Vec<String>>) {
        *self.lines_sink.lock().unwrap() = Some(to);
    }

    /// Папка логов для следующего старта (пока игра не найдена — применится сразу).
    pub fn set_log_dir(&self, dir: Option<PathBuf>) {
        *self.log_dir.lock().unwrap() = dir;
    }

    pub fn boot(&self) {
        let _ = self.tx.send(Msg::Boot);
    }

    fn run(self, app: AppHandle, cfg: Config, rx: Receiver<Msg>) {
        let mut engine = match Engine::new(&cfg.data_dir, &cfg.items, cfg.options.clone(), Arc::new(|| chrono::Utc::now().timestamp_millis())) {
            Ok(e) => e,
            Err(err) => {
                eprintln!("[трекер] база не открылась: {err}");
                return;
            }
        };
        let sink = cfg.sink.clone();
        let mut tailer: Option<TailerHandle> = None;
        let mut last_boot_try = std::time::Instant::now() - Duration::from_secs(3600);
        for msg in rx {
            match msg {
                Msg::Job(f) => f(&mut engine),
                Msg::Lines(lines) => {
                    engine.consume(&lines);
                    if let Some(s) = self.lines_sink.lock().unwrap().as_ref() {
                        let _ = s.send(lines);
                    }
                }
                Msg::Rotate => engine.on_log_rotate(),
                Msg::Tick => {
                    engine.heartbeat();
                    // Игра не найдена: поиск повторяется раз в 45 с.
                    if !engine.is_booted() && last_boot_try.elapsed() > Duration::from_secs(45) {
                        let _ = self.tx.send(Msg::Boot);
                    }
                }
                Msg::Boot => {
                    if engine.is_booted() {
                        continue;
                    }
                    last_boot_try = std::time::Instant::now();
                    let dir = self.log_dir.lock().unwrap().clone();
                    let progress_app = app.clone();
                    let mut progress = move |p: &gorn_core::retroscan::RetroScanProgress| {
                        let _ = progress_app.emit(
                            "scan:progress",
                            json!({ "file": p.file.to_string_lossy(), "index": p.index, "total": p.total, "runsFound": p.runs_found }),
                        );
                    };
                    match engine.start(dir.as_deref(), Some(&mut progress)) {
                        Ok(Some((path, position))) => {
                            let tx = self.tx.clone();
                            let t = Tailer::new(&path, TailerOptions { from_start: false, initial_position: position });
                            if let Some(old) = tailer.take() {
                                old.stop();
                            }
                            tailer = Some(t.spawn(move |ev| {
                                let _ = match ev {
                                    TailEvent::Lines(l) => tx.send(Msg::Lines(l)),
                                    TailEvent::Rotate => tx.send(Msg::Rotate),
                                    TailEvent::Error(e) => {
                                        eprintln!("[трекер] лог: {e}");
                                        Ok(())
                                    }
                                };
                            }));
                            prefetch_icons(&engine);
                        }
                        Ok(None) => {}
                        Err((snap, why)) => {
                            eprintln!("[трекер] старт: {why}");
                            self.publish(&sink, serde_json::to_value(&*snap).unwrap_or(Value::Null));
                        }
                    }
                }
            }
            if engine.take_emit() {
                let snap = serde_json::to_value(engine.build_snapshot()).unwrap_or(Value::Null);
                self.publish(&sink, snap);
            }
        }
        if let Some(t) = tailer {
            t.stop();
        }
        engine.stop();
    }

    fn publish(&self, sink: &Sink, snap: Value) {
        sink(&snap);
        *self.latest.lock().unwrap() = snap;
    }

    /// Остановить: дописать закрытое в базу (вызывать при выходе).
    pub fn shutdown(&self) {
        self.call(|e| e.stop());
    }
}

/// Иконки виденных предметов — заранее, фоном: к входу в карту они уже на диске.
fn prefetch_icons(engine: &Engine) {
    let ids = engine.seen_item_ids();
    std::thread::spawn(move || {
        let n = crate::icons::prefetch(&ids);
        if n > 0 {
            eprintln!("[иконки] загружено {n}");
        }
    });
}
