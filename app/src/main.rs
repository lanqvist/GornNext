//! Горн на Tauri 2: трекер валюты (движок `gorn-core`, интерфейс React из
//! `ui/`) и подсказки в сезонной тюрьме (советник `gorn-prison`, стрелки
//! поверх игры). Мост `window.gorn` — одна команда `gorn` с каналами, как у
//! ipcMain Горна на Electron, и события Tauri с теми же именами.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod assist;
mod facts;
mod capture;
mod icons;
mod layers;
mod overlay;
mod settings;
mod shots;
mod tracker;
mod updater;
mod triggers;

use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use gorn_core::engine::{CatalogueItem, EngineOptions};
use serde_json::{json, Value};
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Listener, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

use assist::{Assist, Event, Paths};
use layers::Layers;
use settings::Settings;
use tracker::{name_language, ui_lang, Tracker};

struct AppState {
    tx: Mutex<Sender<Event>>,
    settings: Arc<Mutex<Settings>>,
    layers: Arc<Layers>,
    /// Сочетания, которые занять не удалось, — показываются в настройках.
    busy_hotkeys: Vec<String>,
    tracker: Tracker,
    shots_dir: PathBuf,
    updater: updater::Updater,
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
    let img = s
        .grab(
            std::time::Duration::ZERO,
            std::time::Duration::from_millis(2000),
            std::time::Duration::from_secs(60),
        )
        .expect("кадр");
    let ms = t0.elapsed().as_millis();
    let f = gorn_prison::recognize::Frame {
        width: img.width,
        height: img.height,
        data: &img.data,
    };
    let t1 = std::time::Instant::now();
    let (view, geo) = gorn_prison::recognize::read(&f, None, Default::default());
    let rms = t1.elapsed().as_millis();
    let file = std::fs::File::create(out).expect("файл");
    let mut enc = png::Encoder::new(
        std::io::BufWriter::new(file),
        img.width as u32,
        img.height as u32,
    );
    enc.set_color(png::ColorType::Rgba);
    enc.write_header()
        .unwrap()
        .write_image_data(&img.data)
        .unwrap();
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

/// Журнал в `%APPDATA%\com.gorn.next\gorn.log`: у релизной сборки нет консоли,
/// и без файла сбой окна или паника остаются невидимыми. stderr перенаправлен
/// туда же — `eprintln!` модулей и сообщение паники попадают в журнал.
fn init_log(data_dir: &std::path::Path) {
    let path = data_dir.join("gorn.log");
    // Прошлый запуск — рядом, один файл: журнал не растёт бесконечно.
    let _ = std::fs::rename(&path, data_dir.join("gorn.log.old"));
    let Ok(file) = std::fs::File::create(&path) else {
        return;
    };
    if cfg!(not(debug_assertions)) {
        use std::os::windows::io::IntoRawHandle;
        let h = file.into_raw_handle();
        unsafe {
            let _ = windows::Win32::System::Console::SetStdHandle(
                windows::Win32::System::Console::STD_ERROR_HANDLE,
                windows::Win32::Foundation::HANDLE(h),
            );
        }
    }
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log(&format!("[паника] {info}"));
        prev(info);
    }));
}

/// Пауза в асинхронной задаче, не занимая поток пула.
async fn tokio_sleep(secs: u64) {
    let (tx, rx) = tauri::async_runtime::channel::<()>(1);
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(secs));
        let _ = tx.blocking_send(());
    });
    let mut rx = rx;
    let _ = rx.recv().await;
}

/// Строка в журнал (и в консоль отладочной сборки).
pub fn log(msg: &str) {
    eprintln!("{} {msg}", chrono::Local::now().format("%H:%M:%S%.3f"));
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
        // Windows не отдаёт фокус фоновому процессу: поверх всех на миг — и обратно.
        let _ = w.set_always_on_top(true);
        let _ = w.set_always_on_top(false);
        let _ = w.set_focus();
    }
}

fn create_main_window(
    app: &AppHandle,
    visible: bool,
    webview_dir: Option<PathBuf>,
) -> tauri::Result<()> {
    let mut b = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()));
    // Проверка `--smoke` — со своей папкой WebView2: общая папка с запущенным
    // Горном держалась процессами проверки, и его окна не создавались.
    if let Some(d) = webview_dir {
        b = b.data_directory(d);
    }
    let w = b
        .title("Горн — трекер Torchlight: Infinite")
        .min_inner_size(940.0, 620.0)
        .background_color(tauri::window::Color(0x0e, 0x0f, 0x13, 0xff))
        // Показ — после того, как окно встанет по экрану.
        .visible(false)
        .additional_browser_args(layers::BROWSER_ARGS)
        // Внешние ссылки — в браузере, а не внутри приложения; страницы
        // приложения (*.localhost) и служебные адреса пропускаются.
        .on_navigation(|url| {
            let external = matches!(url.scheme(), "http" | "https")
                && !url.host_str().is_some_and(|h| h.ends_with(".localhost"));
            if external {
                log(&format!("[горн] внешняя ссылка — в браузер: {url}"));
                let _ = std::process::Command::new("explorer")
                    .arg(url.as_str())
                    .spawn();
            }
            !external
        })
        .build()?;
    layers::fit_main(&w);
    if visible {
        let _ = w.show();
        let _ = w.set_focus();
    }
    let app2 = app.clone();
    w.on_window_event(move |ev| {
        // Окно показали или развернули — догнать пропущенные срезы.
        if let tauri::WindowEvent::Focused(true) | tauri::WindowEvent::Resized(_) = ev {
            app2.state::<AppState>().layers.main_shown();
        }
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
                st.layers.shutdown();
                st.tracker.shutdown();
                app2.exit(0);
            }
        }
    });
    Ok(())
}

fn arg(args: &[Value], i: usize) -> &Value {
    args.get(i).unwrap_or(&Value::Null)
}

/// Мост `window.gorn`: канал и аргументы как у ipcMain Горна на Electron.
#[tauri::command]
async fn gorn(
    app: AppHandle,
    window: tauri::Window,
    channel: String,
    args: Vec<Value>,
) -> Result<Value, String> {
    let st = app.state::<AppState>();
    let tr = st.tracker.clone();
    let a0 = arg(&args, 0).clone();
    let a1 = arg(&args, 1).clone();
    let v = match channel.as_str() {
        // Ошибки страниц — в журнал: у релизной сборки нет инструментов разработчика.
        "log" => {
            log(&format!(
                "[окно {}] {}",
                window.label(),
                a0.as_str().unwrap_or("")
            ));
            Value::Null
        }
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
            tr.call(move |e| serde_json::to_value(e.query_runs(&q)).unwrap_or(Value::Null))
                .unwrap_or(Value::Null)
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
            tr.call(move |e| serde_json::to_value(e.query_prices(&q)).unwrap_or(Value::Null))
                .unwrap_or(json!([]))
        }
        "items:setExcluded" => {
            let Some(id) = a0.as_i64() else {
                return Ok(json!(st.settings.lock().unwrap().excluded()));
            };
            let on = a1.as_bool().unwrap_or(false);
            let list = st.settings.lock().unwrap().set_excluded(id, on);
            tr.post(move |e| e.set_excluded(id, on));
            json!(list)
        }
        "settings:setMinDropFe" => {
            let v = a0
                .as_f64()
                .filter(|v| v.is_finite() && *v >= 0.0)
                .unwrap_or(0.0);
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
        "settings:uiLanguage" => json!(st
            .settings
            .lock()
            .unwrap()
            .str("uiLanguage")
            .unwrap_or("ru")),
        "settings:setUiLanguage" => {
            let Some(lang) = a0
                .as_str()
                .filter(|l| matches!(*l, "ru" | "en"))
                .map(str::to_string)
            else {
                return Ok(json!(st
                    .settings
                    .lock()
                    .unwrap()
                    .str("uiLanguage")
                    .unwrap_or("ru")));
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
        "settings:communityPrices" => json!(st.settings.lock().unwrap().get()["communityPrices"]
            .as_bool()
            .unwrap_or(true)),
        "settings:setCommunityPrices" => {
            let on = a0.as_bool().unwrap_or(true);
            st.settings
                .lock()
                .unwrap()
                .set("communityPrices", json!(on));
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
            st.settings
                .lock()
                .unwrap()
                .set("prisonSaveShots", json!(on));
            let _ = st.tx.lock().unwrap().send(Event::SetSaveShots(on));
            json!(on)
        }
        "settings:openPrisonShots" => {
            let _ = std::fs::create_dir_all(&st.shots_dir);
            let _ = std::process::Command::new("explorer")
                .arg(&st.shots_dir)
                .spawn();
            json!("")
        }
        "app:settings" => {
            let resolved = tr
                .call(|e| e.log_directory().map(|p| p.to_string_lossy().to_string()))
                .flatten();
            let s = st.settings.lock().unwrap();
            json!({
                "logDir": s.get()["logDir"],
                "resolvedLogDir": resolved,
                "closeToTray": s.bool("closeToTray"),
                "autoStart": s.bool("autoStart"),
                "version": env!("CARGO_PKG_VERSION"),
                "busyHotkeys": st.busy_hotkeys,
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
            sync_autostart(&app, on);
            json!(on)
        }
        "app:chooseLogDir" => {
            let picked = app
                .dialog()
                .file()
                .set_title("Папка с логами Torchlight: Infinite")
                .blocking_pick_folder();
            let Some(p) = picked.and_then(|p| p.into_path().ok()) else {
                return Ok(json!({ "status": "cancelled" }));
            };
            let Some(dir) = gorn_core::locate::resolve_log_directory(&p) else {
                return Ok(json!({ "status": "invalid" }));
            };
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
        "overlay:state" => st.settings.lock().unwrap().overlay().clone(),
        "overlay:toggle" => json!(st.layers.toggle_overlay()),
        "overlay:close" => {
            st.layers.close_overlay(true);
            Value::Null
        }
        "overlay:setMode" => {
            st.layers.set_mode(a0.as_str().unwrap_or(""));
            Value::Null
        }
        "overlay:cycleMode" => {
            st.layers.cycle_mode();
            Value::Null
        }
        "overlay:toggleLock" => {
            st.layers.toggle_locked();
            Value::Null
        }
        "overlay:setOpacity" => {
            st.layers.set_opacity(a0.as_f64().unwrap_or(0.9));
            Value::Null
        }
        "overlay:height" => {
            st.layers
                .set_overlay_content(a0.as_f64().unwrap_or(0.0), a1.as_f64());
            Value::Null
        }
        "overlay:setPrefs" => {
            st.layers.patch_prefs(&a0);
            Value::Null
        }
        "toasts:height" => {
            st.layers.toasts_height(a0.as_f64().unwrap_or(0.0));
            Value::Null
        }
        "toasts:active" => {
            st.layers.toasts_active(a0.as_bool().unwrap_or(false));
            Value::Null
        }
        "panel:ready" => {
            st.layers.panel_ready();
            Value::Null
        }
        "panel:close" => {
            st.layers.hide_panel();
            Value::Null
        }
        "panel:tab" => {
            st.layers.panel_tab(a0.as_str().unwrap_or(""));
            Value::Null
        }
        "update:get" => st.updater.state(),
        "update:check" => {
            let en = st.settings.lock().unwrap().str("uiLanguage") == Some("en");
            st.updater.check(en).await;
            Value::Null
        }
        "update:download" => {
            let en = st.settings.lock().unwrap().str("uiLanguage") == Some("en");
            st.updater.download(en).await;
            Value::Null
        }
        "update:install" => {
            // Дописать закрытое в базу до того, как установщик закроет приложение.
            *st.quitting.lock().unwrap() = true;
            st.layers.shutdown();
            st.tracker.shutdown();
            st.updater.install();
            Value::Null
        }
        _ => Value::Null,
    };
    Ok(v)
}

/// Запуск вместе с Windows. В отладочной сборке не регистрируется: в автозагрузку
/// попал бы exe из target/debug.
fn sync_autostart(app: &AppHandle, on: bool) {
    if cfg!(debug_assertions) {
        return;
    }
    use tauri_plugin_autostart::ManagerExt;
    let al = app.autolaunch();
    let _ = if on { al.enable() } else { al.disable() };
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
    // `--smoke <файл> [секунд]`: проверка без окон и значка в трее — главное окно
    // скрыто, подсказки в тюрьме выключены, каждый срез пишется в файл, данные и
    // папка WebView2 свои; через заданное время (20 с) приложение закрывается само.
    let smoke_at = args.iter().position(|a| a == "--smoke");
    let smoke = smoke_at.and_then(|i| args.get(i + 1)).map(PathBuf::from);
    let smoke_secs: u64 = smoke_at
        .and_then(|i| args.get(i + 2))
        .and_then(|s| s.parse().ok())
        .unwrap_or(20);
    let smoke_setup = smoke.clone();
    // Горячие клавиши — как у Горна на Electron. Сочетания редкие: пересечься
    // с игровыми биндами почти нечем.
    let key = |c: Code| Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), c);
    let bindings: Vec<(Shortcut, &'static str)> = vec![
        (key(Code::KeyG), "Ctrl+Alt+G"),
        (key(Code::KeyL), "Ctrl+Alt+L"),
        (key(Code::KeyM), "Ctrl+Alt+M"),
        // Подсказка в тюрьме вручную: если автоматический снимок не сработал.
        (key(Code::KeyP), "Ctrl+Alt+P"),
        // Пульт поверх игры; F — сразу к поиску цены.
        (key(Code::KeyT), "Ctrl+Alt+T"),
        (key(Code::KeyF), "Ctrl+Alt+F"),
    ];
    let handled = bindings.clone();
    let mut builder = tauri::Builder::default();
    // Второй запуск поднимает окно первого: два экземпляра делили бы одну базу,
    // хоткеи и оверлей. Проверка `--smoke` живёт отдельно (свои данные) и
    // работает рядом с запущенным Горном.
    if smoke.is_none() {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            log("[горн] второй запуск — показываю окно");
            show_main(app);
        }));
    }
    builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(move |app, sc, ev| {
                    if ev.state() != ShortcutState::Pressed {
                        return;
                    }
                    let Some(st) = app.try_state::<AppState>() else {
                        return;
                    };
                    match handled.iter().find(|(k, _)| k == sc).map(|(_, n)| *n) {
                        Some("Ctrl+Alt+G") => {
                            st.layers.toggle_overlay();
                        }
                        Some("Ctrl+Alt+L") => st.layers.toggle_locked(),
                        Some("Ctrl+Alt+M") => st.layers.cycle_mode(),
                        Some("Ctrl+Alt+P") => {
                            let _ = st.tx.lock().unwrap().send(Event::Manual);
                        }
                        Some("Ctrl+Alt+T") => st.layers.toggle_panel(None),
                        Some("Ctrl+Alt+F") => st.layers.toggle_panel(Some("prices")),
                        _ => {}
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
            let base = app.path().app_data_dir()?;
            let data = if smoke_setup.is_some() {
                base.join("smoke")
            } else {
                base.clone()
            };
            let _ = std::fs::create_dir_all(&data);
            init_log(&data);
            log(&format!("[горн] запуск {}", env!("CARGO_PKG_VERSION")));
            let pictures = app.path().picture_dir()?;
            let roaming = base.parent().map(|p| p.to_path_buf()).unwrap_or_default();
            let old = roaming.join("gorn-tli");
            // Первый запуск рядом с Горном на Electron: история переезжает копией.
            let moved = settings::import_from_electron(&data, &old);
            if !moved.is_empty() {
                eprintln!(
                    "[горн] перенесено из Горна на Electron: {}",
                    moved.join(", ")
                );
            }
            let settings = Arc::new(Mutex::new(Settings::load(
                &data,
                system_locale().as_deref(),
            )));
            let layers = Layers::new(app.handle().clone(), settings.clone());
            let sink_layers = layers.clone();
            let smoke_file = smoke_setup.clone();
            let sink: tracker::Sink = Arc::new(move |snap: &Value| {
                sink_layers.publish(snap);
                if let Some(f) = &smoke_file {
                    let _ = std::fs::write(f, snap.to_string());
                }
            });
            let settings_now = settings.lock().unwrap();

            let items =
                CatalogueItem::parse_list(include_str!("../../crates/gorn-core/data/items.json"))?;
            let maps =
                serde_json::from_str(include_str!("../../crates/gorn-core/data/maps.json")).ok();
            icons::init(&data, old.exists().then_some(old.clone()), &items);
            let s = settings_now.get().clone();
            let tracker = Tracker::start(
                app.handle().clone(),
                tracker::Config {
                    data_dir: data.clone(),
                    log_dir: s["logDir"].as_str().map(PathBuf::from),
                    options: EngineOptions {
                        community_prices: s["communityPrices"].as_bool().unwrap_or(true),
                        player_filter: s["playerFilter"].as_str().map(str::to_string),
                        all_seasons: s["allSeasons"].as_bool().unwrap_or(false),
                        excluded_items: settings_now.excluded(),
                        min_drop_fe: s["minDropFe"].as_f64().unwrap_or(0.0),
                        maps,
                        language: name_language(s["itemNames"].as_str()),
                        ui: ui_lang(s["uiLanguage"].as_str()),
                        icon_base: Some(icons::ICON_BASE.into()),
                        ..Default::default()
                    },
                    items,
                    sink,
                },
            );

            // Подсказки в тюрьме: строки лога — от трекера (одно дочитывание на всё).
            let paths = Paths {
                state: data.join("prison-state.json"),
                facts: data.join("prison-facts.db"),
                display: data.join("prison-display.json"),
                shots: pictures.join("Горн Next — тюрьма"),
            };
            let shots_dir = paths.shots.clone();
            let (tx, rx) = std::sync::mpsc::channel::<Event>();
            let mut assist = Assist::new(
                app.handle().clone(),
                tx.clone(),
                paths,
                settings_now.bool("prisonSaveShots"),
            );
            if !settings_now.bool("prisonHints") || smoke_setup.is_some() {
                let _ = tx.send(Event::SetEnabled(false));
            }
            let log_found =
                gorn_core::locate::find_log_directory().is_some() || s["logDir"].is_string();
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
            std::thread::Builder::new()
                .name("горн-тюрьма".into())
                .spawn(move || assist.run(rx))?;
            let tx_ready = tx.clone();
            app.listen("prison:ready", move |_| {
                let _ = tx_ready.send(Event::OverlayReady);
            });

            // Трей: окно трекера и подсказки в тюрьме.
            let show = MenuItem::with_id(app, "show", "Показать Горн", true, None::<&str>)?;
            let hints = CheckMenuItem::with_id(
                app,
                "hints",
                "Подсказки в тюрьме",
                true,
                settings_now.bool("prisonHints"),
                None::<&str>,
            )?;
            let save = CheckMenuItem::with_id(
                app,
                "save",
                "Сохранять кадры тюрьмы (для разбора)",
                true,
                settings_now.bool("prisonSaveShots"),
                None::<&str>,
            )?;
            let overlay_item =
                MenuItem::with_id(app, "overlay", "Оверлей (Ctrl+Alt+G)", true, None::<&str>)?;
            let open = MenuItem::with_id(app, "open", "Открыть папку кадров", true, None::<&str>)?;
            let shot = MenuItem::with_id(
                app,
                "shot",
                "Подсказка по экрану сейчас (Ctrl+Alt+P)",
                true,
                None::<&str>,
            )?;
            let quit = MenuItem::with_id(app, "quit", "Выход", true, None::<&str>)?;
            let sep = PredefinedMenuItem::separator(app)?;
            let sep2 = PredefinedMenuItem::separator(app)?;
            let menu = Menu::with_items(
                app,
                &[
                    &show,
                    &overlay_item,
                    &sep,
                    &hints,
                    &save,
                    &shot,
                    &open,
                    &sep2,
                    &quit,
                ],
            )?;
            let tip = if log_found {
                "Горн Next"
            } else {
                "Горн Next — лог игры не найден"
            };
            if smoke_setup.is_none() {
                TrayIconBuilder::with_id("tray")
                    .icon(app.default_window_icon().cloned().expect("иконка"))
                    .tooltip(tip)
                    .menu(&menu)
                    .show_menu_on_left_click(false)
                    .on_tray_icon_event(|tray, ev| {
                        if let TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        } = ev
                        {
                            show_main(tray.app_handle());
                        }
                    })
                    .on_menu_event(move |app, ev| {
                        let st = app.state::<AppState>();
                        match ev.id().as_ref() {
                            "show" => show_main(app),
                            "overlay" => {
                                st.layers.toggle_overlay();
                            }
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
                                let _ = std::process::Command::new("explorer")
                                    .arg(&st.shots_dir)
                                    .spawn();
                            }
                            "shot" => {
                                let _ = st.tx.lock().unwrap().send(Event::Manual);
                            }
                            "quit" => {
                                *st.quitting.lock().unwrap() = true;
                                st.layers.shutdown();
                                st.tracker.shutdown();
                                app.exit(0);
                            }
                            _ => {}
                        }
                    })
                    .build(app)?;
            }

            let mut busy_hotkeys = vec![];
            for (k, name) in bindings.iter().filter(|_| smoke_setup.is_none()) {
                if let Err(e) = app.global_shortcut().register(*k) {
                    eprintln!("[горн] {name} занято другой программой (Горн на Electron?): {e}");
                    busy_hotkeys.push(name.to_string());
                }
            }
            let overlay_on = settings_now.overlay()["enabled"].as_bool().unwrap_or(false);
            // Запись могли удалить руками, а установка — переехать: держим как в настройке.
            sync_autostart(app.handle(), settings_now.bool("autoStart"));
            drop(settings_now);
            let updates = updater::Updater::new(app.handle().clone());
            // Проверка обновлений — после разбора логов (за диск и сеть с ним не
            // соревнуемся), дальше раз в 4 часа: трекер живёт сутками.
            if smoke_setup.is_none() {
                let u = updates.clone();
                let en = settings.lock().unwrap().str("uiLanguage") == Some("en");
                tauri::async_runtime::spawn(async move {
                    tokio_sleep(8).await;
                    loop {
                        u.check(en).await;
                        tokio_sleep(4 * 3600).await;
                    }
                });
            }
            app.manage(AppState {
                updater: updates,
                tx: Mutex::new(tx),
                settings,
                layers: layers.clone(),
                busy_hotkeys,
                tracker,
                shots_dir,
                quitting: Mutex::new(false),
            });
            if smoke_setup.is_none() {
                if overlay_on {
                    layers.open_overlay();
                }
                // Слой всплывашек независим от оверлея-панели.
                layers.sync_toasts();
                // Пульт заранее, спрятанным: первое нажатие открывает его сразу и с фокусом.
                layers.warm_panel();
            }
            create_main_window(
                app.handle(),
                smoke_setup.is_none(),
                smoke_setup.as_ref().map(|_| data.join("webview")),
            )?;
            if smoke_setup.is_some() {
                let h = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(smoke_secs));
                    log("[горн] проверка окончена — выход");
                    let st = h.state::<AppState>();
                    *st.quitting.lock().unwrap() = true;
                    st.tracker.shutdown();
                    h.exit(0);
                });
            }
            log("[горн] главное окно создано");
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("Горн: запуск")
        .run(|app, ev| {
            match ev {
                // Окно стрелок закрывается само — приложение живёт, пока не выход.
                tauri::RunEvent::ExitRequested {
                    api, code: None, ..
                } => {
                    let quitting = app
                        .try_state::<AppState>()
                        .is_some_and(|st| *st.quitting.lock().unwrap());
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
