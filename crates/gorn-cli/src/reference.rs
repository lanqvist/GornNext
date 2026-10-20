//! Запуск эталона на TS (tools/reference.mts) из соседнего gorn-tli.
//!
//! Скрипт идёт через node с загрузчиком tsx из node_modules gorn-tli —
//! без npx и без второго процесса; вывод читается из канала.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

/// Где лежат эталон и скрипт.
#[derive(Clone, Debug)]
pub struct Reference {
    pub tli: PathBuf,
    pub script: PathBuf,
}

impl Reference {
    /// По умолчанию: `../gorn-tli` рядом с GornNext, скрипт — tools/reference.mts этого репозитория.
    pub fn locate(tli: Option<&str>) -> Result<Reference, String> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let root = root.canonicalize().map_err(|e| format!("{}: {e}", root.display()))?;
        let tli = match tli {
            Some(t) => PathBuf::from(t),
            None => root.join("../gorn-tli"),
        };
        let tli = tli.canonicalize().map_err(|e| format!("эталон {}: {e}", tli.display()))?;
        let script = root.join("tools/reference.mts");
        if !script.exists() {
            return Err(format!("нет {}", script.display()));
        }
        if !tli.join("node_modules/tsx/dist/loader.mjs").exists() {
            return Err(format!("в {} нет node_modules/tsx — npm install в эталоне", tli.display()));
        }
        Ok(Reference { tli, script })
    }

    fn url(p: &Path) -> String {
        let s = p.to_string_lossy().replace('\\', "/");
        let s = s.trim_start_matches("//?/");
        if s.starts_with('/') {
            format!("file://{s}")
        } else {
            format!("file:///{s}")
        }
    }

    fn plain(p: &Path) -> String {
        p.to_string_lossy().trim_start_matches(r"\\?\").to_string()
    }

    /// Запустить команду эталона; stdout — в канал.
    pub fn spawn(&self, args: &[String]) -> Result<Child, String> {
        let loader = Self::url(&self.tli.join("node_modules/tsx/dist/loader.mjs"));
        Command::new("node")
            .arg("--import")
            .arg(loader)
            .arg(Self::plain(&self.script))
            .arg(Self::plain(&self.tli))
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("node: {e}"))
    }

    /// Запустить и прочитать весь вывод.
    pub fn output(&self, args: &[String]) -> Result<Vec<u8>, String> {
        let child = self.spawn(args)?;
        let out = child.wait_with_output().map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err(format!("эталон {args:?}: {}", out.status));
        }
        Ok(out.stdout)
    }
}
