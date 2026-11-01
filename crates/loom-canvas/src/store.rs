//! 画布落盘:每个文档一个 `<id>.json`。

use crate::model::{CanvasDocument, CanvasError};
use std::path::{Path, PathBuf};

/// 列表里的一项。
#[derive(Debug, Clone, PartialEq)]
pub struct CanvasSummary {
    pub id: String,
    pub title: String,
    pub updated_at: i64,
}

/// 画布存储目录。
#[derive(Debug, Clone)]
pub struct CanvasStore {
    dir: PathBuf,
}

impl CanvasStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// 默认目录:`<home>/.loomcode/canvases`。
    pub fn default_dir(home: &Path) -> PathBuf {
        home.join(".loomcode").join("canvases")
    }

    /// 把 id 限制为安全文件名,防止路径穿越。
    fn file(&self, id: &str) -> Option<PathBuf> {
        let ok = !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        ok.then(|| self.dir.join(format!("{id}.json")))
    }

    /// 校验并原子写入。
    pub fn save(&self, doc: &CanvasDocument) -> Result<(), CanvasError> {
        doc.validate()?;
        let path = self.file(&doc.id).ok_or_else(|| CanvasError::Format(format!("非法的画布 id:{}", doc.id)))?;
        std::fs::create_dir_all(&self.dir).map_err(|e| CanvasError::Io(e.to_string()))?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, doc.to_json()).map_err(|e| CanvasError::Io(e.to_string()))?;
        std::fs::rename(&tmp, &path).map_err(|e| CanvasError::Io(e.to_string()))
    }

    pub fn load(&self, id: &str) -> Result<CanvasDocument, CanvasError> {
        let path = self.file(id).ok_or_else(|| CanvasError::Format(format!("非法的画布 id:{id}")))?;
        let text = std::fs::read_to_string(path).map_err(|e| CanvasError::Io(e.to_string()))?;
        CanvasDocument::from_json(&text)
    }

    pub fn delete(&self, id: &str) -> Result<(), CanvasError> {
        let path = self.file(id).ok_or_else(|| CanvasError::Format(format!("非法的画布 id:{id}")))?;
        std::fs::remove_file(path).map_err(|e| CanvasError::Io(e.to_string()))
    }

    /// 列出全部画布,最近更新的在前;损坏的文件被跳过。
    pub fn list(&self) -> Vec<CanvasSummary> {
        let Ok(rd) = std::fs::read_dir(&self.dir) else { return Vec::new() };
        let mut out: Vec<CanvasSummary> = rd
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
            .filter_map(|e| {
                let id = e.path().file_stem()?.to_string_lossy().into_owned();
                let doc = self.load(&id).ok()?;
                Some(CanvasSummary { id, title: doc.title, updated_at: doc.updated_at })
            })
            .collect();
        out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo_document;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("loom-canvas-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn 保存读取列出删除() {
        let dir = temp("store");
        let store = CanvasStore::new(&dir);
        let doc = demo_document("/work/demo", 1_000);
        store.save(&doc).unwrap();
        assert_eq!(store.load(&doc.id).unwrap(), doc);
        std::fs::write(dir.join("bad.json"), "{坏的").unwrap();
        let list = store.list();
        assert_eq!(list.len(), 1, "损坏文件被跳过");
        assert_eq!(list[0].title, doc.title);
        store.delete(&doc.id).unwrap();
        assert!(store.load(&doc.id).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 拒绝路径穿越的_id() {
        let store = CanvasStore::new(temp("evil"));
        let mut doc = demo_document("/w", 0);
        doc.id = "../escape".into();
        assert!(store.save(&doc).is_err());
        assert!(store.load("../x").is_err());
    }
}
