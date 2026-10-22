//! 自动化与运行记录的落盘:`automations.json` + `runs/<id>.json`。

use crate::{Automation, AutomationError, RunRecord};
use std::path::{Path, PathBuf};

/// 每个自动化最多保留的运行记录数。
const MAX_RUNS: usize = 30;

#[derive(Debug, Clone)]
pub struct AutomationStore {
    dir: PathBuf,
}

impl AutomationStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// 默认目录:`<home>/.ucode/automations`。
    pub fn default_dir(home: &Path) -> PathBuf {
        home.join(".ucode").join("automations")
    }

    fn list_file(&self) -> PathBuf {
        self.dir.join("automations.json")
    }

    fn runs_file(&self, id: &str) -> Option<PathBuf> {
        let ok = !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        ok.then(|| self.dir.join("runs").join(format!("{id}.json")))
    }

    fn write_atomic(path: &Path, text: &str) -> Result<(), AutomationError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| AutomationError::Io(e.to_string()))?;
        }
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, text).map_err(|e| AutomationError::Io(e.to_string()))?;
        std::fs::rename(&tmp, path).map_err(|e| AutomationError::Io(e.to_string()))
    }

    /// 读取全部自动化;文件不存在视为空,损坏时返回错误而不是静默清空。
    pub fn load_all(&self) -> Result<Vec<Automation>, AutomationError> {
        match std::fs::read_to_string(self.list_file()) {
            Ok(text) => serde_json::from_str(&text).map_err(|e| AutomationError::Format(e.to_string())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(AutomationError::Io(e.to_string())),
        }
    }

    pub fn save_all(&self, items: &[Automation]) -> Result<(), AutomationError> {
        let text = serde_json::to_string_pretty(items).map_err(|e| AutomationError::Format(e.to_string()))?;
        Self::write_atomic(&self.list_file(), &text)
    }

    /// 新增或按 id 覆盖。
    pub fn upsert(&self, item: Automation) -> Result<(), AutomationError> {
        let mut all = self.load_all()?;
        match all.iter_mut().find(|a| a.id == item.id) {
            Some(slot) => *slot = item,
            None => all.push(item),
        }
        self.save_all(&all)
    }

    pub fn remove(&self, id: &str) -> Result<(), AutomationError> {
        let mut all = self.load_all()?;
        all.retain(|a| a.id != id);
        self.save_all(&all)?;
        if let Some(f) = self.runs_file(id) {
            let _ = std::fs::remove_file(f);
        }
        Ok(())
    }

    /// 运行记录,最新的在前。
    pub fn runs(&self, id: &str) -> Vec<RunRecord> {
        self.runs_file(id)
            .and_then(|f| std::fs::read_to_string(f).ok())
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    /// 追加一条运行记录(同一 `started_at` 的记录会被覆盖,用于「运行中 → 完成」的更新)。
    pub fn record_run(&self, id: &str, run: RunRecord) -> Result<(), AutomationError> {
        let file = self.runs_file(id).ok_or_else(|| AutomationError::Format(format!("非法 id:{id}")))?;
        let mut runs = self.runs(id);
        runs.retain(|r| r.started_at != run.started_at);
        runs.insert(0, run);
        runs.truncate(MAX_RUNS);
        let text = serde_json::to_string_pretty(&runs).map_err(|e| AutomationError::Format(e.to_string()))?;
        Self::write_atomic(&file, &text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RunStatus, tests::sample};

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ucode-auto-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn 增删改查() {
        let dir = temp("crud");
        let store = AutomationStore::new(&dir);
        assert!(store.load_all().unwrap().is_empty());
        store.upsert(sample("a")).unwrap();
        store.upsert(sample("b")).unwrap();
        let mut a = sample("a");
        a.name = "改名".into();
        store.upsert(a).unwrap();
        let all = store.load_all().unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].name, "改名");
        store.remove("a").unwrap();
        assert_eq!(store.load_all().unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 损坏的列表返回错误而不是清空() {
        let dir = temp("corrupt");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("automations.json"), "{坏").unwrap();
        assert!(AutomationStore::new(&dir).load_all().is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 运行记录截断并支持覆盖更新() {
        let dir = temp("runs");
        let store = AutomationStore::new(&dir);
        let rec = |t: i64, s: RunStatus| RunRecord { started_at: t, finished_at: None, status: s, summary: String::new(), cost_usd: 0.0, session_id: None };
        store.record_run("a", rec(1, RunStatus::Running)).unwrap();
        store.record_run("a", rec(1, RunStatus::Succeeded)).unwrap();
        assert_eq!(store.runs("a").len(), 1);
        assert_eq!(store.runs("a")[0].status, RunStatus::Succeeded);
        for t in 2..50 {
            store.record_run("a", rec(t, RunStatus::Succeeded)).unwrap();
        }
        let runs = store.runs("a");
        assert_eq!(runs.len(), MAX_RUNS);
        assert_eq!(runs[0].started_at, 49, "最新在前");
        assert!(store.record_run("../x", rec(1, RunStatus::Running)).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
