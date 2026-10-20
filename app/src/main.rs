//! Горн на Tauri 2 — пока подсказки в сезонной тюрьме: лог игры → кадр
//! экрана → советник на Rust (gorn-prison) → стрелки поверх игры.
//! Трекер валюты остаётся в Горне на Electron до этапа 4.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod assist;
mod capture;
mod logtail;
mod overlay;
mod shots;
mod triggers;

use std::sync::mpsc::Sender;
use std::sync::Mutex;

use serde_json::{json, Value};
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Listener, Manager};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

use assist::{Assist, Event, Paths};

/// Настройки: подсказки и сохранение кадров (кадры нужны аудиту — по умолчанию да).
struct Settings {
    path: std::path::PathBuf,
    enabled: bool,
    save_shots: bool,
}

impl Settings {
    fn load(path: std::path::PathBuf) -> Self {
        let v: Value = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
        Settings {
            path,
            enabled: v["prisonHints"].as_bool().unwrap_or(true),
            save_shots: v["prisonSaveShots"].as_bool().unwrap_or(true),
        }
    }

    fn save(&self) {
        if let Some(d) = self.path.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(&self.path, json!({ "prisonHints": self.enabled, "prisonSaveShots": self.save_shots }).to_string());
    }
}

struct AppState {
    tx: Mutex<Sender<Event>>,
    settings: Mutex<Settings>,
}

/// `gorn --test-capture файл.png`: кадр монитора под курсором через WGC —
/// в PNG и в распознавание (проверка захвата без игры).
fn test_capture(out: &str) {
    let d = overlay::display_under_cursor().expect("монитор");
    let mut s = capture::ScreenStream::default();
    s.ensure(d.hmonitor).expect("захват");
    let t0 = std::time::Instant::now();
    let img = s.grab(std::time::Duration::ZERO, std::time::Duration::from_millis(2000), std::time::Duration::from_secs(60)).expect("кадр");
    let ms = t0.elapsed().as_millis();
    let f = gorn_prison::recognize::Frame { width: img.width, height: img.height, data: &img.data };
    let t1 = std::time::Instant::now();
    let (view, geo) = gorn_prison::recognize::read(&f, None, Default::default());
    let rms = t1.elapsed().as_millis();
    let file = std::fs::File::create(out).expect("файл");
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), img.width as u32, img.height as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.write_header().unwrap().write_image_data(&img.data).unwrap();
    // Приложение без консоли: итог — в файл рядом с кадром.
    let line = format!(
        "монитор {}×{} ×{:.2}, кадр {}×{} за {ms} мс, распознавание {rms} мс: {}, геометрия {:?}
",
        d.w,
        d.h,
        d.scale,
        img.width,
        img.height,
        view.to_json()["screen"],
        geo.map(|g| g.s)
    );
    let _ = std::fs::write(format!("{out}.txt"), line);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--test-capture") {
        test_capture(args.get(2).map(String::as_str).unwrap_or("capture.png"));
        return;
    }
    let manual = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyP);
    tauri::Builder::default()
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(move |app, sc, ev| {
                    if sc == &manual && ev.state() == ShortcutState::Pressed {
                        if let Some(st) = app.try_state::<AppState>() {
                            let _ = st.tx.lock().unwrap().send(Event::Manual);
                        }
                    }
                })
                .build(),
        )
        .setup(move |app| {
            let data = app.path().app_data_dir()?;
            let pictures = app.path().picture_dir()?;
            let settings = Settings::load(data.join("settings.json"));
            let paths = Paths {
                state: data.join("prison-state.json"),
                display: data.join("prison-display.json"),
                shots: pictures.join("Горн Next — тюрьма"),
            };
            let shots_dir = paths.shots.clone();
            let (tx, rx) = std::sync::mpsc::channel::<Event>();
            let mut assist = Assist::new(app.handle().clone(), tx.clone(), paths, settings.save_shots);
            if !settings.enabled {
                let _ = tx.send(Event::SetEnabled(false));
            }

            // Лог игры.
            let log = logtail::find_log_directory().map(|d| d.join("UE_game.log"));
            assist.log_found = log.is_some();
            match &log {
                Some(p) => {
                    eprintln!("[горн] лог игры: {}", p.display());
                    let p = p.clone();
                    let tx_log = tx.clone();
                    let (ltx, lrx) = std::sync::mpsc::channel::<Vec<String>>();
                    std::thread::spawn(move || logtail::tail(p, ltx));
                    std::thread::spawn(move || {
                        for lines in lrx {
                            if tx_log.send(Event::Lines(lines)).is_err() {
                                break;
                            }
                        }
                    });
                }
                None => eprintln!("[горн] лог игры не найден — подсказки только по Ctrl+Alt+P"),
            }
            std::thread::Builder::new().name("горн-тюрьма".into()).spawn(move || assist.run(rx))?;

            // Страница окна стрелок готова.
            let tx_ready = tx.clone();
            app.listen("prison:ready", move |_| {
                let _ = tx_ready.send(Event::OverlayReady);
            });

            // Трей.
            let hints = CheckMenuItem::with_id(app, "hints", "Подсказки в тюрьме", true, settings.enabled, None::<&str>)?;
            let save = CheckMenuItem::with_id(app, "save", "Сохранять кадры (для разбора)", true, settings.save_shots, None::<&str>)?;
            let open = MenuItem::with_id(app, "open", "Открыть папку кадров", true, None::<&str>)?;
            let shot = MenuItem::with_id(app, "shot", "Подсказка по экрану сейчас (Ctrl+Alt+P)", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Выход", true, None::<&str>)?;
            let sep = PredefinedMenuItem::separator(app)?;
            let menu = Menu::with_items(app, &[&hints, &save, &shot, &open, &sep, &quit])?;
            let tip = if log.is_some() { "Горн Next — подсказки в тюрьме" } else { "Горн Next — лог игры не найден" };
            TrayIconBuilder::with_id("tray")
                .icon(app.default_window_icon().cloned().expect("иконка"))
                .tooltip(tip)
                .menu(&menu)
                .on_menu_event(move |app, ev| {
                    let st = app.state::<AppState>();
                    match ev.id().as_ref() {
                        "hints" => {
                            let mut s = st.settings.lock().unwrap();
                            s.enabled = !s.enabled;
                            s.save();
                            let _ = hints.set_checked(s.enabled);
                            let _ = st.tx.lock().unwrap().send(Event::SetEnabled(s.enabled));
                        }
                        "save" => {
                            let mut s = st.settings.lock().unwrap();
                            s.save_shots = !s.save_shots;
                            s.save();
                            let _ = save.set_checked(s.save_shots);
                            let _ = st.tx.lock().unwrap().send(Event::SetSaveShots(s.save_shots));
                        }
                        "open" => {
                            let _ = std::fs::create_dir_all(&shots_dir);
                            let _ = std::process::Command::new("explorer").arg(&shots_dir).spawn();
                        }
                        "shot" => {
                            let _ = st.tx.lock().unwrap().send(Event::Manual);
                        }
                        "quit" => app.exit(0),
                        _ => {}
                    }
                })
                .build(app)?;

            if let Err(e) = app.global_shortcut().register(manual) {
                eprintln!("[горн] Ctrl+Alt+P занят (Горн на Electron?): {e}");
            }
            app.manage(AppState { tx: Mutex::new(tx), settings: Mutex::new(settings) });
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("Горн: запуск")
        .run(|_app, ev| {
            // Окон нет, кроме стрелок: приложение живёт в трее.
            if let tauri::RunEvent::ExitRequested { api, code, .. } = ev {
                if code.is_none() {
                    api.prevent_exit();
                }
            }
        });
}
