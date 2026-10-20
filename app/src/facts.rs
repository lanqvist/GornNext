//! Журнал фактов тюрьмы в приложении: запись (`gorn_core::facts`) — в своём
//! потоке, диск не задерживает совет.

use std::path::Path;
use std::sync::mpsc::{channel, Sender};

use gorn_core::facts::Journal;
use serde_json::Value;

/// Поток журнала: факты приходят каналом, пишутся по одному.
pub fn spawn(path: &Path) -> Option<Sender<Value>> {
    let journal = match Journal::open(path, env!("CARGO_PKG_VERSION")) {
        Ok(j) => j,
        Err(e) => {
            crate::log(&format!("[факты] база не открылась: {e}"));
            return None;
        }
    };
    let (tx, rx) = channel::<Value>();
    std::thread::Builder::new()
        .name("горн-факты".into())
        .spawn(move || {
            for f in rx {
                let ts = chrono::Utc::now().timestamp_millis();
                if let Err(e) = journal.add(&f, ts) {
                    crate::log(&format!("[факты] запись: {e}"));
                }
            }
        })
        .ok()?;
    Some(tx)
}
