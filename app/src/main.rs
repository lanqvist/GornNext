//! Горн на Tauri 2: трекер валюты (движок `gorn-core`, интерфейс React из
//! `ui/`) и подсказки в сезонной тюрьме (советник `gorn-prison`, стрелки
//! поверх игры). Мост `window.gorn` — одна команда `gorn` с каналами, как у
//! ipcMain Горна на Electron, и события Tauri с теми же именами.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod assist;
mod capture;
mod icons;
mod overlay;
mod settings;
mod shots;
mod tracker;
mod triggers;

use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::Mutex;

use gorn_core::engine::{CatalogueItem, EngineOptions};
use serde_json::{json, Value};
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Listener, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

use assist::{Assist, Event, Paths};
use settings::Settings;
use tracker::{name_language, ui_lang, Tracker};

struct AppState {
    tx: Mutex<Sender<Event>>,
    settings: Mutex<Settings>,
    tracker: Tracker,
    shots_dir: PathBuf,
    /// Выход по-настоящему: отличает «закрыть в трей» от завершения.
    quitting: Mutex<bool>,
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
        "монитор {}×{} ×{:.2}, кадр {}×{} за {ms} мс, распознавание {rms} мс: {}, геометрия {:?}\n",
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

/// Локаль системы — язык новой установки.
fn system_locale() -> Option<String> {
    let mut buf = [0u16; 85];
    let n = unsafe { windows::Win32::Globalization::GetUserDefaultLocaleName(&mut buf) };
    (n > 1).then(|| String::from_utf16_lossy(&buf[..n as usize - 1]))
}

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

fn create_main_window(app: &AppHandle, visible: bool) -> tauri::Result<()> {
    let w = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("Горн — трекер Torchlight: Infinite")
        .inner_size(1280.0, 820.0)
        .min_inner_size(940.0, 620.0)
        .background_color(tauri::window::Color(0x0e, 0x0f, 0x13, 0xff))
        .visible(visible)
        // Внешние ссылки — в браузере, а не внутри приложения; только http(s).
        .on_navigation(|url| {
            if url.scheme() == "http" && url.host_str().is_some_and(|h| h.ends_with(".localhost")) || url.scheme() == "tauri" {
                return true;
            }
            if matches!(url.scheme(), "http" | "https") {
                let _ = std::process::Command::new("explorer").arg(url.as_str()).spawn();
            }
            false
        })
        .build()?;
    let app2 = app.clone();
    w.on_window_event(move |ev| {
        if let tauri::WindowEvent::CloseRequested { api, .. } = ev {
            let st = app2.state::<AppState>();
            let to_tray = st.settings.lock().unwrap().bool("closeToTray");
            if to_tray && !*st.quitting.lock().unwrap() {
                api.prevent_close();
                if let Some(w) = app2.get_webview_window("main") {
                    let _ = w.hide();
                }
            } else {
                // На Windows закрытие главного окна — выход.
                *st.quitting.lock().unwrap() = true;
                st.tracker.shutdown();
                app2.exit(0);
            }
        }
    });
    Ok(())
}

fn arg<'a>(args: &'a [Value], i: usize) -> &'a Value {
    args.get(i).unwrap_or(&Value::Null)
}

/// Мост `window.gorn`: канал и аргументы как у ipcMain Горна на Electron.
#[tauri::command]
async fn gorn(app: AppHandle, channel: String, args: Vec<Value>) -> Result<Value, String> {
    let st = app.state::<AppState>();
    let tr = st.tracker.clone();
    let a0 = arg(&args, 0).clone();
    let a1 = arg(&args, 1).clone();
    let v = match channel.as_str() {
        "snapshot:get" => {
            let s = st.tracker.latest.lock().unwrap().clone();
            s
        }
        "session:reset" => {
            tr.call(|e| e.reset_session());
            Value::Null
        }
        "runs:query" => {
            let q = a0.as_str().unwrap_or("").to_string();
            tr.call(move |e| serde_json::to_value(e.query_runs(&q)).unwrap_or(Value::Null)).unwrap_or(Value::Null)
        }
        "runs:exportCsv" => {
            let now = chrono::Local::now();
            let docs = app.path().document_dir().unwrap_or_default();
            let picked = app
                .dialog()
                .file()
                .set_title("Экспорт забегов")
                .set_directory(docs)
                .set_file_name(format!("gorn-{}.csv", now.format("%Y-%m-%d")))
                .add_filter("CSV для Excel", &["csv"])
                .blocking_save_file();
            match picked.and_then(|p| p.into_path().ok()) {
                None => json!({ "status": "cancelled" }),
                Some(path) => {
                    let csv = tr.call(|e| e.export_runs_csv()).unwrap_or_default();
                    match std::fs::write(&path, csv) {
                        Ok(()) => json!({ "status": "saved", "path": path.to_string_lossy() }),
                        Err(e) => json!({ "status": "error", "message": e.to_string() }),
                    }
                }
            }
        }
        "prices:query" => {
            let q = a0.as_str().unwrap_or("").to_string();
            tr.call(move |e| serde_json::to_value(e.query_prices(&q)).unwrap_or(Value::Null)).unwrap_or(json!([]))
        }
        "items:setExcluded" => {
            let Some(id) = a0.as_i64() else { return Ok(json!(st.settings.lock().unwrap().excluded())) };
            let on = a1.as_bool().unwrap_or(false);
            let list = st.settings.lock().unwrap().set_excluded(id, on);
            tr.post(move |e| e.set_excluded(id, on));
            json!(list)
        }
        "settings:setMinDropFe" => {
            let v = a0.as_f64().filter(|v| v.is_finite() && *v >= 0.0).unwrap_or(0.0);
            st.settings.lock().unwrap().set("minDropFe", json!(v));
            tr.post(move |e| e.set_min_drop_fe(v));
            json!(v)
        }
        "settings:setRunScope" => {
            let pid = a0["playerId"].as_str().map(str::to_string);
            let all = a0["allSeasons"].as_bool() == Some(true);
            {
                let mut s = st.settings.lock().unwrap();
                s.set("playerFilter", json!(pid));
                s.set("allSeasons", json!(all));
            }
            tr.post(move |e| e.set_run_scope(pid, all));
            Value::Null
        }
        "settings:uiLanguage" => json!(st.settings.lock().unwrap().str("uiLanguage").unwrap_or("ru")),
        "settings:setUiLanguage" => {
            let Some(lang) = a0.as_str().filter(|l| matches!(*l, "ru" | "en")).map(str::to_string) else {
                return Ok(json!(st.settings.lock().unwrap().str("uiLanguage").unwrap_or("ru")));
            };
            st.settings.lock().unwrap().set("uiLanguage", json!(lang));
            let l = ui_lang(Some(&lang));
            tr.post(move |e| e.set_ui_language(l));
            let _ = app.emit("ui:language", &lang);
            json!(lang)
        }
        "settings:itemNames" => json!(st.settings.lock().unwrap().str("itemNames").unwrap_or("ru")),
        "settings:setItemNames" => {
            let lang = a0.as_str().unwrap_or("ru").to_string();
            st.settings.lock().unwrap().set("itemNames", json!(lang));
            let l = name_language(Some(&lang));
            tr.post(move |e| e.set_language(l));
            json!(lang)
        }
        "settings:communityPrices" => json!(st.settings.lock().unwrap().get()["communityPrices"].as_bool().unwrap_or(true)),
        "settings:setCommunityPrices" => {
            let on = a0.as_bool().unwrap_or(true);
            st.settings.lock().unwrap().set("communityPrices", json!(on));
            tr.post(move |e| e.set_community_prices(on));
            json!(on)
        }
        "settings:prisonHints" => json!(st.settings.lock().unwrap().bool("prisonHints")),
        "settings:setPrisonHints" => {
            let on = a0.as_bool().unwrap_or(false);
            st.settings.lock().unwrap().set("prisonHints", json!(on));
            let _ = st.tx.lock().unwrap().send(Event::SetEnabled(on));
            json!(on)
        }
        "settings:prisonSaveShots" => json!(st.settings.lock().unwrap().bool("prisonSaveShots")),
        "settings:setPrisonSaveShots" => {
            let on = a0.as_bool().unwrap_or(false);
            st.settings.lock().unwrap().set("prisonSaveShots", json!(on));
            let _ = st.tx.lock().unwrap().send(Event::SetSaveShots(on));
            json!(on)
        }
        "settings:openPrisonShots" => {
            let _ = std::fs::create_dir_all(&st.shots_dir);
            let _ = std::process::Command::new("explorer").arg(&st.shots_dir).spawn();
            json!("")
        }
        "app:settings" => {
            let resolved = tr.call(|e| e.log_directory().map(|p| p.to_string_lossy().to_string())).flatten();
            let s = st.settings.lock().unwrap();
            json!({
                "logDir": s.get()["logDir"],
                "resolvedLogDir": resolved,
                "closeToTray": s.bool("closeToTray"),
                "autoStart": s.bool("autoStart"),
                "version": env!("CARGO_PKG_VERSION"),
                "busyHotkeys": [],
            })
        }
        "app:setCloseToTray" => {
            let on = a0.as_bool().unwrap_or(false);
            st.settings.lock().unwrap().set("closeToTray", json!(on));
            json!(on)
        }
        "app:setAutoStart" => {
            let on = a0.as_bool().unwrap_or(false);
            st.settings.lock().unwrap().set("autoStart", json!(on));
            json!(on)
        }
        "app:chooseLogDir" => {
            let picked = app.dialog().file().set_title("Папка с логами Torchlight: Infinite").blocking_pick_folder();
            let Some(p) = picked.and_then(|p| p.into_path().ok()) else { return Ok(json!({ "status": "cancelled" })) };
            let Some(dir) = gorn_core::locate::resolve_log_directory(&p) else { return Ok(json!({ "status": "invalid" })) };
            let d = dir.to_string_lossy().to_string();
            st.settings.lock().unwrap().set("logDir", json!(d));
            apply_log_dir(&tr, Some(dir), Some(d))
        }
        "app:resetLogDir" => {
            st.settings.lock().unwrap().set("logDir", Value::Null);
            apply_log_dir(&tr, None, None)
        }
        "app:relaunch" => {
            *st.quitting.lock().unwrap() = true;
            tr.shutdown();
            app.restart();
        }
        "prison:ready" => {
            let _ = st.tx.lock().unwrap().send(Event::OverlayReady);
            Value::Null
        }
        // Оверлей, плашки и пульт — следующий шаг этапа 5; пока окна нет.
        "overlay:state" => {
            let mut o = st.settings.lock().unwrap().overlay().clone();
            o["enabled"] = json!(false);
            o
        }
        "overlay:toggle" => json!(false),
        // Обновления — через плагин updater на следующем шаге.
        "update:get" => Value::Null,
        _ => Value::Null,
    };
    Ok(v)
}

/// Новая папка логов: пока игра не найдена — сразу в работу, иначе — после перезапуска.
fn apply_log_dir(tr: &Tracker, dir: Option<PathBuf>, shown: Option<String>) -> Value {
    let booted = tr.call(|e| e.is_booted()).unwrap_or(true);
    tr.set_log_dir(dir);
    let status = if booted {
        "needsRestart"
    } else {
        tr.boot();
        "applied"
    };
    match shown {
        Some(d) => json!({ "status": status, "dir": d }),
        None => json!({ "status": status }),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--test-capture") {
        test_capture(args.get(2).map(String::as_str).unwrap_or("capture.png"));
        return;
    }
    // `--smoke <файл>`: проверка без окна поверх игры — главное окно скрыто,
    // подсказки в тюрьме выключены на этот запуск, каждый срез пишется в файл.
    let smoke = args.iter().position(|a| a == "--smoke").and_then(|i| args.get(i + 1)).map(PathBuf::from);
    let smoke_setup = smoke.clone();
    let manual = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyP);
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
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
        // Иконки предметов: с диска, при первом показе — с CDN, в своём потоке.
        .register_asynchronous_uri_scheme_protocol("gorn-icon", |_ctx, req, responder| {
            let path = req.uri().path().to_string();
            std::thread::spawn(move || {
                let res = match icons::id_from_path(&path).and_then(icons::get) {
                    Some(data) => tauri::http::Response::builder()
                        .header("Content-Type", "image/webp")
                        .header("Cache-Control", "public, max-age=31536000, immutable")
                        .header("Access-Control-Allow-Origin", "*")
                        .body(data),
                    None => tauri::http::Response::builder().status(404).body(vec![]),
                };
                responder.respond(res.unwrap());
            });
        })
        .invoke_handler(tauri::generate_handler![gorn])
        .setup(move |app| {
            let data = app.path().app_data_dir()?;
            let pictures = app.path().picture_dir()?;
            let roaming = data.parent().map(|p| p.to_path_buf()).unwrap_or_default();
            let old = roaming.join("gorn-tli");
            // Первый запуск рядом с Горном на Electron: история переезжает копией.
            let moved = settings::import_from_electron(&data, &old);
            if !moved.is_empty() {
                eprintln!("[горн] перенесено из Горна на Electron: {}", moved.join(", "));
            }
            let settings = Settings::load(&data, system_locale().as_deref());

            let items = CatalogueItem::parse_list(include_str!("../../crates/gorn-core/data/items.json"))?;
            let maps = serde_json::from_str(include_str!("../../crates/gorn-core/data/maps.json")).ok();
            icons::init(&data, old.exists().then_some(old.clone()), &items);
            let s = settings.get();
            let tracker = Tracker::start(
                app.handle().clone(),
                tracker::Config {
                    data_dir: data.clone(),
                    log_dir: s["logDir"].as_str().map(PathBuf::from),
                    options: EngineOptions {
                        community_prices: s["communityPrices"].as_bool().unwrap_or(true),
                        player_filter: s["playerFilter"].as_str().map(str::to_string),
                        all_seasons: s["allSeasons"].as_bool().unwrap_or(false),
                        excluded_items: settings.excluded(),
                        min_drop_fe: s["minDropFe"].as_f64().unwrap_or(0.0),
                        maps,
                        language: name_language(s["itemNames"].as_str()),
                        ui: ui_lang(s["uiLanguage"].as_str()),
                        icon_base: Some(icons::ICON_BASE.into()),
                        ..Default::default()
                    },
                    items,
                },
            );

            // Подсказки в тюрьме: строки лога — от трекера (одно дочитывание на всё).
            let paths = Paths {
                state: data.join("prison-state.json"),
                display: data.join("prison-display.json"),
                shots: pictures.join("Горн Next — тюрьма"),
            };
            let shots_dir = paths.shots.clone();
            let (tx, rx) = std::sync::mpsc::channel::<Event>();
            let mut assist = Assist::new(app.handle().clone(), tx.clone(), paths, settings.bool("prisonSaveShots"));
            if !settings.bool("prisonHints") || smoke_setup.is_some() {
                let _ = tx.send(Event::SetEnabled(false));
            }
            let log_found = gorn_core::locate::find_log_directory().is_some() || s["logDir"].is_string();
            assist.log_found = log_found;
            let (ltx, lrx) = std::sync::mpsc::channel::<Vec<String>>();
            tracker.forward_lines(ltx);
            let tx_log = tx.clone();
            std::thread::spawn(move || {
                for lines in lrx {
                    if tx_log.send(Event::Lines(lines)).is_err() {
                        break;
                    }
                }
            });
            std::thread::Builder::new().name("горн-тюрьма".into()).spawn(move || assist.run(rx))?;
            let tx_ready = tx.clone();
            app.listen("prison:ready", move |_| {
                let _ = tx_ready.send(Event::OverlayReady);
            });

            // Трей: окно трекера и подсказки в тюрьме.
            let show = MenuItem::with_id(app, "show", "Показать Горн", true, None::<&str>)?;
            let hints = CheckMenuItem::with_id(app, "hints", "Подсказки в тюрьме", true, settings.bool("prisonHints"), None::<&str>)?;
            let save = CheckMenuItem::with_id(app, "save", "Сохранять кадры тюрьмы (для разбора)", true, settings.bool("prisonSaveShots"), None::<&str>)?;
            let open = MenuItem::with_id(app, "open", "Открыть папку кадров", true, None::<&str>)?;
            let shot = MenuItem::with_id(app, "shot", "Подсказка по экрану сейчас (Ctrl+Alt+P)", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Выход", true, None::<&str>)?;
            let sep = PredefinedMenuItem::separator(app)?;
            let sep2 = PredefinedMenuItem::separator(app)?;
            let menu = Menu::with_items(app, &[&show, &sep, &hints, &save, &shot, &open, &sep2, &quit])?;
            let tip = if log_found { "Горн Next" } else { "Горн Next — лог игры не найден" };
            TrayIconBuilder::with_id("tray")
                .icon(app.default_window_icon().cloned().expect("иконка"))
                .tooltip(tip)
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_tray_icon_event(|tray, ev| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = ev {
                        show_main(tray.app_handle());
                    }
                })
                .on_menu_event(move |app, ev| {
                    let st = app.state::<AppState>();
                    match ev.id().as_ref() {
                        "show" => show_main(app),
                        "hints" => {
                            let mut s = st.settings.lock().unwrap();
                            let on = !s.bool("prisonHints");
                            s.set("prisonHints", json!(on));
                            let _ = hints.set_checked(on);
                            let _ = st.tx.lock().unwrap().send(Event::SetEnabled(on));
                        }
                        "save" => {
                            let mut s = st.settings.lock().unwrap();
                            let on = !s.bool("prisonSaveShots");
                            s.set("prisonSaveShots", json!(on));
                            let _ = save.set_checked(on);
                            let _ = st.tx.lock().unwrap().send(Event::SetSaveShots(on));
                        }
                        "open" => {
                            let _ = std::fs::create_dir_all(&st.shots_dir);
                            let _ = std::process::Command::new("explorer").arg(&st.shots_dir).spawn();
                        }
                        "shot" => {
                            let _ = st.tx.lock().unwrap().send(Event::Manual);
                        }
                        "quit" => {
                            *st.quitting.lock().unwrap() = true;
                            st.tracker.shutdown();
                            app.exit(0);
                        }
                        _ => {}
                    }
                })
                .build(app)?;

            if let Err(e) = app.global_shortcut().register(manual) {
                eprintln!("[горн] Ctrl+Alt+P занят (Горн на Electron?): {e}");
            }
            app.manage(AppState { tx: Mutex::new(tx), settings: Mutex::new(settings), tracker, shots_dir, quitting: Mutex::new(false) });
            if let Some(f) = &smoke_setup {
                let f = f.clone();
                app.listen("snapshot", move |ev| {
                    let _ = std::fs::write(&f, ev.payload());
                });
            }
            create_main_window(app.handle(), smoke_setup.is_none())?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("Горн: запуск")
        .run(|app, ev| {
            match ev {
                // Окно стрелок закрывается само — приложение живёт, пока не выход.
                tauri::RunEvent::ExitRequested { api, code: None, .. } => {
                    let quitting = app.try_state::<AppState>().is_some_and(|st| *st.quitting.lock().unwrap());
                    if !quitting {
                        api.prevent_exit();
                    }
                }
                tauri::RunEvent::Exit => {
                    if let Some(st) = app.try_state::<AppState>() {
                        st.tracker.shutdown();
                    }
                }
                _ => {}
            }
        });
}
