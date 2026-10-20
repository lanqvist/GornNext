//! Автопоиск папки с логами игры и список логов. Перенос `core/locate.ts`.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

const LOG_SUBPATH: &str = r"UE_game\TorchLight\Saved\Logs";

/// Возможные названия папки игры: глобальная и китайская версии.
const GAME_DIRS: [&str; 2] = ["Torchlight Infinite", "Torchlight Infinite CN"];

/// Папки Steam-библиотек из libraryfolders.vdf.
fn steam_libraries() -> Vec<PathBuf> {
    let env = |k: &str, d: &str| std::env::var(k).unwrap_or_else(|_| d.to_string());
    let roots = [
        Path::new(&env("ProgramFiles(x86)", r"C:\Program Files (x86)")).join("Steam"),
        Path::new(&env("ProgramFiles", r"C:\Program Files")).join("Steam"),
    ];
    let re = regex::Regex::new(r#""path"\s+"([^"]+)""#).unwrap();
    let mut libs: Vec<PathBuf> = vec![];
    for root in roots {
        if !root.exists() {
            continue;
        }
        libs.push(root.clone());
        // В vdf интересны только строки вида  "path"  "D:\\SteamLibrary"
        if let Ok(text) = std::fs::read_to_string(root.join(r"steamapps\libraryfolders.vdf")) {
            for m in re.captures_iter(&text) {
                libs.push(PathBuf::from(m[1].replace(r"\\", r"\")));
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    libs.retain(|p| seen.insert(p.clone()));
    libs
}

/// Путь к папке Logs или None.
pub fn find_log_directory() -> Option<PathBuf> {
    for lib in steam_libraries() {
        for game in GAME_DIRS {
            let dir = lib.join("steamapps").join("common").join(game).join(LOG_SUBPATH);
            if dir.exists() {
                return Some(dir);
            }
        }
    }
    None
}

/// Папка, указанная игроком вручную: сама Logs или корень установки игры.
pub fn resolve_log_directory(picked: &Path) -> Option<PathBuf> {
    let candidates = [picked.to_path_buf(), picked.join(LOG_SUBPATH)];
    // Сначала папка, в которой логи реально лежат.
    for dir in &candidates {
        if dir.exists() && list_log_files(dir).is_ok_and(|v| !v.is_empty()) {
            return Some(dir.clone());
        }
    }
    // Логов может не быть вовсе (игра ещё не запускалась) — тогда папка с правильным именем.
    candidates.into_iter().find(|d| d.exists() && d.to_string_lossy().ends_with("Logs"))
}

#[derive(Clone, Debug, PartialEq)]
pub struct LogFile {
    pub path: PathBuf,
    /// Время последней записи — по нему упорядочиваем сессии.
    pub mtime: SystemTime,
    pub size: u64,
    /// Текущий лог, в который игра пишет прямо сейчас.
    pub is_current: bool,
}

/// Все логи в папке, от старых к новым; текущий UE_game.log — последний.
pub fn list_log_files(dir: &Path) -> std::io::Result<Vec<LogFile>> {
    let mut files = vec![];
    for e in std::fs::read_dir(dir)? {
        let e = e?;
        let name = e.file_name().to_string_lossy().to_string();
        if !name.starts_with("UE_game") || !name.ends_with(".log") {
            continue;
        }
        let path = dir.join(&name);
        let md = match std::fs::metadata(&path) {
            Ok(m) => m,
            // Игра удалила старый бэкап между чтением папки и stat.
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => return Err(err),
        };
        if md.len() == 0 {
            continue;
        }
        files.push(LogFile { path, mtime: md.modified()?, size: md.len(), is_current: name == "UE_game.log" });
    }
    files.sort_by(|a, b| a.is_current.cmp(&b.is_current).then(a.mtime.cmp(&b.mtime)));
    Ok(files)
}
