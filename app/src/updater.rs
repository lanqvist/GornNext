//! Обновления из релизов GornNext на GitHub — перенос `main/updater.ts` на
//! плагин updater Tauri: подписанный `latest.json`, установщик NSIS.
//! Состояние для интерфейса — тот же `UpdateState`, что у Горна на Electron.
//! Сам ничего не качает: «есть версия» → игрок жмёт «Скачать» → «Перезапустить».

use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::{Update, UpdaterExt};

#[derive(Default)]
struct Inner {
    status: &'static str,
    version: Option<String>,
    progress: f64,
    error: Option<String>,
    update: Option<Update>,
    bytes: Option<Vec<u8>>,
}

#[derive(Clone)]
pub struct Updater {
    app: AppHandle,
    inner: Arc<Mutex<Inner>>,
}

/// Обновления — только у установленной версии: у отладочной сборки и у exe
/// из target/ ставить поверх нечего.
fn supported() -> bool {
    !cfg!(debug_assertions) && std::env::current_exe().is_ok_and(|p| !p.to_string_lossy().contains("\\target\\"))
}

/// Причина ошибки одной строкой — как `shortReason` у эталона.
fn short_reason(err: &str, en: bool) -> String {
    let first = err.lines().next().unwrap_or_default();
    let tr = |ru: &str, e: &str| if en { e.to_string() } else { ru.to_string() };
    if ["dns", "connect", "timed out", "Connection", "resolve"].iter().any(|k| first.contains(k)) {
        return tr("нет связи с сервером обновлений", "cannot reach the update server");
    }
    if first.contains("404") {
        return tr("в последнем релизе нет файла обновления", "the latest release has no update file");
    }
    if first.contains("403") || first.to_lowercase().contains("rate limit") {
        return tr("сервер обновлений временно ограничил запросы", "the update server is rate-limiting requests");
    }
    if first.chars().count() > 120 {
        format!("{}…", first.chars().take(117).collect::<String>())
    } else {
        first.to_string()
    }
}

impl Updater {
    pub fn new(app: AppHandle) -> Self {
        Updater { app, inner: Arc::new(Mutex::new(Inner { status: "idle", ..Default::default() })) }
    }

    pub fn state(&self) -> Value {
        let i = self.inner.lock().unwrap();
        json!({ "supported": supported(), "status": i.status, "version": i.version, "progress": i.progress, "error": i.error })
    }

    fn set(&self, f: impl FnOnce(&mut Inner)) {
        f(&mut self.inner.lock().unwrap());
        let _ = self.app.emit_to("main", "update:state", self.state());
    }

    fn fail(&self, err: impl std::fmt::Display, en: bool) {
        let msg = short_reason(&err.to_string(), en);
        crate::log(&format!("[обновление] ошибка: {err}"));
        self.set(|i| {
            i.status = "error";
            i.error = Some(msg);
        });
    }

    pub async fn check(&self, en: bool) {
        if !supported() {
            return;
        }
        self.set(|i| i.status = "checking");
        let updater = match self.app.updater() {
            Ok(u) => u,
            Err(e) => return self.fail(e, en),
        };
        match updater.check().await {
            Ok(Some(u)) => {
                crate::log(&format!("[обновление] есть {}", u.version));
                let v = u.version.clone();
                self.set(|i| {
                    i.status = "available";
                    i.version = Some(v);
                    i.error = None;
                    i.update = Some(u);
                });
            }
            Ok(None) => self.set(|i| i.status = "idle"),
            Err(e) => self.fail(e, en),
        }
    }

    pub async fn download(&self, en: bool) {
        let Some(u) = self.inner.lock().unwrap().update.clone() else { return };
        self.set(|i| {
            i.status = "downloading";
            i.progress = 0.0;
        });
        let me = self.clone();
        let mut got = 0usize;
        let res = u
            .download(
                move |chunk, total| {
                    got += chunk;
                    if let Some(t) = total.filter(|t| *t > 0) {
                        let p = got as f64 / t as f64;
                        me.set(|i| i.progress = p);
                    }
                },
                || {},
            )
            .await;
        match res {
            Ok(bytes) => self.set(|i| {
                i.status = "ready";
                i.progress = 1.0;
                i.bytes = Some(bytes);
            }),
            Err(e) => self.fail(e, en),
        }
    }

    /// Установщик NSIS ставит новую версию поверх и запускает её; приложение выходит.
    pub fn install(&self) {
        let (u, bytes) = {
            let mut i = self.inner.lock().unwrap();
            if i.status != "ready" {
                return;
            }
            (i.update.clone(), i.bytes.take())
        };
        let (Some(u), Some(bytes)) = (u, bytes) else { return };
        crate::log(&format!("[обновление] установка {}", u.version));
        if let Err(e) = u.install(bytes) {
            self.fail(e, false);
        }
    }
}
