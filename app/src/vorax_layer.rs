//! Окно отметок Vorax: прозрачное во весь монитор, клики насквозь, фокус не берёт, в захват не попадает (как
//! окна стрелок тюрьмы и цен прилавка). Страница — `ui/public/vorax.js`, событие `vorax:marks`.

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, SetWindowDisplayAffinity, SetWindowLongPtrW, SetWindowPos, ShowWindow,
    GWL_EXSTYLE, HWND_TOPMOST, SWP_NOACTIVATE, SW_HIDE, SW_SHOWNOACTIVATE, WDA_EXCLUDEFROMCAPTURE,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
};

use crate::overlay::Display;
use gorn_vorax_scan::marks::Plan;

pub struct VoraxLayer {
    app: AppHandle,
    window: Option<WebviewWindow>,
    ready: bool,
    pending: Option<Value>,
    visible: bool,
    at: Option<Display>,
    /// последние отметки (CSS-пиксели) — для «приглушить», пока доска меняется
    last: Option<Value>,
}

impl VoraxLayer {
    pub fn new(app: AppHandle) -> Self {
        VoraxLayer { app, window: None, ready: false, pending: None, visible: false, at: None, last: None }
    }

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
        let win = WebviewWindowBuilder::new(&self.app, "vorax", WebviewUrl::App("vorax.html".into()))
            .title("Горн — подсказки Vorax")
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
            .map_err(|e| eprintln!("[vorax] окно: {e}"))
            .ok()?;
        let _ = win.set_ignore_cursor_events(true);
        if let Ok(hwnd) = win.hwnd() {
            unsafe {
                let h = HWND(hwnd.0);
                let _ = SetWindowDisplayAffinity(h, WDA_EXCLUDEFROMCAPTURE);
                let ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
                SetWindowLongPtrW(
                    h,
                    GWL_EXSTYLE,
                    ex | (WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0 | WS_EX_TRANSPARENT.0 | WS_EX_LAYERED.0) as isize,
                );
            }
        }
        self.ready = false;
        self.window = Some(win.clone());
        Some(win)
    }

    fn send(&mut self, payload: Value) {
        if let Some(win) = &self.window {
            let _ = win.emit_to(win.label(), "vorax:marks", payload);
        }
    }

    fn put(&mut self, d: &Display, payload: Value) {
        let Some(win) = self.ensure() else { return };
        if self.at != Some(*d) {
            if let Ok(hwnd) = win.hwnd() {
                unsafe {
                    let _ = SetWindowPos(HWND(hwnd.0), Some(HWND_TOPMOST), d.x, d.y, d.w, d.h, SWP_NOACTIVATE);
                }
            }
            self.at = Some(*d);
        }
        if self.ready {
            self.send(payload);
        } else {
            self.pending = Some(payload);
        }
        if !self.visible {
            if let Ok(hwnd) = win.hwnd() {
                unsafe {
                    let _ = ShowWindow(HWND(hwnd.0), SW_SHOWNOACTIVATE);
                }
            }
            self.visible = true;
        }
    }

    /// Отметки в пикселях кадра монитора (`frame_w` — ширина кадра) на мониторе d.
    pub fn show(&mut self, d: &Display, plan: &Plan, frame_w: usize) {
        if plan.is_empty() {
            self.clear();
            return;
        }
        let k = d.w as f64 / frame_w as f64 / d.scale;
        let payload = json!({ "plan": plan.mapped(0.0, 0.0, k), "dim": false, "scale": d.h as f64 / d.scale / 1440.0 });
        self.last = Some(payload.clone());
        self.put(d, payload);
    }

    /// Доска меняется (игрок подтвердил или обновил): отметки прочь, плашка приглушена до нового совета.
    pub fn dim(&mut self) {
        self.set_dim(true);
    }

    /// Доска не сменилась (ход не принят): отметки обратно.
    pub fn undim(&mut self) {
        self.set_dim(false);
    }

    fn set_dim(&mut self, dim: bool) {
        let (Some(d), Some(mut p)) = (self.at, self.last.clone()) else { return };
        if !self.visible || p["dim"] == json!(dim) {
            return;
        }
        p["dim"] = json!(dim);
        self.last = Some(p.clone());
        self.put(&d, p);
    }

    pub fn clear(&mut self) {
        self.last = None;
        if !self.visible {
            return;
        }
        self.pending = None;
        if self.ready {
            self.send(Value::Null);
        }
        if let Some(win) = &self.window {
            if let Ok(hwnd) = win.hwnd() {
                unsafe {
                    let _ = ShowWindow(HWND(hwnd.0), SW_HIDE);
                }
            }
        }
        self.visible = false;
    }
}
