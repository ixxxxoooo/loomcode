//! 会话概要索引:从 jsonl 里提取标题与工作目录,流式读取 + 增量缓存。
//!
//! 会话文件只追加不改写,所以缓存里记下「已扫描到的字节偏移」与已提取的字段:
//! 文件变大只扫新增部分;变小或文件头变了才从头重扫;大小与修改时间都没变直接用缓存。
//! 读取按行进行,内存峰值只是最长的一行,而不是整个文件(会话文件常有几十 MB)。
//!
//! 缓存常驻内存;应用启动时用 [`set_cache_file`] 指定落盘位置后,下次启动也能直接命中。
//!
//! @author ixxxxoooo

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

/// 用来识别「文件被整体替换」的文件头长度。
const HEAD_LEN: usize = 256;

/// 一个会话文件的扫描进度与已提取的字段。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Scan {
    /// 已扫描到的字节偏移(总在完整行的末尾)
    pub offset: u64,
    pub size: u64,
    /// 修改时间(自纪元起的纳秒)
    pub mtime: u64,
    /// 文件头的哈希
    pub head: u64,
    pub cwd: String,
    pub ai_title: Option<String>,
    pub last_prompt: Option<String>,
    pub first_user: Option<String>,
}

impl Scan {
    /// 展示用标题:`ai-title` 优先,其次第一条用户输入,再次最后一条提示;都没有返回 `None`(不展示)。
    pub fn title(&self) -> Option<String> {
        let t = self.ai_title.as_ref().or(self.first_user.as_ref()).or(self.last_prompt.as_ref())?;
        Some(t.lines().next().unwrap_or_default().chars().take(60).collect())
    }
}

#[derive(Default)]
struct Index {
    scans: HashMap<PathBuf, Scan>,
    file: Option<PathBuf>,
    dirty: bool,
}

fn index() -> &'static Mutex<Index> {
    static INDEX: OnceLock<Mutex<Index>> = OnceLock::new();
    INDEX.get_or_init(Default::default)
}

/// 指定缓存落盘位置并载入已有缓存(应用启动时调用一次;不调用则只在内存里缓存)。
///
/// 参数:`path` 缓存文件路径,如 `~/.loomcode/session-index.json`。
pub fn set_cache_file(path: PathBuf) {
    let loaded: HashMap<PathBuf, Scan> = std::fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    let mut ix = index().lock().unwrap_or_else(|e| e.into_inner());
    for (k, v) in loaded {
        ix.scans.entry(k).or_insert(v);
    }
    ix.file = Some(path);
}

/// 有变化时把缓存写盘(先写临时文件再改名);只保留仍然存在的文件。
pub fn flush() {
    let (file, snapshot) = {
        let mut ix = index().lock().unwrap_or_else(|e| e.into_inner());
        let Some(file) = ix.file.clone().filter(|_| ix.dirty) else { return };
        ix.scans.retain(|p, _| p.exists());
        ix.dirty = false;
        (file, ix.scans.clone())
    };
    let Ok(text) = serde_json::to_string(&snapshot) else { return };
    let tmp = file.with_extension("json.tmp");
    if std::fs::write(&tmp, text).is_ok() && std::fs::rename(&tmp, &file).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

fn nanos(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64)
}

fn hash_bytes(b: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    b.hash(&mut h);
    h.finish()
}

/// 取一个会话文件的扫描结果(命中缓存 / 增量扫描 / 全量扫描)。
///
/// 参数:`path` 会话文件;`size`、`modified` 来自调用方已取到的元数据。返回:扫描结果;文件读不了返回 `None`。
pub fn scan(path: &Path, size: u64, modified: SystemTime) -> Option<Scan> {
    let mtime = nanos(modified);
    let cached = index().lock().unwrap_or_else(|e| e.into_inner()).scans.get(path).cloned();
    if let Some(c) = &cached
        && c.size == size
        && c.mtime == mtime
    {
        return cached;
    }
    let fresh = scan_file(path, size, mtime, cached)?;
    let mut ix = index().lock().unwrap_or_else(|e| e.into_inner());
    ix.scans.insert(path.to_path_buf(), fresh.clone());
    ix.dirty = true;
    Some(fresh)
}

/// 真正读文件:文件头没变且没有变小就从上次的偏移继续,否则从头扫。
fn scan_file(path: &Path, size: u64, mtime: u64, prev: Option<Scan>) -> Option<Scan> {
    let mut f = std::fs::File::open(path).ok()?;
    let mut head = vec![0u8; HEAD_LEN.min(size as usize)];
    f.read_exact(&mut head).ok()?;
    let head = hash_bytes(&head);
    let mut s = match prev {
        // 上次扫描时文件不足 HEAD_LEN,头部哈希会随增长变化,保守起见从头扫
        Some(p) if p.head == head && p.size >= HEAD_LEN as u64 && size >= p.offset => p,
        _ => Scan::default(),
    };
    (s.size, s.mtime, s.head) = (size, mtime, head);
    f.seek(SeekFrom::Start(s.offset)).ok()?;
    let mut reader = BufReader::with_capacity(64 * 1024, f);
    let mut buf: Vec<u8> = Vec::new();
    loop {
        buf.clear();
        let n = reader.read_until(b'\n', &mut buf).ok()?;
        // 末尾没有换行的半行(claude 正在写)留到下次再读
        if n == 0 || buf.last() != Some(&b'\n') {
            break;
        }
        s.offset += n as u64;
        if let Ok(line) = std::str::from_utf8(&buf) {
            absorb(&mut s, line.trim_end());
        }
    }
    Some(s)
}

/// 吸收一行:只解析可能有用的行,避免对巨大的会话文件做全量 JSON 解析。
fn absorb(s: &mut Scan, line: &str) {
    let interesting = line.starts_with(r#"{"type":"ai-title""#)
        || line.starts_with(r#"{"type":"last-prompt""#)
        || (s.cwd.is_empty() && line.contains(r#""cwd":""#))
        || (s.first_user.is_none() && line.starts_with(r#"{"type":"user""#));
    if !interesting {
        return;
    }
    let Ok(v) = serde_json::from_str::<Value>(line) else { return };
    if s.cwd.is_empty()
        && let Some(c) = v.get("cwd").and_then(Value::as_str)
    {
        s.cwd = c.into();
    }
    match v.get("type").and_then(Value::as_str) {
        Some("ai-title") => s.ai_title = v.get("aiTitle").and_then(Value::as_str).map(String::from),
        Some("last-prompt") => s.last_prompt = v.get("lastPrompt").and_then(Value::as_str).map(String::from),
        Some("user") if s.first_user.is_none() => {
            s.first_user = user_text(&v).filter(|t| !t.starts_with('<') && !t.starts_with("Caveat:"));
        }
        _ => {}
    }
}

fn user_text(v: &Value) -> Option<String> {
    match &v["message"]["content"] {
        Value::String(s) => Some(s.clone()),
        Value::Array(a) => a
            .iter()
            .find(|b| b.get("type").and_then(Value::as_str) == Some("text"))
            .and_then(|b| b.get("text").and_then(Value::as_str))
            .map(String::from),
        _ => None,
    }
}

#[cfg(test)]
#[path = "session_index_tests.rs"]
mod tests;
