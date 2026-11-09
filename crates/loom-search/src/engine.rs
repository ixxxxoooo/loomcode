//! 搜索引擎:`ignore` 并行遍历(遵守 .gitignore)+ 正则逐行匹配,可取消,结果数封顶。
//!
//! @author ixxxxoooo

use crate::preview::{Preview, make_preview};
use crate::query::SearchQuery;
use ignore::{WalkBuilder, WalkState};
use regex::Regex;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// 最多收集的匹配数(与 VS Code 默认 `maxResults` 一致)。
pub const MAX_RESULTS: usize = 20_000;
/// 超过这个大小的文件不搜索。
pub const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
/// 判断二进制时检查的开头字节数。
const BINARY_PROBE: usize = 8000;

/// 一处匹配(一行里有多处就是多条)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    /// 行号(1 起始)
    pub line: u32,
    /// 匹配起止列(字符数,0 起始,左闭右开),与编辑器 `Position.character` 同单位
    pub start: u32,
    pub end: u32,
    pub preview: Preview,
}

/// 一个文件里的全部匹配。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMatches {
    pub path: PathBuf,
    pub matches: Vec<Match>,
}

/// 搜索结果。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchOutcome {
    /// 按路径排序的文件
    pub files: Vec<FileMatches>,
    /// 匹配总数
    pub total: usize,
    /// 是否因达到上限被截断
    pub truncated: bool,
    /// 实际读过的文件数
    pub files_scanned: usize,
    /// 是否被中途取消
    pub cancelled: bool,
}

/// 扫描一段文本,返回逐处匹配(最多 `limit` 条);空匹配忽略。
///
/// 参数:`text` 文本;`re` 已编译的正则;`limit` 上限。
/// 返回:匹配列表。
pub(crate) fn scan_text(text: &str, re: &Regex, limit: usize) -> Vec<Match> {
    let mut out = Vec::new();
    for (i, raw) in text.split('\n').enumerate() {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        // 字节偏移 → 字符列:沿同一行增量计数,避免超长行上 O(n²)
        let (mut last_byte, mut last_col) = (0usize, 0u32);
        for m in re.find_iter(line) {
            if m.is_empty() {
                continue;
            }
            let start = last_col + line[last_byte..m.start()].chars().count() as u32;
            let end = start + m.as_str().chars().count() as u32;
            (last_byte, last_col) = (m.start(), start);
            out.push(Match { line: i as u32 + 1, start, end, preview: make_preview(line, start as usize, end as usize) });
            if out.len() >= limit {
                return out;
            }
        }
    }
    out
}

/// 读文件并扫描:二进制、非 UTF-8、过大的文件返回 `None`。
fn scan_file(path: &Path, re: &Regex, limit: usize) -> Option<Vec<Match>> {
    if std::fs::metadata(path).ok()?.len() > MAX_FILE_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    if bytes[..bytes.len().min(BINARY_PROBE)].contains(&0) {
        return None;
    }
    let text = String::from_utf8(bytes).ok()?;
    Some(scan_text(&text, re, limit))
}

/// 在 `root` 下搜索。
///
/// 参数:`root` 项目根;`q` 条件;`cancel` 取消标志(置 true 后尽快返回,结果里 `cancelled = true`)。
/// 返回:结果;条件非法(空关键字、坏正则、坏 glob)时返回错误文案。
pub fn search(root: &Path, q: &SearchQuery, cancel: &AtomicBool) -> Result<SearchOutcome, String> {
    let re = q.build_regex()?;
    let overrides = q.build_globs(root)?;
    let use_ignore = q.use_ignore_files;
    let mut walk = WalkBuilder::new(root);
    walk.hidden(false).follow_links(false).overrides(overrides)
        .git_ignore(use_ignore).git_global(use_ignore).git_exclude(use_ignore).ignore(use_ignore).parents(use_ignore).require_git(false)
        .threads(std::thread::available_parallelism().map_or(4, |n| n.get().min(8)))
        .filter_entry(|e| e.file_name() != ".git");
    let (found, total, scanned) = (Mutex::new(Vec::<FileMatches>::new()), AtomicUsize::new(0), AtomicUsize::new(0));
    walk.build_parallel().run(|| {
        let (re, found, total, scanned) = (re.clone(), &found, &total, &scanned);
        Box::new(move |entry| {
            if cancel.load(Ordering::Relaxed) || total.load(Ordering::Relaxed) >= MAX_RESULTS {
                return WalkState::Quit;
            }
            let Ok(entry) = entry else { return WalkState::Continue };
            if !entry.file_type().is_some_and(|t| t.is_file()) {
                return WalkState::Continue;
            }
            let room = MAX_RESULTS.saturating_sub(total.load(Ordering::Relaxed)).max(1);
            if let Some(matches) = scan_file(entry.path(), &re, room) {
                scanned.fetch_add(1, Ordering::Relaxed);
                if !matches.is_empty() {
                    total.fetch_add(matches.len(), Ordering::Relaxed);
                    if let Ok(mut f) = found.lock() {
                        f.push(FileMatches { path: entry.into_path(), matches });
                    }
                }
            }
            WalkState::Continue
        })
    });
    let mut files = found.into_inner().unwrap_or_default();
    files.sort_by(|a, b| a.path.cmp(&b.path));
    // 并行时各线程可能略微超出上限,按顺序截齐
    let mut total = 0usize;
    let mut truncated = false;
    for f in &mut files {
        if total + f.matches.len() > MAX_RESULTS {
            f.matches.truncate(MAX_RESULTS - total);
            truncated = true;
        }
        total += f.matches.len();
    }
    files.retain(|f| !f.matches.is_empty());
    truncated |= total >= MAX_RESULTS;
    Ok(SearchOutcome { files, total, truncated, files_scanned: scanned.load(Ordering::Relaxed), cancelled: cancel.load(Ordering::Relaxed) })
}

/// 在多个根目录下搜索(多根工作区),结果合并。
///
/// 参数:`roots` 各根目录(互相嵌套时同一文件只算一次);`q` 条件;`cancel` 取消标志。
/// 返回:合并后的结果(按路径排序,总数仍受 `MAX_RESULTS` 限制);任一根的条件非法时返回错误文案。
pub fn search_many(roots: &[PathBuf], q: &SearchQuery, cancel: &AtomicBool) -> Result<SearchOutcome, String> {
    let mut all = SearchOutcome { files: Vec::new(), total: 0, truncated: false, files_scanned: 0, cancelled: false };
    let mut seen = std::collections::HashSet::new();
    for root in roots {
        if all.total >= MAX_RESULTS {
            all.truncated = true;
            break;
        }
        let o = search(root, q, cancel)?;
        all.files_scanned += o.files_scanned;
        all.truncated |= o.truncated;
        all.cancelled |= o.cancelled;
        for mut f in o.files {
            if !seen.insert(f.path.clone()) {
                continue;
            }
            let room = MAX_RESULTS - all.total;
            if f.matches.len() > room {
                f.matches.truncate(room);
                all.truncated = true;
            }
            all.total += f.matches.len();
            if !f.matches.is_empty() {
                all.files.push(f);
            }
        }
        if all.cancelled {
            break;
        }
    }
    all.files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(all)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn sandbox(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("loom-search-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::create_dir_all(dir.join("target")).unwrap();
        fs::create_dir_all(dir.join("node_modules/x")).unwrap();
        fs::write(dir.join(".gitignore"), "target/\n").unwrap();
        fs::write(dir.join("a.txt"), "hello\nWorld Foo foo\n").unwrap();
        fs::write(dir.join("src/b.rs"), "fn foo() {}\r\n    let foo = 1;\n").unwrap();
        fs::write(dir.join("target/c.txt"), "foo\n").unwrap();
        fs::write(dir.join("node_modules/x/d.js"), "foo\n").unwrap();
        fs::write(dir.join("bin.dat"), b"foo\0\0foo").unwrap();
        dir
    }

    fn run(dir: &Path, q: &SearchQuery) -> SearchOutcome {
        search(dir, q, &AtomicBool::new(false)).unwrap()
    }

    #[test]
    fn 多根搜索合并结果_嵌套根不重复() {
        let dir = sandbox("many");
        let q = SearchQuery { pattern: "foo".into(), ..Default::default() };
        let one = run(&dir, &q);
        // 根目录与它的子目录同时作为根:src 里的文件只算一次
        let many = search_many(&[dir.clone(), dir.join("src")], &q, &AtomicBool::new(false)).unwrap();
        assert_eq!(many.total, one.total);
        assert_eq!(many.files.len(), one.files.len());
        let _ = fs::remove_dir_all(&dir);
    }

    fn names(o: &SearchOutcome, root: &Path) -> Vec<String> {
        o.files.iter().map(|f| f.path.strip_prefix(root).unwrap().to_string_lossy().to_string()).collect()
    }

    #[test]
    fn 遵守忽略文件并跳过二进制() {
        let dir = sandbox("ignore");
        let o = run(&dir, &SearchQuery::new("foo"));
        assert_eq!(names(&o, &dir), vec!["a.txt", "src/b.rs"]);
        assert_eq!(o.total, 4, "a.txt 里两处 + b.rs 里两处");
        let mut q = SearchQuery::new("foo");
        q.use_ignore_files = false;
        assert!(names(&run(&dir, &q), &dir).contains(&"target/c.txt".to_string()));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn 列号按字符计且行内多处各算一条() {
        let dir = sandbox("cols");
        fs::write(dir.join("zh.txt"), "你好 foo 世界 foo\n").unwrap();
        let o = run(&dir, &SearchQuery::new("foo"));
        let zh = o.files.iter().find(|f| f.path.ends_with("zh.txt")).unwrap();
        assert_eq!(zh.matches.iter().map(|m| (m.start, m.end)).collect::<Vec<_>>(), vec![(3, 6), (10, 13)]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn 包含与排除() {
        let dir = sandbox("glob");
        let mut q = SearchQuery::new("foo");
        q.include = "src".into();
        assert_eq!(names(&run(&dir, &q), &dir), vec!["src/b.rs"]);
        q.include = "*.txt".into();
        assert_eq!(names(&run(&dir, &q), &dir), vec!["a.txt"]);
        q.include = String::new();
        q.exclude = "src, *.md".into();
        assert_eq!(names(&run(&dir, &q), &dir), vec!["a.txt"]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn 正则全词与大小写() {
        let dir = sandbox("re");
        let mut q = SearchQuery::new(r"f\w+");
        q.regex = true;
        q.case_sensitive = true;
        assert_eq!(run(&dir, &q).total, 4);
        let mut q = SearchQuery::new("Foo");
        q.case_sensitive = true;
        assert_eq!(run(&dir, &q).total, 1);
        let mut q = SearchQuery::new("fo");
        q.whole_word = true;
        assert_eq!(run(&dir, &q).total, 0);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn 取消与非法条件() {
        let dir = sandbox("cancel");
        let o = search(&dir, &SearchQuery::new("foo"), &AtomicBool::new(true)).unwrap();
        assert!(o.cancelled);
        assert!(search(&dir, &SearchQuery::new(""), &AtomicBool::new(false)).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn 结果数封顶() {
        let dir = sandbox("cap");
        let big = "x\n".repeat(MAX_RESULTS / 2 + 10);
        for i in 0..3 {
            fs::write(dir.join(format!("big{i}.txt")), &big).unwrap();
        }
        let o = run(&dir, &SearchQuery::new("x"));
        assert_eq!(o.total, MAX_RESULTS);
        assert!(o.truncated);
        let _ = fs::remove_dir_all(&dir);
    }
}
