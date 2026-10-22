//! Окно со стрелками поверх игры (перенос `main/prison/arrowLayer.ts`).
//!
//! Во весь монитор: стрелки тянутся через всю доску. Полноэкранное
//! прозрачное окно дорого для композиции, поэтому оно показано, только пока
//! есть подсказка или «смотрю…». Клики проходят насквозь всегда, фокус окно
//! не берёт (иначе полноэкранная игра свернётся), в захват экрана не попадает
//! (WDA_EXCLUDEFROMCAPTURE — стрелки не читаются как часть доски).
//!
//! Страница окна — `ui/public/arrows.js` (без React, не `prison.tsx`): формат
//! подсказки из `show` и `thinking` меняется вместе с ней.

use std::time::{Duration, Instant};

use gorn_prison::hint::{Hint, HintMark, Plate};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, HMONITOR, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetWindowLongPtrW, SetWindowDisplayAffinity, SetWindowLongPtrW, SetWindowPos,
    ShowWindow, GWL_EXSTYLE, HWND_TOPMOST, SWP_NOACTIVATE, SW_HIDE, SW_SHOWNOACTIVATE,
    WDA_EXCLUDEFROMCAPTURE, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
};

/// Плашка под карточкой: отступ и высота с запасом (CSS-пиксели).
const PLATE_GAP: f64 = 14.0;
const PLATE_H: f64 = 84.0;

/// «Смотрю…» без подсказки дольше этого — убрать.
const THINKING_MAX: Duration = Duration::from_secs(20);

/// Монитор: HMONITOR, прямоугольник в пикселях, масштаб.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Display {
    pub hmonitor: isize,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub scale: f64,
}

pub fn display_of(hm: HMONITOR) -> Option<Display> {
    let mut mi = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    unsafe {
        if !GetMonitorInfoW(hm, &mut mi).as_bool() {
            return None;
        }
    }
    let RECT {
        left,
        top,
        right,
        bottom,
    } = mi.rcMonitor;
    let (mut dx, mut dy) = (96u32, 96u32);
    unsafe {
        let _ = GetDpiForMonitor(hm, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
    }
    Some(Display {
        hmonitor: hm.0 as isize,
        x: left,
        y: top,
        w: right - left,
        h: bottom - top,
        scale: dx as f64 / 96.0,
    })
}

/// Монитор под курсором.
pub fn display_under_cursor() -> Option<Display> {
    let mut p = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut p);
        display_of(MonitorFromPoint(p, MONITOR_DEFAULTTONEAREST))
    }
}

/// Монитор по точке (сохранённый с прошлого раза).
pub fn display_at(x: i32, y: i32) -> Option<Display> {
    unsafe {
        display_of(MonitorFromPoint(
            POINT { x: x + 1, y: y + 1 },
            MONITOR_DEFAULTTONEAREST,
        ))
    }
}

pub struct ArrowLayer {
    app: AppHandle,
    window: Option<WebviewWindow>,
    ready: bool,
    pending: Option<Value>,
    visible: bool,
    at: Option<Display>,
    think_until: Option<Instant>,
}

impl ArrowLayer {
    pub fn new(app: AppHandle) -> Self {
        ArrowLayer {
            app,
            window: None,
            ready: false,
            pending: None,
            visible: false,
            at: None,
            think_until: None,
        }
    }

    /// Страница окна подписалась на подсказки.
    pub fn on_ready(&mut self) {
        self.ready = true;
        if let Some(p) = self.pending.take() {
            self.send(p);
        }
    }

    fn ensure(&mut self) -> Option<WebviewWindow> {
        if let Some(w) = &self.window {
            return Some(w.clone());
        }
        let win =
            WebviewWindowBuilder::new(&self.app, "arrows", WebviewUrl::App("arrows.html".into()))
                .title("Горн — подсказки в тюрьме")
                .transparent(true)
                .decorations(false)
                .always_on_top(true)
                .skip_taskbar(true)
                .resizable(false)
                .shadow(false)
                .focused(false)
                .visible(false)
                .additional_browser_args(crate::layers::BROWSER_ARGS)
                .build()
                .map_err(|e| eprintln!("[тюрьма] окно стрелок: {e}"))
                .ok()?;
        let _ = win.set_ignore_cursor_events(true);
        if let Ok(hwnd) = win.hwnd() {
            unsafe {
                let h = HWND(hwnd.0);
                // Не попадать в собственный захват экрана и не брать фокус.
                let _ = SetWindowDisplayAffinity(h, WDA_EXCLUDEFROMCAPTURE);
                let ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
                SetWindowLongPtrW(
                    h,
                    GWL_EXSTYLE,
                    ex | (WS_EX_NOACTIVATE.0
                        | WS_EX_TOOLWINDOW.0
                        | WS_EX_TRANSPARENT.0
                        | WS_EX_LAYERED.0) as isize,
                );
            }
        }
        self.ready = false;
        self.window = Some(win.clone());
        Some(win)
    }

    fn place(&mut self, d: &Display) {
        let Some(win) = self.ensure() else { return };
        if let Ok(hwnd) = win.hwnd() {
            unsafe {
                let _ = SetWindowPos(
                    HWND(hwnd.0),
                    Some(HWND_TOPMOST),
                    d.x,
                    d.y,
                    d.w,
                    d.h,
                    SWP_NOACTIVATE,
                );
            }
        }
        self.at = Some(*d);
    }

    fn show_window(&mut self) {
        if self.visible {
            return;
        }
        if let Some(win) = &self.window {
            if let Ok(hwnd) = win.hwnd() {
                unsafe {
                    let _ = ShowWindow(HWND(hwnd.0), SW_SHOWNOACTIVATE);
                }
                self.visible = true;
            }
        }
    }

    fn hide_window(&mut self) {
        if let Some(win) = &self.window {
            if let Ok(hwnd) = win.hwnd() {
                unsafe {
                    let _ = ShowWindow(HWND(hwnd.0), SW_HIDE);
                }
            }
        }
        self.visible = false;
    }

    fn send(&mut self, payload: Value) {
        if let Some(win) = &self.window {
            let _ = win.emit_to(win.label(), "prison:hint", payload);
        }
    }

    fn present(&mut self, d: &Display, payload: Value) {
        self.think_until = None;
        self.place(d);
        if self.ready {
            self.send(payload);
            self.show_window();
        } else {
            self.pending = Some(payload);
            self.show_window();
        }
    }

    /// Подсказка в пикселях кадра (fw × fh) → окно на мониторе d.
    pub fn show(&mut self, d: &Display, hint: &Hint, fw: usize, fh: usize, live: bool, phase: &str) {
        // кадр → пиксели монитора → CSS-пиксели окна
        let sx = d.w as f64 / fw as f64 / d.scale;
        let sy = d.h as f64 / fh as f64 / d.scale;
        let pt = |p: [f64; 2]| [p[0] * sx, p[1] * sy];
        let bx = |b: &[f64; 4]| [b[0] * sx, b[1] * sy, b[2] * sx, b[3] * sy];
        let marks: Vec<Value> = hint
            .marks
            .iter()
            .map(|m| match m {
                HintMark::Arrow { from, to, step } => {
                    json!({ "kind": "arrow", "from": pt(*from), "to": pt(*to), "step": step })
                }
                HintMark::Ring { at, r, step } => {
                    json!({ "kind": "ring", "at": pt(*at), "r": r * sx, "step": step })
                }
                HintMark::Box { b, step } => json!({ "kind": "box", "box": bx(b), "step": step }),
                HintMark::Path { points } => {
                    json!({ "kind": "path", "points": points.iter().map(|p| pt(*p)).collect::<Vec<_>>() })
                }
            })
            .collect();
        let (cw, ch) = (d.w as f64 / d.scale, d.h as f64 / d.scale);
        let plate = match hint.plate {
            // Под карточкой; у нижнего края экрана — над ней.
            Some(Plate::Card(b)) => {
                let [x0, y0, x1, y1] = bx(&b);
                let x = (x0 + x1) / 2.0;
                if y1 + PLATE_GAP + PLATE_H <= ch {
                    json!({ "at": [x, y1 + PLATE_GAP], "anchor": "top", "card": x1 - x0 })
                } else {
                    json!({ "at": [x, y0 - PLATE_GAP], "anchor": "bottom", "card": x1 - x0 })
                }
            }
            Some(Plate::Yard { at, anchor }) => json!({ "at": pt(at), "anchor": anchor.name() }),
            None => json!({ "at": [cw / 2.0, ch * 0.12], "anchor": "top" }),
        };
        let payload = json!({
            "title": hint.title,
            "sub": hint.sub,
            "twoSteps": hint.two_steps(),
            "marks": marks,
            "plate": plate,
            "live": live,
            "phase": phase,
        });
        self.present(d, payload);
    }

    /// Ход сделан, новая подсказка в пути: убрать стрелки и показать «смотрю…».
    /// Окно на экране ещё и заставляет Windows пересобирать экран — поток
    /// экрана получает свежие кадры (в сам кадр окно не попадает).
    pub fn thinking(&mut self, d: &Display) {
        let (cw, ch) = (d.w as f64 / d.scale, d.h as f64 / d.scale);
        let payload = json!({
            "title": "",
            "sub": "",
            "twoSteps": false,
            "marks": [],
            "plate": { "at": [cw / 2.0, ch * 0.1], "anchor": "top" },
            "live": false,
            "phase": "instant",
            "thinking": true,
        });
        self.present(d, payload);
        self.think_until = Some(Instant::now() + THINKING_MAX);
    }

    /// Поток застыл: заново показанное окно поверх игры возвращает композицию.
    pub fn nudge(&mut self, d: &Display) {
        if !self.visible {
            self.thinking(d);
            return;
        }
        self.hide_window();
        self.show_window();
    }

    /// Совет подтвердился проигрыванием вариантов — тот же ход.
    pub fn settle(&mut self, live: bool) {
        if self.ready && self.visible {
            if let Some(win) = &self.window {
                let _ = win.emit_to(win.label(), "prison:settled", live);
            }
        }
    }

    /// Убрать подсказку и спрятать окно.
    pub fn clear(&mut self) {
        self.think_until = None;
        self.pending = None;
        if self.ready {
            self.send(Value::Null);
        }
        self.hide_window();
    }

    /// Раз в тик: «смотрю…» слишком долго — убрать.
    pub fn tick(&mut self) {
        if self.think_until.is_some_and(|t| Instant::now() >= t) {
            self.clear();
        }
    }
}
