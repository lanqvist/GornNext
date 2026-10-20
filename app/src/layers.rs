//! Окна трекера поверх игры: оверлей, слой всплывающего дохода и пульт —
//! перенос `main/overlay.ts`, `main/toastLayer.ts`, `main/panel.ts` и
//! `main/snapshotGate.ts`. Срезы движка расходятся по окнам отсюда:
//! спрятанное окно их не получает, а при показе ему уходит последний.
//!
//! Перекрыть эксклюзивный полноэкранный режим нельзя ничем: он минует
//! оконный композитор. Игре нужен «окно без рамки» или оконный режим.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Map, Value};
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    MONITOR_DEFAULTTOPRIMARY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, ShowWindow, GWL_EXSTYLE,
    SWP_NOACTIVATE, SWP_NOZORDER, SW_HIDE, SW_SHOWNOACTIVATE, WS_EX_TOOLWINDOW,
};

use crate::settings::{Settings, OVERLAY_MODES};

/// Аргументы WebView2 у всех окон одинаковые (одно окружение на приложение):
/// умолчания Tauri плюс звук без жеста пользователя — как в Electron.
pub const BROWSER_ARGS: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --autoplay-policy=no-user-gesture-required";

/// Ширина колонки оверлея под режим, CSS-пиксели (высоту сообщает интерфейс).
fn mode_width(mode: &str) -> f64 {
    match mode {
        "minimal" => 250.0,
        "full" => 330.0,
        // Ленте ширину задаёт содержимое; это — до первого замера.
        "strip" => 640.0,
        _ => 290.0,
    }
}
const STRIP_MIN: f64 = 320.0;
const STRIP_MAX: f64 = 1200.0;
const INITIAL_HEIGHT: f64 = 64.0;
const MAX_HEIGHT: f64 = 520.0;

const TOAST_WIDTH: f64 = 560.0;
const TOAST_MAX: f64 = 260.0;

const PANEL_WIDTH: f64 = 780.0;
const PANEL_HEIGHT: f64 = 540.0;

/// Рабочая область монитора (без панели задач), пиксели, и масштаб.
#[derive(Clone, Copy, Debug)]
struct Area {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    scale: f64,
}

fn area_at(p: Option<(i32, i32)>) -> Area {
    unsafe {
        let hm = match p {
            Some((x, y)) => MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST),
            None => MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY),
        };
        let mut mi = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let _ = GetMonitorInfoW(hm, &mut mi);
        let RECT {
            left,
            top,
            right,
            bottom,
        } = mi.rcWork;
        let scale = crate::overlay::display_of(hm)
            .map(|d| d.scale)
            .unwrap_or(1.0);
        Area {
            x: left,
            y: top,
            w: right - left,
            h: bottom - top,
            scale,
        }
    }
}

fn cursor_area() -> Area {
    let mut p = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut p);
    }
    area_at(Some((p.x, p.y)))
}

/// Окно не видно в Alt+Tab. Фокус не берёт — `focusable(false)` при создании:
/// стиль, выставленный руками, Tauri стирает при смене флагов окна (клики насквозь).
fn tool_window(win: &WebviewWindow) {
    if let Ok(hwnd) = win.hwnd() {
        unsafe {
            let h = HWND(hwnd.0);
            let ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
            SetWindowLongPtrW(h, GWL_EXSTYLE, ex | WS_EX_TOOLWINDOW.0 as isize);
        }
    }
}

fn show_inactive(win: &WebviewWindow) {
    if let Ok(hwnd) = win.hwnd() {
        unsafe {
            let _ = ShowWindow(HWND(hwnd.0), SW_SHOWNOACTIVATE);
        }
    }
}

fn hide(win: &WebviewWindow) {
    if let Ok(hwnd) = win.hwnd() {
        unsafe {
            let _ = ShowWindow(HWND(hwnd.0), SW_HIDE);
        }
    }
}

/// Перенос и размер одним вызовом — для окон, уже стоящих на своём мониторе.
/// Каждое изменение размера прозрачного окна WebView2 — пересоздание его
/// поверхности в другом процессе; три вызова подряд (как в `place`) на
/// каждое изменение содержимого давали подтормаживание игры.
fn set_rect(win: &WebviewWindow, x: i32, y: i32, w: i32, h: i32) {
    if let Ok(hwnd) = win.hwnd() {
        unsafe {
            let _ = SetWindowPos(HWND(hwnd.0), None, x, y, w.max(1), h.max(1), SWP_NOZORDER | SWP_NOACTIVATE);
        }
    }
}

/// Позиция и размер в пикселях. Сначала перенос: окно, созданное на мониторе
/// с другим масштабом, получает WM_DPICHANGED и пересчитывает размер само —
/// размер, заданный до переноса, умножался дважды (оверлей и плашки в 1,5 раза
/// шире при 150 %). После размера — позиция ещё раз.
fn place(win: &WebviewWindow, x: i32, y: i32, w: i32, h: i32) {
    let _ = win.set_position(PhysicalPosition::new(x, y));
    let _ = win.set_size(PhysicalSize::new(w.max(1) as u32, h.max(1) as u32));
    let _ = win.set_position(PhysicalPosition::new(x, y));
}

/// Сохранённая позиция оверлея в пикселях: своя точная, иначе — логическая
/// от Горна на Electron (DIP), пересчитанная масштабом основного монитора.
fn saved_placement(p: &Value) -> Option<(i32, i32)> {
    let px = &p["placementPx"];
    if let (Some(x), Some(y)) = (px["x"].as_f64(), px["y"].as_f64()) {
        return Some((x as i32, y as i32));
    }
    let dip = &p["placement"];
    let (x, y) = (dip["x"].as_f64()?, dip["y"].as_f64()?);
    let scale = area_at(None).scale;
    Some(((x * scale).round() as i32, (y * scale).round() as i32))
}

/// Прямоугольник в пределах рабочей области.
fn keep_on(area: &Area, x: i32, y: i32, w: i32, h: i32) -> (i32, i32) {
    (
        x.clamp(area.x, (area.x + area.w - w).max(area.x)),
        y.clamp(area.y, (area.y + area.h - h).max(area.y)),
    )
}

/// Настройки для окон оверлея и плашек: только то, что влияет на отрисовку и звук.
pub fn prefs_payload(o: &Value) -> Value {
    let mut m = Map::new();
    for k in [
        "popups",
        "sound",
        "volume",
        "divineFe",
        "jackpotFe",
        "exaltedFe",
        "fontScale",
        "goalFe",
    ] {
        m.insert(k.into(), o[k].clone());
    }
    Value::Object(m)
}

/// Отпечаток полей среза, от которых зависят плашки (`main/toastFeed.ts`).
fn toast_key(s: &Value) -> String {
    let drops = |r: &Value| -> Value {
        Value::Array(
            r["drops"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .map(|d| {
                            json!([
                                d["configBaseId"],
                                d["qty"],
                                d["unitFe"],
                                d["totalFe"],
                                d["priceSource"],
                                d["name"]
                            ])
                        })
                        .collect()
                })
                .unwrap_or_default(),
        )
    };
    let cur = &s["current"];
    let cur_key = if cur.is_null() {
        Value::Null
    } else {
        json!([cur["startedAt"], cur["netFe"], cur["prison"], drops(cur)])
    };
    let recent: Vec<Value> = s["recent"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|r| {
                    json!([
                        r["startedAt"],
                        r["netFe"],
                        r["perHour"],
                        r["durationMs"],
                        drops(r)
                    ])
                })
                .collect()
        })
        .unwrap_or_default();
    json!([s["norm"], s["jackpotAutoFe"], cur_key, recent]).to_string()
}

#[derive(Default)]
struct OverlayState {
    open: bool,
}

#[derive(Default)]
struct ToastState {
    open: bool,
    anchor_bottom: i32,
    /// Левый край и ширина в пикселях: окно по центру монитора.
    left: i32,
    width: i32,
    scale: f64,
    last_key: Option<String>,
    /// Поколение «спрятать»: новая плашка отменяет отложенное скрытие.
    hide_gen: u64,
}

#[derive(Default)]
struct PanelState {
    ready: bool,
    pending: Option<Option<String>>,
    shown_tab: Option<String>,
    /// Последний срез, не отправленный спрятанному пульту.
    stale: bool,
}

/// Все окна поверх игры и раздача срезов.
pub struct Layers {
    app: AppHandle,
    settings: Arc<Mutex<Settings>>,
    overlay: Mutex<OverlayState>,
    toasts: Mutex<ToastState>,
    panel: Mutex<PanelState>,
    main_stale: Mutex<bool>,
    latest: Mutex<Value>,
}

impl Layers {
    pub fn new(app: AppHandle, settings: Arc<Mutex<Settings>>) -> Arc<Self> {
        Arc::new(Layers {
            app,
            settings,
            overlay: Default::default(),
            toasts: Default::default(),
            panel: Default::default(),
            main_stale: Mutex::new(false),
            latest: Mutex::new(Value::Null),
        })
    }

    fn win(&self, label: &str) -> Option<WebviewWindow> {
        self.app.get_webview_window(label)
    }

    fn overlay_prefs(&self) -> Value {
        self.settings.lock().unwrap().overlay().clone()
    }

    fn patch_overlay(&self, patch: Value) -> Value {
        let m = patch.as_object().cloned().unwrap_or_default();
        self.settings.lock().unwrap().patch_overlay(&m)
    }

    // ── срезы ────────────────────────────────────────────────────────────

    /// Новый срез движка: в каждое окно — по его правилам.
    pub fn publish(&self, snap: &Value) {
        *self.latest.lock().unwrap() = snap.clone();
        // Главное окно: свёрнутому или спрятанному — не слать, догонит при показе.
        match self.win("main") {
            Some(w) if w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false) => {
                let _ = w.emit_to(w.label(), "snapshot", snap);
                *self.main_stale.lock().unwrap() = false;
            }
            _ => *self.main_stale.lock().unwrap() = true,
        }
        if self.overlay.lock().unwrap().open {
            if let Some(w) = self.win("overlay") {
                let _ = w.emit_to(w.label(), "snapshot", snap);
            }
        }
        // Плашкам — урезанный срез и только если для них что-то изменилось.
        {
            let mut t = self.toasts.lock().unwrap();
            if t.open {
                let key = toast_key(snap);
                if t.last_key.as_deref() != Some(key.as_str()) {
                    t.last_key = Some(key);
                    let slim = json!({ "current": snap["current"], "recent": snap["recent"], "norm": snap["norm"], "jackpotAutoFe": snap["jackpotAutoFe"] });
                    if let Some(w) = self.win("toasts") {
                        let _ = w.emit_to(w.label(), "snapshot", slim);
                    }
                }
            }
        }
        match self.win("panel") {
            Some(w) if w.is_visible().unwrap_or(false) => {
                let _ = w.emit_to(w.label(), "snapshot", snap);
                self.panel.lock().unwrap().stale = false;
            }
            _ => self.panel.lock().unwrap().stale = true,
        }
    }

    /// Главное окно показали или развернули — догнать пропущенное.
    pub fn main_shown(&self) {
        let mut stale = self.main_stale.lock().unwrap();
        if *stale {
            if let Some(w) = self.win("main") {
                let _ = w.emit_to(w.label(), "snapshot", &*self.latest.lock().unwrap());
            }
            *stale = false;
        }
    }

    // ── оверлей ──────────────────────────────────────────────────────────

    pub fn overlay_open(&self) -> bool {
        self.overlay.lock().unwrap().open && self.win("overlay").is_some()
    }

    pub fn open_overlay(self: &Arc<Self>) {
        if self.overlay_open() {
            if let Some(w) = self.win("overlay") {
                show_inactive(&w);
            }
            return;
        }
        // Первое включение — открепленным: иначе игрок не узнал бы ни про перетаскивание, ни про режимы.
        if !self.overlay_prefs()["introShown"]
            .as_bool()
            .unwrap_or(false)
        {
            self.patch_overlay(json!({ "locked": false, "introShown": true }));
        }
        let p = self.overlay_prefs();
        let saved = saved_placement(&p);
        let area = area_at(saved);
        let scale = area.scale;
        let w = (mode_width(p["mode"].as_str().unwrap_or("compact"))
            * p["fontScale"].as_f64().unwrap_or(1.0)
            * scale)
            .round() as i32;
        let h = (INITIAL_HEIGHT * scale) as i32;
        let (x, y) = match saved {
            // По умолчанию — правый верхний угол, там меньше всего игрового HUD.
            None => (
                area.x + area.w - w - (24.0 * scale) as i32,
                area.y + (24.0 * scale) as i32,
            ),
            Some((x, y)) => keep_on(&area, x, y, w, h),
        };
        let win = match WebviewWindowBuilder::new(
            &self.app,
            "overlay",
            WebviewUrl::App("overlay.html".into()),
        )
        .title("Горн — оверлей")
        .transparent(true)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .shadow(false)
        .focused(false)
        .focusable(false)
        .visible(false)
        .additional_browser_args(BROWSER_ARGS)
        .build()
        {
            Ok(w) => w,
            Err(e) => {
                eprintln!("[оверлей] окно: {e}");
                return;
            }
        };
        tool_window(&win);
        place(&win, x, y, w, h);
        let _ = win.set_ignore_cursor_events(p["locked"].as_bool().unwrap_or(true));
        crate::log(&format!(
            "[оверлей] открыт: {x},{y} {w}x{h}, масштаб {scale}"
        ));
        let me = self.clone();
        win.on_window_event(move |ev| match ev {
            tauri::WindowEvent::Moved(pos) => {
                // Логическая позиция — для Горна на Electron, точная — для себя.
                let scale = area_at(Some((pos.x, pos.y))).scale;
                me.patch_overlay(json!({
                    "placement": { "x": (pos.x as f64 / scale).round(), "y": (pos.y as f64 / scale).round(), "displayId": 0 },
                    "placementPx": { "x": pos.x, "y": pos.y },
                }));
            }
            tauri::WindowEvent::Destroyed => {
                me.overlay.lock().unwrap().open = false;
                me.emit_main("overlay:enabled", json!(false));
            }
            _ => {}
        });
        // Прозрачное окно показывается после загрузки страницы — без белой вспышки.
        let shown = win.clone();
        let opacity = p["opacity"].as_f64().unwrap_or(0.9);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(400));
            apply_opacity(&shown, opacity);
            // Размер ещё раз — после того, как окно обжилось на своём мониторе.
            if let Ok(size) = shown.inner_size() {
                if size.width as i32 != w {
                    crate::log(&format!(
                        "[оверлей] ширина {} вместо {w} — поправлена",
                        size.width
                    ));
                    place(&shown, x, y, w, size.height as i32);
                }
            }
            show_inactive(&shown);
        });
        self.overlay.lock().unwrap().open = true;
        self.patch_overlay(json!({ "enabled": true }));
        self.emit_main("overlay:enabled", json!(true));
        // Первый срез — сразу, не дожидаясь пульса.
        let snap = self.latest.lock().unwrap().clone();
        if !snap.is_null() {
            let w2 = win.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(700));
                let _ = w2.emit_to(w2.label(), "snapshot", snap);
            });
        }
    }

    /// persist=false — закрытие при выходе: «оверлей включён» переживает перезапуск.
    pub fn close_overlay(&self, persist: bool) {
        self.overlay.lock().unwrap().open = false;
        if let Some(w) = self.win("overlay") {
            let _ = w.destroy();
        }
        if persist {
            self.patch_overlay(json!({ "enabled": false }));
        }
        self.emit_main("overlay:enabled", json!(false));
    }

    pub fn toggle_overlay(self: &Arc<Self>) -> bool {
        if self.overlay_open() {
            self.close_overlay(true);
        } else {
            self.open_overlay();
        }
        self.overlay_open()
    }

    pub fn set_locked(&self, locked: bool) {
        self.patch_overlay(json!({ "locked": locked }));
        if let Some(w) = self.win("overlay") {
            let _ = w.set_ignore_cursor_events(locked);
            let _ = w.emit_to(w.label(), "overlay:locked", locked);
        }
    }

    pub fn toggle_locked(&self) {
        let locked = self.overlay_prefs()["locked"].as_bool().unwrap_or(true);
        self.set_locked(!locked);
    }

    pub fn set_mode(&self, mode: &str) {
        if !OVERLAY_MODES.contains(&mode) {
            return;
        }
        let p = self.patch_overlay(json!({ "mode": mode }));
        self.resize_width(mode_width(mode) * p["fontScale"].as_f64().unwrap_or(1.0));
        if let Some(w) = self.win("overlay") {
            let _ = w.emit_to(w.label(), "overlay:mode", mode);
        }
    }

    pub fn cycle_mode(&self) {
        let cur = self.overlay_prefs()["mode"]
            .as_str()
            .unwrap_or("compact")
            .to_string();
        let i = OVERLAY_MODES.iter().position(|m| *m == cur).unwrap_or(0);
        self.set_mode(OVERLAY_MODES[(i + 1) % OVERLAY_MODES.len()]);
    }

    pub fn set_opacity(&self, v: f64) {
        let v = v.clamp(0.25, 1.0);
        self.patch_overlay(json!({ "opacity": v }));
        if let Some(w) = self.win("overlay") {
            apply_opacity(&w, v);
        }
    }

    /// Ширина в CSS-пикселях → окно, с оглядкой на край экрана.
    fn resize_width(&self, css_w: f64) {
        let Some(w) = self.win("overlay") else { return };
        let (Ok(pos), Ok(size)) = (w.outer_position(), w.inner_size()) else {
            return;
        };
        let area = area_at(Some((pos.x, pos.y)));
        let nw = (css_w * area.scale).round() as i32;
        let (x, y) = keep_on(&area, pos.x, pos.y, nw, size.height as i32);
        set_rect(&w, x, y, nw, size.height as i32);
    }

    /// Окно ровно по содержимому: лишняя прозрачная площадь дорога композитору.
    pub fn set_overlay_content(&self, height: f64, width: Option<f64>) {
        let Some(w) = self.win("overlay") else { return };
        let (Ok(pos), Ok(size)) = (w.outer_position(), w.inner_size()) else {
            return;
        };
        let area = area_at(Some((pos.x, pos.y)));
        let strip = self.overlay_prefs()["mode"].as_str() == Some("strip");
        let min_h = if strip { 24.0 } else { INITIAL_HEIGHT };
        let th = (height.ceil().clamp(min_h, MAX_HEIGHT) * area.scale).round() as i32;
        let tw = match width {
            Some(cw) if strip && cw > 0.0 => {
                (cw.ceil().clamp(STRIP_MIN, STRIP_MAX) * area.scale).round() as i32
            }
            _ => size.width as i32,
        };
        if (size.height as i32 - th).abs() < 2 && (size.width as i32 - tw).abs() < 2 {
            return;
        }
        let (x, y) = keep_on(&area, pos.x, pos.y, tw, th);
        set_rect(&w, x, y, tw, th);
    }

    /// Всплывашки, звук, масштаб, цель: оверлей и плашки узнают сразу.
    pub fn patch_prefs(self: &Arc<Self>, patch: &Value) {
        let mut clean = patch.as_object().cloned().unwrap_or_default();
        let clamp = |v: &Value, lo: f64, hi: f64| v.as_f64().map(|x| json!(x.clamp(lo, hi)));
        if let Some(v) = clean.get("volume").and_then(|v| clamp(v, 0.0, 1.0)) {
            clean.insert("volume".into(), v);
        }
        if let Some(v) = clean.get("goalFe").and_then(|v| v.as_f64()) {
            clean.insert("goalFe".into(), json!(gorn_core::csv::js_round(v.max(0.0))));
        }
        for k in ["jackpotFe", "exaltedFe", "divineFe"] {
            if let Some(v) = clean.get(k).and_then(|v| v.as_f64()) {
                clean.insert(k.into(), json!(v.max(0.0)));
            }
        }
        if let Some(v) = clean.get("fontScale").and_then(|v| clamp(v, 0.8, 1.6)) {
            clean.insert("fontScale".into(), v);
        }
        let scale_changed = clean.contains_key("fontScale");
        let p = self.settings.lock().unwrap().patch_overlay(&clean);
        if scale_changed {
            self.resize_width(
                mode_width(p["mode"].as_str().unwrap_or("compact"))
                    * p["fontScale"].as_f64().unwrap_or(1.0),
            );
        }
        let payload = prefs_payload(&p);
        if let Some(w) = self.win("overlay") {
            let _ = w.emit_to(w.label(), "overlay:prefs", payload.clone());
        }
        // Слой плашек живёт своим окном: появляется и исчезает вслед за настройками.
        self.sync_toasts();
        if let Some(w) = self.win("toasts") {
            let _ = w.emit_to(w.label(), "overlay:prefs", payload);
        }
    }

    fn emit_main(&self, ev: &str, v: Value) {
        if let Some(w) = self.win("main") {
            let _ = w.emit_to(w.label(), ev, v);
        }
    }

    // ── плашки ───────────────────────────────────────────────────────────

    /// Слой нужен, если включены всплывашки или звук: и то и другое живёт в нём.
    pub fn sync_toasts(&self) {
        let p = self.overlay_prefs();
        if p["popups"].as_bool().unwrap_or(true) || p["sound"].as_bool().unwrap_or(true) {
            self.open_toasts();
        } else {
            self.close_toasts();
        }
    }

    fn open_toasts(&self) {
        if self.toasts.lock().unwrap().open && self.win("toasts").is_some() {
            return;
        }
        // Монитор оверлея-панели (обычно с игрой), иначе основной.
        let saved = saved_placement(&self.overlay_prefs());
        let area = area_at(saved);
        let w = (TOAST_WIDTH * area.scale).round() as i32;
        // Сразу во всю высоту стека: размер окна плашек больше не меняется.
        let h = (TOAST_MAX * area.scale).round() as i32;
        // Кромка чуть выше середины: стек растёт вверх, не доставая до полосы босса.
        let anchor = (area.y as f64 + area.h as f64 * 0.36).round() as i32;
        let win = match WebviewWindowBuilder::new(
            &self.app,
            "toasts",
            WebviewUrl::App("toasts.html".into()),
        )
        .title("Горн — всплывающий доход")
        .transparent(true)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .shadow(false)
        .focused(false)
        .focusable(false)
        .visible(false)
        .additional_browser_args(BROWSER_ARGS)
        .build()
        {
            Ok(w) => w,
            Err(e) => {
                eprintln!("[плашки] окно: {e}");
                return;
            }
        };
        tool_window(&win);
        // Клики — всегда насквозь: это чистый «floating combat text».
        let _ = win.set_ignore_cursor_events(true);
        let left = area.x + (area.w - w) / 2;
        place(&win, left, anchor - h, w, h);
        crate::log(&format!(
            "[плашки] слой: {left},{} {w}x{h}, масштаб {}",
            anchor - h,
            area.scale
        ));
        let mut t = self.toasts.lock().unwrap();
        t.open = true;
        t.anchor_bottom = anchor;
        t.left = left;
        t.width = w;
        t.scale = area.scale;
        // Новое окно ничего не видело — следующий срез уходит в любом случае.
        t.last_key = None;
    }

    fn close_toasts(&self) {
        self.toasts.lock().unwrap().open = false;
        if let Some(w) = self.win("toasts") {
            let _ = w.destroy();
        }
    }

    /// Есть плашки — показать; кончились — спрятать с паузой на анимацию.
    pub fn toasts_active(self: &Arc<Self>, active: bool) {
        let gen = {
            let mut t = self.toasts.lock().unwrap();
            if !t.open {
                return;
            }
            t.hide_gen += 1;
            t.hide_gen
        };
        let Some(w) = self.win("toasts") else { return };
        if active {
            if !w.is_visible().unwrap_or(false) {
                show_inactive(&w);
            }
            return;
        }
        let me = self.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(600));
            if me.toasts.lock().unwrap().hide_gen == gen {
                if let Some(w) = me.win("toasts") {
                    hide(&w);
                }
            }
        });
    }

    /// Окно слоя — постоянного размера во всю высоту стека, плашки прижаты к
    /// нижней кромке стилями: подгонка прозрачного окна WebView2 на каждом кадре
    /// анимации плашек тормозила игру. Здесь — только возврат окна на место,
    /// если его сдвинули (смена масштаба монитора).
    pub fn toasts_height(&self, _height: f64) {
        let (anchor, scale, left, width) = {
            let t = self.toasts.lock().unwrap();
            if !t.open {
                return;
            }
            (t.anchor_bottom, t.scale, t.left, t.width)
        };
        let Some(w) = self.win("toasts") else { return };
        let (Ok(pos), Ok(size)) = (w.outer_position(), w.inner_size()) else {
            return;
        };
        let th = (TOAST_MAX * scale).round() as i32;
        if size.height as i32 == th && size.width as i32 == width && pos.x == left && pos.y == anchor - th {
            return;
        }
        set_rect(&w, left, anchor - th, width, th);
    }

    // ── пульт ────────────────────────────────────────────────────────────

    /// Создать заранее, спрятанным: первое открытие — сразу и с фокусом.
    pub fn warm_panel(self: &Arc<Self>) {
        self.ensure_panel();
    }

    fn ensure_panel(self: &Arc<Self>) -> Option<WebviewWindow> {
        if let Some(w) = self.win("panel") {
            return Some(w);
        }
        self.panel.lock().unwrap().ready = false;
        let win =
            WebviewWindowBuilder::new(&self.app, "panel", WebviewUrl::App("panel.html".into()))
                .title("Горн — пульт")
                .decorations(false)
                .always_on_top(true)
                .skip_taskbar(true)
                .resizable(false)
                .background_color(tauri::window::Color(0x14, 0x12, 0x17, 0xff))
                .visible(false)
                .additional_browser_args(BROWSER_ARGS)
                .build()
                .map_err(|e| eprintln!("[пульт] окно: {e}"))
                .ok()?;
        let me = self.clone();
        win.on_window_event(move |ev| {
            // Клик в игру — пульт уходит, как окно цены в APT.
            if let tauri::WindowEvent::Focused(false) = ev {
                me.hide_panel();
            }
        });
        Some(win)
    }

    /// Открыть на вкладке; та же вкладка уже открыта — закрыть.
    pub fn toggle_panel(self: &Arc<Self>, tab: Option<&str>) {
        let visible = self
            .win("panel")
            .is_some_and(|w| w.is_visible().unwrap_or(false));
        let shown = self.panel.lock().unwrap().shown_tab.clone();
        if visible && (tab.is_none() || tab.map(str::to_string) == shown) {
            self.hide_panel();
            return;
        }
        self.open_panel(tab.map(str::to_string));
    }

    fn open_panel(self: &Arc<Self>, tab: Option<String>) {
        let Some(win) = self.ensure_panel() else {
            return;
        };
        if !self.panel.lock().unwrap().ready {
            self.panel.lock().unwrap().pending = Some(tab);
            return;
        }
        let font = self.overlay_prefs()["fontScale"].as_f64().unwrap_or(1.0);
        let area = cursor_area();
        let w = (PANEL_WIDTH * font * area.scale).round() as i32;
        let h = (PANEL_HEIGHT * font * area.scale).round() as i32;
        // Справа по центру: слева и в центре — персонаж и бой, сверху справа — миникарта.
        let margin = (24.0 * area.scale).max(area.w as f64 * 0.06) as i32;
        place(
            &win,
            area.x + area.w - w - margin,
            area.y + (area.h - h) / 2,
            w,
            h,
        );
        // Сначала содержимое, потом показ: иначе мелькает пустая подложка.
        let _ = win.emit_to(win.label(), "panel:open", json!({ "tab": tab, "scale": font }));
        let _ = win.show();
        let _ = win.set_focus();
        let stale = std::mem::replace(&mut self.panel.lock().unwrap().stale, false);
        if stale {
            let _ = win.emit_to(win.label(), "snapshot", &*self.latest.lock().unwrap());
        }
        self.panel.lock().unwrap().shown_tab = tab;
    }

    /// Страница пульта подписалась на события.
    pub fn panel_ready(self: &Arc<Self>) {
        let pending = {
            let mut p = self.panel.lock().unwrap();
            p.ready = true;
            p.pending.take()
        };
        if let Some(tab) = pending {
            self.open_panel(tab);
        }
    }

    pub fn hide_panel(&self) {
        let Some(w) = self.win("panel") else { return };
        if !w.is_visible().unwrap_or(false) {
            return;
        }
        let _ = w.emit_to(w.label(), "panel:hidden", ());
        let _ = w.hide();
        self.panel.lock().unwrap().shown_tab = None;
    }

    pub fn panel_tab(&self, tab: &str) {
        self.panel.lock().unwrap().shown_tab = Some(tab.to_string());
    }

    /// Выход из приложения: окна поверх игры — без записи «оверлей выключен».
    pub fn shutdown(&self) {
        for l in ["overlay", "toasts", "panel"] {
            if let Some(w) = self.win(l) {
                let _ = w.destroy();
            }
        }
    }
}

/// Прозрачность оверлея — у страницы: окну WebView2 альфу целиком не задать.
fn apply_opacity(w: &WebviewWindow, v: f64) {
    let _ = w.eval(format!("document.documentElement.style.opacity = '{v}'"));
}
