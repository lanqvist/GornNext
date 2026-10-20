//! Лог игры: где он (Steam-библиотеки из libraryfolders.vdf, как
//! `core/locate.ts`) и дочитывание новых строк UE_game.log опросом.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::Duration;

/// Папка логов игры или None.
pub fn find_log_directory() -> Option<PathBuf> {
    let pf86 = std::env::var("ProgramFiles(x86)").unwrap_or_else(|_| r"C:\Program Files (x86)".into());
    let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".into());
    let mut libs: Vec<PathBuf> = vec![];
    for root in [Path::new(&pf86).join("Steam"), Path::new(&pf).join("Steam")] {
        if !root.exists() {
            continue;
        }
        libs.push(root.clone());
        if let Ok(text) = std::fs::read_to_string(root.join("steamapps").join("libraryfolders.vdf")) {
            for line in text.lines() {
                if let Some(rest) = line.trim().strip_prefix("\"path\"") {
                    let v = rest.trim();
                    if v.len() >= 2 && v.starts_with('"') && v.ends_with('"') {
                        libs.push(PathBuf::from(v[1..v.len() - 1].replace("\\\\", "\\")));
                    }
                }
            }
        }
    }
    libs.dedup();
    for lib in libs {
        for game in ["Torchlight Infinite", "Torchlight Infinite CN"] {
            let dir = lib.join("steamapps").join("common").join(game).join("UE_game").join("TorchLight").join("Saved").join("Logs");
            if dir.exists() {
                return Some(dir);
            }
        }
    }
    None
}

/// Дочитывать файл с конца; новые строки пачками — в tx. Файл стал короче
/// (игра начала лог заново) — читать с начала. Работает до закрытия канала.
pub fn tail(path: PathBuf, tx: Sender<Vec<String>>) {
    let mut pos: Option<u64> = None;
    let mut partial: Vec<u8> = vec![];
    let mut buf = vec![0u8; 1 << 16];
    loop {
        std::thread::sleep(Duration::from_millis(100));
        let Ok(mut f) = File::open(&path) else { continue };
        let Ok(len) = f.metadata().map(|m| m.len()) else { continue };
        let start = match pos {
            None => len, // при запуске — только новое
            Some(p) if len < p => {
                partial.clear();
                0
            }
            Some(p) => p,
        };
        if start == len {
            pos = Some(len);
            continue;
        }
        if f.seek(SeekFrom::Start(start)).is_err() {
            continue;
        }
        let mut read = 0u64;
        let mut lines = vec![];
        while start + read < len {
            let want = ((len - start - read) as usize).min(buf.len());
            let Ok(n) = f.read(&mut buf[..want]) else { break };
            if n == 0 {
                break;
            }
            read += n as u64;
            for &b in &buf[..n] {
                if b == b'\n' {
                    let s = String::from_utf8_lossy(&partial).trim_end_matches('\r').to_string();
                    partial.clear();
                    if !s.is_empty() {
                        lines.push(s);
                    }
                } else {
                    partial.push(b);
                }
            }
        }
        pos = Some(start + read);
        if !lines.is_empty() && tx.send(lines).is_err() {
            return;
        }
    }
}
