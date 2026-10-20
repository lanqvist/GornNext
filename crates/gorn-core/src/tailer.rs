//! Дочитывание лога в реальном времени. Перенос `main/tailer.ts`.
//!
//! На Windows уведомления об изменении этого файла приходят нерегулярно
//! (игра пишет буферизованным потоком), поэтому — опрос размера раз в 300 мс.
//! Игра ротирует лог: уменьшение размера — признак ротации, читаем с нуля.
//! За карту в лог падает несколько мегабайт — читаем порциями.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub const POLL_INTERVAL: Duration = Duration::from_millis(300);
/// Максимум за один проход, чтобы не застревать на гигабайтном логе.
pub const MAX_CHUNK_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Default)]
pub struct TailerOptions {
    /// Начать с начала файла (первичный разбор), иначе — с конца.
    pub from_start: bool,
    /// Явная стартовая позиция — до неё файл уже разобран ретро-сканом.
    /// Приоритетнее from_start: строки, записанные между сканом и стартом, не теряются.
    pub initial_position: Option<u64>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TailEvent {
    Lines(Vec<String>),
    /// Игра ротировала лог и начала новый.
    Rotate,
    Error(String),
}

pub struct Tailer {
    path: PathBuf,
    /// None — позиция неизвестна: файл не удалось прочитать при старте.
    position: Option<u64>,
    /// Хвост неполной строки (байты: и недописанный символ UTF-8 тоже).
    carry: Vec<u8>,
    /// Старт с конца файла пришёлся на середину строки: первый кусок — обрубок.
    skip_partial: bool,
}

impl Tailer {
    /// Создаёт и выставляет стартовую позицию (как `start()` эталона).
    pub fn new(path: impl Into<PathBuf>, options: TailerOptions) -> Self {
        let path = path.into();
        let mut t = Tailer { path, position: None, carry: vec![], skip_partial: false };
        if let Some(p) = options.initial_position {
            t.position = Some(p);
        } else if options.from_start {
            // С начала — без взгляда на файл: его может ещё не быть.
            t.position = Some(0);
        } else if let Ok(md) = std::fs::metadata(&t.path) {
            let p = md.len();
            t.position = Some(p);
            t.skip_partial = p > 0 && !ends_with_newline(&t.path, p);
        }
        // Иначе файла ещё нет или он мгновение заблокирован: ноль здесь скормил бы
        // движку весь живой лог, поэтому позицию выставит первый успешный проход.
        t
    }

    /// Позиция в байтах; -1 — неизвестна.
    pub fn position(&self) -> i64 {
        self.position.map(|p| p as i64).unwrap_or(-1)
    }

    /// Один проход: прочитать новое, если оно есть.
    pub fn pump(&mut self) -> Vec<TailEvent> {
        let mut events = vec![];
        let Ok(md) = std::fs::metadata(&self.path) else { return events }; // игра ещё не создала файл
        let size = md.len();
        // Первый успешный stat после неудачного старта: игра только что создала лог — читаем целиком.
        let mut pos = self.position.unwrap_or(0);
        if size < pos {
            pos = 0;
            self.carry.clear();
            self.skip_partial = false;
            events.push(TailEvent::Rotate);
        }
        self.position = Some(pos);
        if size <= pos {
            return events;
        }
        let end = size.min(pos + MAX_CHUNK_BYTES);
        match self.read_range(pos, end) {
            Ok(lines) => {
                self.position = Some(end);
                if !lines.is_empty() {
                    events.push(TailEvent::Lines(lines));
                }
            }
            Err(e) => events.push(TailEvent::Error(e.to_string())),
        }
        events
    }

    /// Диапазон байт, порезанный на строки по `\r?\n`; хвост ждёт следующего прохода.
    fn read_range(&mut self, start: u64, end: u64) -> std::io::Result<Vec<String>> {
        let mut f = std::fs::File::open(&self.path)?;
        f.seek(SeekFrom::Start(start))?;
        let mut buf = Vec::with_capacity((end - start) as usize);
        f.take(end - start).read_to_end(&mut buf)?;
        self.carry.extend_from_slice(&buf);
        let mut lines = vec![];
        let mut from = 0;
        while let Some(i) = self.carry[from..].iter().position(|&b| b == b'\n') {
            let mut line = &self.carry[from..from + i];
            if line.last() == Some(&b'\r') {
                line = &line[..line.len() - 1];
            }
            lines.push(String::from_utf8_lossy(line).into_owned());
            from += i + 1;
        }
        self.carry.drain(..from);
        if self.skip_partial && !lines.is_empty() {
            lines.remove(0);
            self.skip_partial = false;
        }
        Ok(lines)
    }

    /// Опрос в своём потоке; события — в `on_event`. Остановка — через возвращённый флаг.
    pub fn spawn(mut self, mut on_event: impl FnMut(TailEvent) + Send + 'static) -> TailerHandle {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let thread = std::thread::Builder::new()
            .name("gorn-tailer".into())
            .spawn(move || {
                while !flag.load(Ordering::Relaxed) {
                    for e in self.pump() {
                        on_event(e);
                    }
                    std::thread::sleep(POLL_INTERVAL);
                }
                self
            })
            .expect("поток дочитывания");
        TailerHandle { stop, thread: Some(thread) }
    }
}

/// Байт перед позицией — перевод строки, то есть позиция на границе строк.
fn ends_with_newline(path: &Path, position: u64) -> bool {
    let read = || -> std::io::Result<u8> {
        let mut f = std::fs::File::open(path)?;
        f.seek(SeekFrom::Start(position - 1))?;
        let mut b = [0u8; 1];
        f.read_exact(&mut b)?;
        Ok(b[0])
    };
    read().map(|b| b == b'\n').unwrap_or(true)
}

pub struct TailerHandle {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<Tailer>>,
}

impl TailerHandle {
    /// Остановить опрос и вернуть дочитыватель (с его позицией).
    pub fn stop(mut self) -> Option<Tailer> {
        self.stop.store(true, Ordering::Relaxed);
        self.thread.take().and_then(|t| t.join().ok())
    }
}

impl Drop for TailerHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}
