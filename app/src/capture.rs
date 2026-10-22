//! Поток экрана монитора с игрой: Windows.Graphics.Capture (без скрытого
//! окна Chromium и копий кадра через IPC, как в Горне на Electron).
//!
//! Обработчик кадров держит последний кадр, уменьшенный до ширины не больше
//! MAX_WIDTH (распознавание и так работает на ~1920), и отпечаток 64×36:
//! когда картинка последний раз заметно менялась. Кадр для подсказки берётся,
//! как только экран успокоился после хода (`grab`), — как `renderer/capture.ts`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows_capture::capture::{CaptureControl, Context, GraphicsCaptureApiHandler};
use windows_capture::frame::Frame;
use windows_capture::graphics_capture_api::InternalCaptureControl;
use windows_capture::monitor::Monitor;
use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
    MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
};

/// Ширина кадра для распознавания не больше этой (4K уменьшается вдвое).
const MAX_WIDTH: usize = 2600;
/// Средняя разница отпечатков (0–255), выше которой картинка «изменилась».
const CHANGE: f64 = 3.0;
const SIG_W: usize = 64;
const SIG_H: usize = 36;
/// Кадров в секунду от системы.
const FPS: u64 = 10;
/// Без запросов кадры переводятся в RGBA реже (каждый IDLE_EVERY-й): экономия процессора.
const IDLE_EVERY: u64 = 5;
const BOOST: Duration = Duration::from_secs(5);

/// Кадр RGBA.
#[derive(Clone)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub data: Vec<u8>,
}

#[derive(Default)]
struct Shared {
    latest: Option<Arc<Image>>,
    latest_at: Option<Instant>,
    changed_at: Option<Instant>,
    sig: Vec<u8>,
    boost_until: Option<Instant>,
    frames: u64,
    closed: bool,
}

struct Handler {
    shared: Arc<Mutex<Shared>>,
    scratch: Vec<u8>,
}

impl GraphicsCaptureApiHandler for Handler {
    type Flags = Arc<Mutex<Shared>>;
    type Error = Box<dyn std::error::Error + Send + Sync>;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        Ok(Handler {
            shared: ctx.flags,
            scratch: vec![],
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        _c: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        let now = Instant::now();
        let fb = frame.buffer()?;
        let (w, h) = (fb.width() as usize, fb.height() as usize);
        let raw = fb.as_nopadding_buffer(&mut self.scratch);
        // Отпечаток 64×36 по точкам сетки (BGRA: B, G, R).
        let mut sig = vec![0u8; SIG_W * SIG_H * 2];
        for j in 0..SIG_H {
            let y = (j * 2 + 1) * h / (SIG_H * 2);
            for i in 0..SIG_W {
                let x = (i * 2 + 1) * w / (SIG_W * 2);
                let k = (y * w + x) * 4;
                sig[(j * SIG_W + i) * 2] = raw[k + 2];
                sig[(j * SIG_W + i) * 2 + 1] = raw[k + 1];
            }
        }
        let (convert, changed) = {
            let mut s = self.shared.lock().unwrap();
            s.frames += 1;
            let changed = if s.sig.len() == sig.len() {
                let diff: u64 = s
                    .sig
                    .iter()
                    .zip(&sig)
                    .map(|(a, b)| (*a as i32 - *b as i32).unsigned_abs() as u64)
                    .sum();
                diff as f64 / sig.len() as f64 > CHANGE
            } else {
                true
            };
            if changed {
                s.changed_at = Some(now);
            }
            s.sig = sig;
            let boosted = s.boost_until.is_some_and(|b| now < b);
            (
                boosted || s.latest.is_none() || s.frames % IDLE_EVERY == 0,
                changed,
            )
        };
        let _ = changed;
        if convert {
            let img = Arc::new(downscale_bgra(raw, w, h));
            let mut s = self.shared.lock().unwrap();
            s.latest = Some(img);
            s.latest_at = Some(now);
        }
        Ok(())
    }

    fn on_closed(&mut self) -> Result<(), Self::Error> {
        self.shared.lock().unwrap().closed = true;
        Ok(())
    }
}

/// BGRA → RGBA, уменьшение в целое число раз (среднее по квадрату) до ширины ≤ MAX_WIDTH.
fn downscale_bgra(raw: &[u8], w: usize, h: usize) -> Image {
    let f = w.div_ceil(MAX_WIDTH).max(1);
    let (ow, oh) = (w / f, h / f);
    let mut data = vec![255u8; ow * oh * 4];
    if f == 1 {
        for i in 0..ow * oh {
            data[i * 4] = raw[i * 4 + 2];
            data[i * 4 + 1] = raw[i * 4 + 1];
            data[i * 4 + 2] = raw[i * 4];
        }
    } else {
        let n = (f * f) as u32;
        for y in 0..oh {
            for x in 0..ow {
                let (mut r, mut g, mut b) = (0u32, 0u32, 0u32);
                for dy in 0..f {
                    let row = (y * f + dy) * w;
                    for dx in 0..f {
                        let k = (row + x * f + dx) * 4;
                        b += raw[k] as u32;
                        g += raw[k + 1] as u32;
                        r += raw[k + 2] as u32;
                    }
                }
                let o = (y * ow + x) * 4;
                data[o] = ((r + n / 2) / n) as u8;
                data[o + 1] = ((g + n / 2) / n) as u8;
                data[o + 2] = ((b + n / 2) / n) as u8;
            }
        }
    }
    Image {
        width: ow,
        height: oh,
        data,
    }
}

/// Почему grab не дал кадра.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Miss {
    /// Поток жив, но новых кадров нет (игра не пересобирает экран).
    Stale,
    /// Поток молчит (ни одного кадра) или закрыт.
    Silent,
}

/// Поток экрана одного монитора.
pub struct ScreenStream {
    shared: Arc<Mutex<Shared>>,
    control: Option<CaptureControl<Handler, Box<dyn std::error::Error + Send + Sync>>>,
    /// HMONITOR открытого потока.
    pub monitor: Option<isize>,
    opened_at: Instant,
    /// Последний grab: экран к снимку не менялся stable (иначе — посреди анимации).
    settled: AtomicBool,
}

impl Default for ScreenStream {
    fn default() -> Self {
        ScreenStream {
            shared: Arc::default(),
            control: None,
            monitor: None,
            opened_at: Instant::now(),
            settled: AtomicBool::new(true),
        }
    }
}

impl ScreenStream {
    /// Открыть поток монитора (HMONITOR), если ещё не открыт на нём.
    pub fn ensure(&mut self, hmonitor: isize) -> Result<(), String> {
        if self.control.is_some()
            && self.monitor == Some(hmonitor)
            && !self.shared.lock().unwrap().closed
        {
            return Ok(());
        }
        self.stop();
        let shared: Arc<Mutex<Shared>> = Arc::default();
        let settings = Settings::new(
            Monitor::from_raw_hmonitor(hmonitor as *mut std::ffi::c_void),
            CursorCaptureSettings::WithoutCursor,
            DrawBorderSettings::WithoutBorder,
            SecondaryWindowSettings::Default,
            MinimumUpdateIntervalSettings::Custom(Duration::from_millis(1000 / FPS)),
            DirtyRegionSettings::Default,
            ColorFormat::Bgra8,
            shared.clone(),
        );
        let control =
            Handler::start_free_threaded(settings).map_err(|e| format!("захват экрана: {e}"))?;
        self.shared = shared;
        self.control = Some(control);
        self.monitor = Some(hmonitor);
        self.opened_at = Instant::now();
        Ok(())
    }

    pub fn stop(&mut self) {
        if let Some(c) = self.control.take() {
            let _ = c.stop();
        }
        self.monitor = None;
    }

    /// Экран вот-вот сменится: каждый кадр — в RGBA несколько секунд.
    pub fn boost(&self) {
        self.shared.lock().unwrap().boost_until = Some(Instant::now() + BOOST);
    }

    /// Кадр после запроса, на котором экран уже stable не менялся; не дольше
    /// max (тогда — последний свежий). Новых кадров нет — годится последний,
    /// если он не старше ok_age. Иначе — Miss.
    pub fn grab(
        &self,
        stable: Duration,
        mut max: Duration,
        ok_age: Duration,
    ) -> Result<Arc<Image>, Miss> {
        self.boost();
        // Только что открытому потоку — время на первый кадр.
        if self.opened_at.elapsed() < Duration::from_millis(2500) {
            max = max.max(Duration::from_millis(2500));
        }
        let asked = Instant::now();
        loop {
            let now = Instant::now();
            {
                let s = self.shared.lock().unwrap();
                if s.closed {
                    return Err(Miss::Silent);
                }
                let fresh = s.latest_at.is_some_and(|t| t >= asked);
                if fresh && s.changed_at.is_none_or(|c| now - c >= stable) {
                    break;
                }
            }
            if now - asked >= max {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let s = self.shared.lock().unwrap();
        let Some(img) = s.latest.clone() else {
            return Err(Miss::Silent);
        };
        self.settled.store(s.changed_at.is_none_or(|c| Instant::now() - c >= stable), Ordering::Relaxed);
        let at = s.latest_at.unwrap();
        if at < asked && asked - at > ok_age {
            return Err(Miss::Stale);
        }
        Ok(img)
    }
}

impl ScreenStream {
    /// Успокоился ли экран к последнему снимку: false — снят посреди анимации
    /// (ждали дольше max), учиться на таком кадре нельзя.
    pub fn last_settled(&self) -> bool {
        self.settled.load(Ordering::Relaxed)
    }
}

impl Drop for ScreenStream {
    fn drop(&mut self) {
        self.stop();
    }
}
