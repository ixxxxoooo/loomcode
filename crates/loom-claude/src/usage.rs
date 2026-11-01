//! 用量统计:扫描 `~/.claude/projects/**/*.jsonl`,按「日期(UTC)+ 模型」汇总 token、回复数、工具调用与提问数。
//!
//! 每个文件按 (大小, 修改时间) 增量缓存到 `usage-index.json`,只重新解析变化过的文件。
//! 同一条助手消息在 jsonl 里会按内容块拆成多行、重复携带 usage,这里按消息 id 去重并取各字段最大值。
//!
//! @author ixxxxoooo

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

/// 一个文件里某天某模型的汇总。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DayModel {
    pub date: String,
    pub model: String,
    pub input: u64,
    pub output: u64,
    pub cache_create: u64,
    pub cache_read: u64,
    /// 助手回复条数(按消息 id 去重)
    pub replies: u64,
    pub tools: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct FileEntry {
    size: u64,
    mtime: u64,
    rows: Vec<DayModel>,
    /// 日期 → 用户提问数
    prompts: BTreeMap<String, u64>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Index {
    files: HashMap<String, FileEntry>,
}

/// 某一天的合计。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DayTotal {
    pub date: String,
    pub tokens: u64,
    pub output: u64,
    pub replies: u64,
    pub tools: u64,
    pub prompts: u64,
    pub sessions: u64,
}

/// 统计结果。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UsageReport {
    /// 最近 30 天(含今天,旧 → 新,没有数据的日子为 0)
    pub days: Vec<DayTotal>,
    /// 按模型累计:(模型, 输入, 输出, 缓存读取, 缓存写入, 回复数),按总量降序
    pub by_model: Vec<(String, u64, u64, u64, u64, u64)>,
    pub total_input: u64,
    pub total_output: u64,
    pub total_cache_read: u64,
    pub total_cache_create: u64,
    pub files: usize,
}

impl UsageReport {
    /// 缓存命中率:缓存读取 / (输入 + 缓存读取 + 缓存写入)。
    pub fn cache_hit_rate(&self) -> f64 {
        let all = self.total_input + self.total_cache_read + self.total_cache_create;
        if all == 0 { 0. } else { self.total_cache_read as f64 / all as f64 }
    }

    /// 最近 `n` 天的合计。
    pub fn recent(&self, n: usize) -> DayTotal {
        let mut t = DayTotal::default();
        for d in self.days.iter().rev().take(n) {
            t.tokens += d.tokens;
            t.output += d.output;
            t.replies += d.replies;
            t.tools += d.tools;
            t.prompts += d.prompts;
            t.sessions += d.sessions;
        }
        t
    }
}

/// 解析一个 jsonl 文件的全部内容。
fn parse_file(text: &str) -> FileEntry {
    // 消息 id → (日期, 模型, [输入, 输出, 缓存写, 缓存读])
    let mut msgs: HashMap<String, (String, String, [u64; 4])> = HashMap::new();
    let mut tools: HashMap<(String, String), u64> = HashMap::new();
    let mut prompts: BTreeMap<String, u64> = BTreeMap::new();
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        let date = v["timestamp"].as_str().unwrap_or("").get(..10).unwrap_or("").to_string();
        if date.is_empty() {
            continue;
        }
        let msg = &v["message"];
        match v["type"].as_str() {
            Some("user") if !v["isMeta"].as_bool().unwrap_or(false) => {
                let is_prompt = match &msg["content"] {
                    Value::String(s) => !s.starts_with('<'),
                    Value::Array(a) => a.iter().any(|b| b["type"] == "text") && !a.iter().any(|b| b["type"] == "tool_result"),
                    _ => false,
                };
                if is_prompt {
                    *prompts.entry(date).or_default() += 1;
                }
            }
            Some("assistant") => {
                let model = msg["model"].as_str().unwrap_or("").to_string();
                if model.is_empty() || model == "<synthetic>" {
                    continue;
                }
                let u = &msg["usage"];
                let n = |k: &str| u[k].as_u64().unwrap_or(0);
                let usage = [n("input_tokens"), n("output_tokens"), n("cache_creation_input_tokens"), n("cache_read_input_tokens")];
                let id = msg["id"].as_str().map(str::to_string).unwrap_or_else(|| v["uuid"].as_str().unwrap_or("").to_string());
                let e = msgs.entry(id).or_insert_with(|| (date.clone(), model.clone(), [0; 4]));
                for (a, b) in e.2.iter_mut().zip(usage) {
                    *a = (*a).max(b);
                }
                let tool_count = msg["content"].as_array().map(|a| a.iter().filter(|b| b["type"] == "tool_use").count() as u64).unwrap_or(0);
                *tools.entry((date, model)).or_default() += tool_count;
            }
            _ => {}
        }
    }
    let mut rows: BTreeMap<(String, String), DayModel> = BTreeMap::new();
    for (_, (date, model, u)) in msgs {
        let r = rows.entry((date.clone(), model.clone())).or_insert_with(|| DayModel { date, model, ..Default::default() });
        r.input += u[0];
        r.output += u[1];
        r.cache_create += u[2];
        r.cache_read += u[3];
        r.replies += 1;
    }
    for ((date, model), n) in tools {
        if let Some(r) = rows.get_mut(&(date, model)) {
            r.tools += n;
        }
    }
    FileEntry { size: 0, mtime: 0, rows: rows.into_values().collect(), prompts }
}

/// 公历日期(UTC):距 1970-01-01 的天数 → `YYYY-MM-DD`。
fn civil(days: i64) -> String {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// 汇总成报告。
///
/// 参数:`entries` 各文件结果;`today` 今天距纪元的天数(UTC)。
fn build_report(entries: &[&FileEntry], today: i64) -> UsageReport {
    let dates: Vec<String> = (0..30).rev().map(|i| civil(today - i)).collect();
    let mut days: Vec<DayTotal> = dates.iter().map(|d| DayTotal { date: d.clone(), ..Default::default() }).collect();
    let mut models: HashMap<String, (u64, u64, u64, u64, u64)> = HashMap::new();
    let mut r = UsageReport { files: entries.len(), ..Default::default() };
    for e in entries {
        let mut active: Vec<&str> = Vec::new();
        for row in &e.rows {
            let m = models.entry(row.model.clone()).or_default();
            (m.0, m.1, m.2, m.3, m.4) = (m.0 + row.input, m.1 + row.output, m.2 + row.cache_read, m.3 + row.cache_create, m.4 + row.replies);
            r.total_input += row.input;
            r.total_output += row.output;
            r.total_cache_read += row.cache_read;
            r.total_cache_create += row.cache_create;
            if let Some(d) = days.iter_mut().find(|d| d.date == row.date) {
                d.tokens += row.input + row.output + row.cache_read + row.cache_create;
                d.output += row.output;
                d.replies += row.replies;
                d.tools += row.tools;
                if !active.contains(&row.date.as_str()) {
                    active.push(&row.date);
                    d.sessions += 1;
                }
            }
        }
        for (date, n) in &e.prompts {
            if let Some(d) = days.iter_mut().find(|d| &d.date == date) {
                d.prompts += n;
            }
        }
    }
    r.days = days;
    r.by_model = models.into_iter().map(|(k, v)| (k, v.0, v.1, v.2, v.3, v.4)).collect();
    r.by_model.sort_by_key(|m| std::cmp::Reverse(m.1 + m.2 + m.3 + m.4));
    r
}

/// 递归收集 jsonl 文件(含子 Agent 的 `subagents/` 目录)。
fn collect(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(&p, out);
        } else if p.extension().is_some_and(|x| x == "jsonl") {
            out.push(p);
        }
    }
}

/// 每百万 token 的公开参考价(美元):(输入, 输出)。缓存读取按输入价 0.1 倍、缓存写入按 1.25 倍估算。
///
/// 参数:`model` 模型 id(按名称里的系列判断)。返回:未知模型(如第三方服务商的模型)返回 `None`。
pub fn price_per_mtok(model: &str) -> Option<(f64, f64)> {
    let m = model.to_lowercase();
    if m.contains("opus") {
        Some((15., 75.))
    } else if m.contains("sonnet") {
        Some((3., 15.))
    } else if m.contains("haiku") {
        Some(if m.contains("3-5") || m.contains("3.5") { (0.8, 4.) } else { (1., 5.) })
    } else {
        None
    }
}

/// 估算一组 token 的费用(美元);模型价格未知时返回 `None`。
pub fn estimate_cost(model: &str, input: u64, output: u64, cache_read: u64, cache_write: u64) -> Option<f64> {
    let (pi, po) = price_per_mtok(model)?;
    let tok = |n: u64| n as f64 / 1e6;
    Some(tok(input) * pi + tok(output) * po + tok(cache_read) * pi * 0.1 + tok(cache_write) * pi * 1.25)
}

impl UsageReport {
    /// 累计估算费用:只计入价格已知的模型;返回 (美元, 是否有模型因价格未知被略过)。
    pub fn estimated_cost(&self) -> (f64, bool) {
        let mut skipped = false;
        let mut sum = 0.;
        for (model, input, output, cr, cw, _) in &self.by_model {
            match estimate_cost(model, *input, *output, *cr, *cw) {
                Some(c) => sum += c,
                None => skipped = true,
            }
        }
        (sum, skipped)
    }
}

/// 扫描并生成报告,同时更新缓存。耗时操作,请在后台线程调用。
///
/// 参数:`projects_dir` 通常是 `~/.claude/projects`;`cache_path` 缓存文件。返回:报告。
pub fn scan(projects_dir: &Path, cache_path: &Path) -> UsageReport {
    let mut index: Index = std::fs::read_to_string(cache_path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    let mut files = Vec::new();
    collect(projects_dir, &mut files);
    let mut fresh: HashMap<String, FileEntry> = HashMap::new();
    for p in files {
        let Ok(meta) = std::fs::metadata(&p) else { continue };
        let mtime = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
        let key = p.to_string_lossy().to_string();
        let entry = match index.files.remove(&key) {
            Some(e) if e.size == meta.len() && e.mtime == mtime => e,
            _ => FileEntry { size: meta.len(), mtime, ..parse_file(&std::fs::read_to_string(&p).unwrap_or_default()) },
        };
        fresh.insert(key, entry);
    }
    index.files = fresh;
    if let Ok(text) = serde_json::to_string(&index) {
        let _ = std::fs::write(cache_path, text);
    }
    let today = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64 / 86_400);
    build_report(&index.files.values().collect::<Vec<_>>(), today)
}

#[cfg(test)]
#[path = "usage_tests.rs"]
mod tests;
