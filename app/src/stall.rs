//! Цены слотов сумки поверх «Моего прилавка» в торговом доме.
//!
//! Что лежит в каждом слоте, знает лог (вкладка «Сумка» пульта): клетки
//! сетки прилавка идут в том же порядке — слот = ряд × столбцов + столбец.
//! С экрана берётся только то, чего в логе нет: открыта ли вкладка «Мой
//! прилавок» и какая вкладка сумки подсвечена. Открытие и закрытие торгового
//! дома и окна поверх него (выставить лот, подтверждение) — строки лога
//! `DispatchPageRunChange`; пока дом открыт, кадр проверяется раз в секунду
//! и сразу после кликов по вкладкам и изменений сумки. Из кадра берётся
//! только полоса вкладок сверху слева (CROP): с видеокарты копируется она
//! одна, а не весь экран. Пока дом закрыт, экран не снимается вовсе.
//!
//! Геометрия снята с экрана 3440×1440 (21:9) в опорных пикселях высоты 1080:
//! интерфейс масштабируется по высоте, лоты прижаты влево, сетка сумки
//! тянется до правого края. Столбцов — сколько клеток опорного размера
//! влезает по ширине, с округлением, и сетка целиком (шаг, клетки, ряды)
//! растягивается или сжимается, чтобы последний столбец дошёл до поля справа:
//! на 21:9 — 23 столбца почти без растяжения, на 16:9 — 15 столбцов,
//! клетки мельче на 2,5 % (снимок 16:9). Другие пропорции не проверены.

use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use gorn_core::engine::BagGrid;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, SetWindowDisplayAffinity, SetWindowLongPtrW, SetWindowPos, ShowWindow,
    GWL_EXSTYLE, HWND_TOPMOST, SWP_NOACTIVATE, SW_HIDE, SW_SHOWNOACTIVATE, WDA_EXCLUDEFROMCAPTURE,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
};

use crate::capture::{Image, ScreenStream};
use crate::overlay::{display_under_cursor, Display};
use crate::tracker::Tracker;

/// Вкладка «Мой прилавок» в строке вкладок торгового дома (подсвечена жёлтым).
const MY_STALL: [f64; 4] = [382.0, 5.0, 511.0, 44.0];
/// Центры иконок вкладок сумки над сеткой: снаряжение, навыки, материалы, прочее.
const BAG_TABS: [(f64, i64); 4] = [(760.0, 100), (816.8, 101), (873.5, 102), (931.6, 103)];
const BAG_TAB_Y: f64 = 92.9;
const BAG_TAB_R: f64 = 11.5;
/// Сетка сумки на 21:9: левый край и верх первой клетки, шаг, размер клетки,
/// поле справа. Шаг и клетки — до растяжения по ширине (`grid`).
const GRID_X: f64 = 745.8;
const GRID_Y: f64 = 126.4;
const PITCH_X: f64 = 78.74;
const PITCH_Y: f64 = 111.0;
const CELL_W: f64 = 71.0;
const CELL_H: f64 = 103.2;
const GRID_RIGHT: f64 = 29.7;
/// Слотов на странице сумки.
const SLOTS: i64 = 120;
/// Что вырезать из кадра: всё, что смотрит `recognize`, с запасом (опорные пиксели).
const CROP: [f64; 4] = [0.0, 0.0, 960.0, 116.0];

/// Без событий кадр проверяется так часто, пока торговый дом открыт.
const POLL: Duration = Duration::from_secs(1);
/// После строки лога (клик по вкладке, изменение сумки) — проверить через столько.
const SOON: Duration = Duration::from_millis(250);

pub enum Event {
    Lines(Vec<String>),
    Ready,
}

/// Что видно на экране: открыта страница сумки `page` в сетке из `cols` столбцов.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StallView {
    pub page: i64,
    pub cols: i64,
    /// Во сколько раз сетка крупнее снятой на 21:9.
    pub scale: f64,
    /// Опорный пиксель → пиксель кадра.
    pub k: f64,
}

impl StallView {
    /// Клетка слота в пикселях кадра: x0, y0, x1, y1.
    pub fn cell(&self, slot: i64) -> [f64; 4] {
        let (row, col) = (slot / self.cols, slot % self.cols);
        let x = GRID_X + col as f64 * PITCH_X * self.scale;
        let y = GRID_Y + row as f64 * PITCH_Y * self.scale;
        let (w, h) = (CELL_W * self.scale, CELL_H * self.scale);
        [x * self.k, y * self.k, (x + w) * self.k, (y + h) * self.k]
    }
}

/// Столбцы сетки и её масштаб при ширине экрана `width` (опорные пиксели):
/// клетки опорного размера по ширине с округлением, затем сетка растягивается
/// так, чтобы последний столбец кончался у поля справа.
fn grid(width: f64) -> (i64, f64) {
    // Ширина под сетку с одним лишним промежутком: ровно столбцы × шаг.
    let span = width - GRID_X - GRID_RIGHT + (PITCH_X - CELL_W);
    let cols = (span / PITCH_X).round().max(1.0);
    (cols as i64, span / (cols * PITCH_X))
}

fn yellow(img: &Image, x: usize, y: usize) -> bool {
    let i = (y * img.width + x) * 4;
    let (r, g, b) = (img.data[i], img.data[i + 1], img.data[i + 2]);
    r > 180 && g > 140 && b < 90
}

/// Доля жёлтых точек в прямоугольнике (опорные пиксели; кадр может быть вырезан).
fn yellow_share(img: &Image, k: f64, b: [f64; 4]) -> f64 {
    let (ox, oy) = img.origin;
    let x0 = ((b[0] * k) as usize).saturating_sub(ox);
    let y0 = ((b[1] * k) as usize).saturating_sub(oy);
    let x1 = ((b[2] * k) as usize).saturating_sub(ox).min(img.width);
    let y1 = ((b[3] * k) as usize).saturating_sub(oy).min(img.height);
    let (mut hit, mut all) = (0usize, 0usize);
    for y in (y0..y1).step_by(2) {
        for x in (x0..x1).step_by(2) {
            all += 1;
            hit += yellow(img, x, y) as usize;
        }
    }
    if all == 0 {
        0.0
    } else {
        hit as f64 / all as f64
    }
}

/// «Мой прилавок» на экране и какая вкладка сумки на нём выбрана.
pub fn recognize(img: &Image) -> Option<StallView> {
    let k = img.full.1 as f64 / 1080.0;
    let width = img.full.0 as f64 / k;
    if width < GRID_X + CELL_W + GRID_RIGHT {
        return None;
    }
    if yellow_share(img, k, MY_STALL) < 0.12 {
        return None;
    }
    let shares: Vec<(f64, i64)> = BAG_TABS
        .iter()
        .map(|&(x, page)| {
            let b = [x - BAG_TAB_R, BAG_TAB_Y - BAG_TAB_R, x + BAG_TAB_R, BAG_TAB_Y + BAG_TAB_R];
            (yellow_share(img, k, b), page)
        })
        .collect();
    let (best, page) = shares.iter().copied().fold((0.0, 0), |a, b| if b.0 > a.0 { b } else { a });
    // Подсвечена ровно одна: остальные серые.
    let second = shares.iter().filter(|s| s.1 != page).map(|s| s.0).fold(0.0, f64::max);
    if best < 0.08 || second > best / 3.0 {
        return None;
    }
    let (cols, scale) = grid(width);
    Some(StallView { page, cols, scale, k })
}

/// Что делать по строкам лога.
#[derive(Default, PartialEq, Debug)]
struct LineNews {
    open: Option<bool>,
    dialog: Option<bool>,
    recheck: bool,
}

fn read_lines(lines: &[String]) -> LineNews {
    let mut n = LineNews::default();
    for l in lines {
        if let Some(rest) = l.split("DispatchPageRunChange PageName = ").nth(1) {
            let (name, state) = rest.split_once(" , PageRunState = ").unwrap_or((rest, ""));
            let run = state.trim_start().starts_with("Run");
            match name.trim() {
                "AuctionHouseV2" => n.open = Some(run),
                // Окна поверх прилавка закрывают сетку.
                "AuctionHousePutOnItemV2" | "ConfirmV2" | "AuctionHouseShowItemGoodV2" | "PCBag" => {
                    n.dialog = Some(run)
                }
                _ => {}
            }
            n.recheck = true;
        } else if l.contains("BagTabButtonItem")
            || l.contains("UITitleV2Tab")
            || l.contains("BagMgr@")
            || l.contains("ItemChange@")
        {
            n.recheck = true;
        }
    }
    n
}

pub struct Stall {
    tracker: Tracker,
    layer: StallLayer,
    stream: ScreenStream,
    open: bool,
    dialog: bool,
    display: Option<Display>,
    check_at: Option<Instant>,
    /// Проверка по строке лога: ждать свежий спокойный кадр, а не брать последний.
    urgent: bool,
}

impl Stall {
    pub fn new(app: AppHandle, tracker: Tracker) -> Self {
        Stall {
            tracker,
            layer: StallLayer::new(app),
            stream: ScreenStream::default(),
            open: false,
            dialog: false,
            display: None,
            check_at: None,
            urgent: false,
        }
    }

    pub fn run(mut self, rx: Receiver<Event>) {
        loop {
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(Event::Lines(lines)) => self.on_lines(&lines),
                Ok(Event::Ready) => self.layer.on_ready(),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            if self.check_at.is_some_and(|t| Instant::now() >= t) {
                self.check();
            }
        }
    }

    fn on_lines(&mut self, lines: &[String]) {
        let n = read_lines(lines);
        if let Some(open) = n.open {
            if open && !self.open {
                // Торговый дом открывают кликом в игре: игра — на мониторе под курсором.
                self.display = display_under_cursor();
                self.dialog = false;
            }
            self.open = open;
            if !open {
                self.close();
                return;
            }
        }
        if let Some(d) = n.dialog {
            self.dialog = d;
            if d {
                self.layer.clear();
            }
        }
        if self.open && n.recheck {
            let soon = Instant::now() + SOON;
            self.check_at = Some(self.check_at.map_or(soon, |t| t.min(soon)));
            self.urgent = true;
        }
    }

    fn close(&mut self) {
        self.open = false;
        self.check_at = None;
        self.layer.clear();
        self.stream.stop();
    }

    fn check(&mut self) {
        self.check_at = Some(Instant::now() + POLL);
        let urgent = std::mem::take(&mut self.urgent);
        if !self.open || self.dialog {
            return;
        }
        let Some(d) = self.display else { return };
        self.stream.set_crop(Some(CROP.map(|v| v / 1080.0)));
        if let Err(e) = self.stream.ensure(d.hmonitor) {
            eprintln!("[прилавок] {e}");
            self.close();
            return;
        }
        // Опрос — последний готовый кадр; после клика — свежий, когда экран успокоится.
        let img = if urgent {
            self.stream.grab(Duration::from_millis(150), Duration::from_millis(800), Duration::from_secs(2)).ok()
        } else {
            self.stream.latest(Duration::from_millis(1500))
        };
        let Some(img) = img else { return };
        let Some(view) = recognize(&img) else {
            self.layer.clear();
            return;
        };
        let Some(grid) = self.tracker.call(|e| e.query_bag()) else { return };
        self.layer.show(&d, &labels(&grid, &view), img.full.0, img.full.1);
    }
}

/// Ценники клеток страницы, которую видно (в пикселях кадра).
fn labels(grid: &BagGrid, view: &StallView) -> Value {
    let Some(page) = grid.pages.iter().find(|p| p.page_id == view.page) else {
        return json!({ "cells": [], "total": 0.0 });
    };
    let cells: Vec<Value> = page
        .slots
        .iter()
        .filter(|s| s.slot_id < SLOTS)
        .map(|s| {
            json!({
                "box": view.cell(s.slot_id),
                "fe": s.item.total_fe,
                "unit": s.item.unit_fe,
                "qty": s.item.qty,
                "known": s.known,
            })
        })
        .collect();
    let first = view.cell(0);
    json!({
        "cells": cells,
        "total": page.value_fe,
        "unpriced": page.unpriced,
        // Итог страницы — над сеткой, правее иконок вкладок (они не масштабируются).
        "totalAt": [first[0] + (BAG_TABS[3].0 - GRID_X + 40.0) * view.k, BAG_TAB_Y * view.k],
    })
}

/// Прозрачное окно во весь монитор: клики насквозь, фокус не берёт, в захват
/// не попадает (как окно стрелок тюрьмы). Страница — `ui/public/stall.js`.
struct StallLayer {
    app: AppHandle,
    window: Option<WebviewWindow>,
    ready: bool,
    pending: Option<Value>,
    visible: bool,
    at: Option<Display>,
    last: Option<Value>,
}

impl StallLayer {
    fn new(app: AppHandle) -> Self {
        StallLayer { app, window: None, ready: false, pending: None, visible: false, at: None, last: None }
    }

    fn on_ready(&mut self) {
        self.ready = true;
        if let Some(p) = self.pending.take() {
            self.send(p);
        }
    }

    fn ensure(&mut self) -> Option<WebviewWindow> {
        if let Some(w) = &self.window {
            return Some(w.clone());
        }
        let win = WebviewWindowBuilder::new(&self.app, "stall", WebviewUrl::App("stall.html".into()))
            .title("Горн — цены на прилавке")
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
            .map_err(|e| eprintln!("[прилавок] окно: {e}"))
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
            let _ = win.emit_to(win.label(), "stall:prices", payload);
        }
    }

    /// Ценники в пикселях кадра (fw × fh) → CSS-пиксели окна на мониторе d.
    fn show(&mut self, d: &Display, labels: &Value, fw: usize, fh: usize) {
        let sx = d.w as f64 / fw as f64 / d.scale;
        let sy = d.h as f64 / fh as f64 / d.scale;
        let mut payload = labels.clone();
        if let Some(cells) = payload["cells"].as_array_mut() {
            for c in cells {
                if let Some(b) = c["box"].as_array() {
                    let v: Vec<f64> = b.iter().filter_map(Value::as_f64).collect();
                    c["box"] = json!([v[0] * sx, v[1] * sy, v[2] * sx, v[3] * sy]);
                }
            }
        }
        if let Some(t) = payload["totalAt"].as_array() {
            let v: Vec<f64> = t.iter().filter_map(Value::as_f64).collect();
            payload["totalAt"] = json!([v[0] * sx, v[1] * sy]);
        }
        if self.visible && self.last.as_ref() == Some(&payload) {
            return;
        }
        let Some(win) = self.ensure() else { return };
        if self.at != Some(*d) {
            if let Ok(hwnd) = win.hwnd() {
                unsafe {
                    let _ = SetWindowPos(HWND(hwnd.0), Some(HWND_TOPMOST), d.x, d.y, d.w, d.h, SWP_NOACTIVATE);
                }
            }
            self.at = Some(*d);
        }
        self.last = Some(payload.clone());
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

    fn clear(&mut self) {
        if !self.visible {
            return;
        }
        self.pending = None;
        self.last = None;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn line(body: &str) -> String {
        format!("[2026.10.24-11.14.12:614][734]TLLua: Display: [Game] {body}")
    }

    #[test]
    fn log_lines_open_close_and_dialogs() {
        let n = read_lines(&[line("TipMsgShowMgr@DispatchPageRunChange PageName = AuctionHouseV2 , PageRunState = Run ")]);
        assert_eq!(n, LineNews { open: Some(true), dialog: None, recheck: true });
        let n = read_lines(&[line("TipMsgShowMgr@DispatchPageRunChange PageName = AuctionHousePutOnItemV2 , PageRunState = Run ")]);
        assert_eq!(n.dialog, Some(true));
        let n = read_lines(&[line("TipMsgShowMgr@DispatchPageRunChange PageName = AuctionHouseV2 , PageRunState = Destory")]);
        assert_eq!(n.open, Some(false));
        let n = read_lines(&[line("BagMgr@:Modfy BagItem PageId = 102 SlotId = 1 ConfigBaseId = 100300 Num = 5")]);
        assert_eq!(n, LineNews { open: None, dialog: None, recheck: true });
    }

    #[test]
    fn grid_fills_width() {
        // 21:9: 23 столбца, как сняты.
        let (cols, scale) = grid(3440.0 / (1440.0 / 1080.0));
        assert_eq!(cols, 23);
        assert!((scale - 1.0).abs() < 0.01);
        // 16:9 по снимку 2000×1125: 15 столбцов, клетка 0 — x 777,5…849,5,
        // клетка 14 кончается на 1969,5; ряд 3 начинается на 469,5.
        let k = 1125.0 / 1080.0;
        let (cols, scale) = grid(2000.0 / k);
        assert_eq!(cols, 15);
        let v = StallView { page: 102, cols, scale, k };
        let near = |a: f64, b: f64| (a - b).abs() < 2.0;
        assert!(near(v.cell(0)[0], 777.5) && near(v.cell(0)[2], 849.5), "{:?}", v.cell(0));
        assert!(near(v.cell(1)[0], 857.5), "{:?}", v.cell(1));
        assert!(near(v.cell(14)[2], 1969.5), "{:?}", v.cell(14));
        assert!(near(v.cell(45)[1], 469.5) && near(v.cell(15)[0], 777.5), "{:?}", v.cell(45));
    }

    #[test]
    fn cells_follow_slot_order() {
        let v = StallView { page: 103, cols: 23, scale: 1.0, k: 1440.0 / 1080.0 };
        let a = v.cell(0);
        let b = v.cell(23);
        assert!((b[0] - a[0]).abs() < 1e-9 && b[1] > a[1]);
        assert!(v.cell(22)[2] <= 3440.0);
    }

    /// Снимок экрана PNG (RGB или RGBA): STALL_PNG=путь STALL_PAGE=102 [STALL_COLS=15]
    /// cargo test -- --ignored
    #[test]
    #[ignore]
    fn recognizes_screenshot() {
        let path = std::env::var("STALL_PNG").unwrap();
        let dec = png::Decoder::new(std::fs::File::open(&path).unwrap());
        let mut r = dec.read_info().unwrap();
        let mut buf = vec![0; r.output_buffer_size()];
        let info = r.next_frame(&mut buf).unwrap();
        let (w, h) = (info.width as usize, info.height as usize);
        let ch = info.line_size / w;
        let mut data = vec![255u8; w * h * 4];
        for i in 0..w * h {
            data[i * 4..i * 4 + 3].copy_from_slice(&buf[i * ch..i * ch + 3]);
        }
        let want: i64 = std::env::var("STALL_PAGE").unwrap().parse().unwrap();
        let v = recognize(&Image::whole(w, h, data.clone()));
        println!("{path}: {v:?}");
        let cols: i64 = std::env::var("STALL_COLS").map_or(23, |c| c.parse().unwrap());
        assert_eq!(v.map(|v| (v.page, v.cols)), Some((want, cols)));
        // То же по вырезанной полосе, как её отдаёт поток (CROP).
        let k = h as f64 / 1080.0;
        let (cw, ch) = ((CROP[2] * k) as usize, (CROP[3] * k) as usize);
        let mut cut = vec![0u8; cw * ch * 4];
        for y in 0..ch {
            cut[y * cw * 4..(y + 1) * cw * 4].copy_from_slice(&data[y * w * 4..y * w * 4 + cw * 4]);
        }
        let img = Image { full: (w, h), origin: (0, 0), ..Image::whole(cw, ch, cut) };
        assert_eq!(recognize(&img), v);
    }
}
