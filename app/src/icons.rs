//! Иконки предметов: локальный кэш на диске, при первом показе — загрузка
//! с CDN справочника (`main/icons.ts`). Интерфейс получает их по своей схеме
//! `gorn-icon` (в WebView2 на Windows — `http://gorn-icon.localhost/item/<id>.webp`),
//! а не ссылкой на чужой CDN: иконка грузится с диска.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::Duration;

/// Начало адреса иконки для движка.
pub const ICON_BASE: &str = "http://gorn-icon.localhost/item/";

struct Cache {
    dir: PathBuf,
    /// Кэш Горна на Electron — читается, если своего файла ещё нет.
    old_dir: Option<PathBuf>,
    sources: HashMap<i64, String>,
    failed: Mutex<HashSet<i64>>,
    in_flight: Mutex<HashSet<i64>>,
    done: Condvar,
}

static CACHE: OnceLock<Cache> = OnceLock::new();

pub fn init(data_dir: &Path, old_dir: Option<PathBuf>, items: &[gorn_core::engine::CatalogueItem]) {
    let dir = data_dir.join("icons");
    let _ = std::fs::create_dir_all(&dir);
    let sources = items
        .iter()
        .filter_map(|it| it.icon.clone().map(|u| (it.id, u)))
        .collect();
    let _ = CACHE.set(Cache {
        dir,
        old_dir: old_dir.map(|d| d.join("icons")),
        sources,
        failed: Default::default(),
        in_flight: Default::default(),
        done: Condvar::new(),
    });
}

fn is_webp(d: &[u8]) -> bool {
    d.len() >= 12 && &d[0..4] == b"RIFF" && &d[8..12] == b"WEBP"
}

fn read_cached(file: &Path) -> Option<Vec<u8>> {
    let d = std::fs::read(file).ok()?;
    if is_webp(&d) {
        Some(d)
    } else {
        // Битый файл (оборванная запись) — удалить и скачать заново.
        let _ = std::fs::remove_file(file);
        None
    }
}

/// Иконка предмета или None (нет в справочнике, CDN недоступен).
pub fn get(id: i64) -> Option<Vec<u8>> {
    let c = CACHE.get()?;
    let file = c.dir.join(format!("{id}.webp"));
    if let Some(d) = read_cached(&file) {
        return Some(d);
    }
    if let Some(d) = c
        .old_dir
        .as_ref()
        .and_then(|o| std::fs::read(o.join(format!("{id}.webp"))).ok())
        .filter(|d| is_webp(d))
    {
        let _ = write_atomic(&file, &d);
        return Some(d);
    }
    if c.failed.lock().unwrap().contains(&id) {
        return None;
    }
    // Одна загрузка на предмет: остальные ждут её.
    {
        let mut busy = c.in_flight.lock().unwrap();
        while busy.contains(&id) {
            busy = c.done.wait(busy).unwrap();
        }
        if let Some(d) = read_cached(&file) {
            return Some(d);
        }
        busy.insert(id);
    }
    let got = download(c, id, &file);
    c.in_flight.lock().unwrap().remove(&id);
    c.done.notify_all();
    got
}

fn download(c: &Cache, id: i64, file: &Path) -> Option<Vec<u8>> {
    let Some(url) = c.sources.get(&id) else {
        c.failed.lock().unwrap().insert(id);
        return None;
    };
    match ureq::get(url).timeout(Duration::from_secs(15)).call() {
        Ok(res) => {
            let mut data = vec![];
            std::io::Read::read_to_end(&mut res.into_reader(), &mut data).ok()?;
            if !is_webp(&data) {
                return None;
            }
            let _ = write_atomic(file, &data);
            Some(data)
        }
        Err(ureq::Error::Status(404 | 410, _)) => {
            c.failed.lock().unwrap().insert(id);
            None
        }
        Err(_) => None,
    }
}

fn write_atomic(file: &Path, data: &[u8]) -> std::io::Result<()> {
    let tmp = file.with_extension(format!("{}.tmp", std::process::id()));
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, file).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// Подкачать иконки, которых ещё нет на диске; сколько загружено.
pub fn prefetch(ids: &[i64]) -> usize {
    let Some(c) = CACHE.get() else { return 0 };
    let todo: Vec<i64> = ids
        .iter()
        .copied()
        .filter(|id| c.sources.contains_key(id) && !c.dir.join(format!("{id}.webp")).exists())
        .collect();
    let next = Mutex::new(todo.into_iter());
    let count = std::sync::atomic::AtomicUsize::new(0);
    std::thread::scope(|s| {
        for _ in 0..2 {
            s.spawn(|| loop {
                let Some(id) = next.lock().unwrap().next() else {
                    break;
                };
                if get(id).is_some() {
                    count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
            });
        }
    });
    count.into_inner()
}

/// Предмет из адреса: `/item/100300.webp` (Tauri) или `/100300.webp` (адрес эталона).
pub fn id_from_path(path: &str) -> Option<i64> {
    let name = path.rsplit('/').next()?;
    name.strip_suffix(".webp")?
        .parse()
        .ok()
        .filter(|&id| id > 0)
}
