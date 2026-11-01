//! 提交说明生成(对标 Cursor / VS Code Copilot 的「生成提交消息」按钮)。
//!
//! 把待提交的补丁、文件清单与最近提交的标题一起交给小模型,让它学习本仓库的写法
//! (语言、是否 Conventional Commits、粒度),输出「标题 + 可选要点正文」;
//! 没有可用模型或调用失败时,退回到基于补丁内容的启发式生成(不只看文件名)。
//! 本模块只含纯逻辑(材料解析、补丁精简、提示词、结果清洗、启发式)与读取材料的 git 调用,
//! 不含模型调用(见 `loom-claude::rewrite::generate_commit_message`)。
//!
//! 提示词与启发式的结构参考了 OhMyAgent 的 `CommitMessageGenerator`。
//!
//! @author ixxxxoooo

use crate::git::git;
use std::collections::HashMap;

/// 发给模型的补丁字符预算。
pub const PATCH_BUDGET: usize = 14_000;
/// 单个文件补丁的字符上限(避免一个大文件挤掉其他文件)。
pub const PER_FILE_BUDGET: usize = 3_000;
/// 标题行的建议最大长度。
pub const SUBJECT_LIMIT: usize = 72;

/// 一个被改动的文件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    /// 仓库相对路径
    pub path: String,
    /// 状态字母:A 新增 / M 修改 / D 删除 / R 重命名 / ? 未跟踪
    pub status: char,
    pub added: usize,
    pub removed: usize,
}

/// 生成提交说明的上下文。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Context {
    pub branch: String,
    pub files: Vec<FileChange>,
    /// 统一 diff 补丁(可能很长,生成前会精简)
    pub patch: String,
    /// 最近提交的标题(新到旧),用于学习风格
    pub recent_subjects: Vec<String>,
    /// 材料来自暂存区(true)还是相对 HEAD 的全部改动(false)
    pub staged: bool,
}

// ---------- 材料 ----------

/// 把 `--name-status` 与 `--numstat` 输出合并成文件清单。
///
/// `name_status` 每行 `状态<TAB>路径`(重命名为 `R100<TAB>旧<TAB>新`);`numstat` 每行 `新增<TAB>删除<TAB>路径`(二进制为 `-`);
/// `untracked` 是以补丁形式附上的未跟踪文件。按路径去重。
pub fn parse_files(name_status: &str, numstat: &str, untracked: &[String]) -> Vec<FileChange> {
    let mut counts: HashMap<&str, (usize, usize)> = HashMap::new();
    for line in numstat.lines() {
        let p: Vec<&str> = line.split('\t').collect();
        if p.len() >= 3 {
            counts.insert(p[2], (p[0].parse().unwrap_or(0), p[1].parse().unwrap_or(0)));
        }
    }
    let mut out: Vec<FileChange> = Vec::new();
    for line in name_status.lines() {
        let p: Vec<&str> = line.split('\t').collect();
        if p.len() < 2 {
            continue;
        }
        let path = p[p.len() - 1];
        if out.iter().any(|f| f.path == path) {
            continue;
        }
        let (added, removed) = counts.get(path).copied().unwrap_or((0, 0));
        out.push(FileChange { path: path.to_string(), status: p[0].chars().next().unwrap_or('M'), added, removed });
    }
    for path in untracked {
        if !out.iter().any(|f| &f.path == path) {
            out.push(FileChange { path: path.clone(), status: 'A', added: 0, removed: 0 });
        }
    }
    out
}

/// 收集生成提交说明所需的材料。
///
/// 有暂存内容时只看暂存区(与 `git commit` 一致),否则看相对 HEAD 的全部改动,并附上未跟踪文件的开头几行。
/// 参数:`dir` 仓库根;`untracked` 未跟踪文件(仅在没有暂存内容时纳入)。读取失败的部分留空。
pub fn collect(dir: &str, untracked: &[String]) -> Context {
    let has_head = git(dir, &["rev-parse", "--verify", "-q", "HEAD"]).is_ok();
    let staged_names = git(dir, &["diff", "--cached", "--name-only"]).unwrap_or_default();
    let staged = !staged_names.trim().is_empty();
    let base: Vec<&str> = if staged || !has_head { vec!["diff", "--cached"] } else { vec!["diff", "HEAD"] };
    let run = |extra: &[&str]| {
        let mut args = base.clone();
        args.extend(["--no-color", "--no-ext-diff"]);
        args.extend(extra);
        git(dir, &args).unwrap_or_default()
    };
    let mut patch = run(&["--unified=2"]);
    let name_status = run(&["--name-status"]);
    let numstat = run(&["--numstat"]);
    let mut extra_files = Vec::new();
    if !staged {
        for rel in untracked.iter().take(20) {
            let abs = std::path::Path::new(dir).join(rel);
            let Ok(meta) = std::fs::metadata(&abs) else { continue };
            if meta.len() >= 200_000 {
                continue;
            }
            let Ok(bytes) = std::fs::read(&abs) else { continue };
            if bytes.iter().take(8000).any(|b| *b == 0) {
                continue;
            }
            let text = String::from_utf8_lossy(&bytes);
            let head: Vec<String> = text.lines().take(40).map(|l| format!("+{l}")).collect();
            patch.push_str(&format!("\ndiff --git a/{rel} b/{rel}\nnew file mode 100644\n--- /dev/null\n+++ b/{rel}\n{}\n", head.join("\n")));
            extra_files.push(rel.clone());
        }
    }
    if patch.chars().count() > 60_000 {
        patch = patch.chars().take(60_000).collect();
    }
    let recent_subjects = git(dir, &["--no-optional-locks", "log", "-n10", "--format=%s"]).map(|o| o.lines().map(str::to_string).collect()).unwrap_or_default();
    let branch = git(dir, &["symbolic-ref", "--short", "-q", "HEAD"]).map(|s| s.trim().to_string()).unwrap_or_default();
    let mut files = parse_files(&name_status, &numstat, &extra_files);
    if files.is_empty() {
        // 兜底:补丁读不到时直接用传入的未跟踪列表
        files = untracked.iter().map(|p| FileChange { path: p.clone(), status: 'A', added: 0, removed: 0 }).collect();
    }
    Context { branch, files, patch, recent_subjects, staged }
}

// ---------- 补丁精简 ----------

/// 噪声文件:锁文件、生成物、资源,补丁内容对写提交说明没有帮助。
pub fn is_noise(path: &str) -> bool {
    let lower = path.to_lowercase();
    let name = lower.rsplit('/').next().unwrap_or(&lower);
    if ["package-lock.json", "yarn.lock", "pnpm-lock.yaml", "package.resolved", "podfile.lock", "cargo.lock", "gemfile.lock", "go.sum"].contains(&name) {
        return true;
    }
    if [".min.js", ".svg", ".png", ".pdf", ".icns", ".xcassets"].iter().any(|e| lower.ends_with(e)) {
        return true;
    }
    lower.contains("/generated/") || lower.contains("/vendor/") || lower.starts_with("vendor/") || lower.starts_with("build/") || lower.starts_with("dist/")
}

/// 精简补丁:去掉噪声文件,每个文件按上限截断,总量不超过预算;文件之间保持原有顺序。
pub fn condense_patch(patch: &str, budget: usize, per_file: usize) -> String {
    let mut chunks: Vec<(String, String)> = Vec::new();
    let (mut cur, mut cur_path): (Vec<&str>, String) = (Vec::new(), String::new());
    for line in patch.split('\n') {
        if line.starts_with("diff --git ") {
            if !cur.is_empty() {
                chunks.push((std::mem::take(&mut cur_path), cur.join("\n")));
                cur.clear();
            }
            cur_path = line.rsplit(' ').next().map(|p| p.strip_prefix("b/").unwrap_or(p).to_string()).unwrap_or_default();
        }
        cur.push(line);
    }
    if !cur.is_empty() {
        chunks.push((cur_path, cur.join("\n")));
    }
    let (mut out, mut omitted) = (String::new(), Vec::new());
    for (path, text) in chunks {
        if path.is_empty() && text.trim().is_empty() {
            continue;
        }
        if is_noise(&path) {
            omitted.push(path);
            continue;
        }
        let mut text = text;
        if text.chars().count() > per_file {
            text = text.chars().take(per_file).collect::<String>() + "\n… (该文件改动较长,已截断)";
        }
        if out.chars().count() + text.chars().count() > budget {
            omitted.push(path);
            continue;
        }
        out.push_str(&text);
        out.push('\n');
    }
    if !omitted.is_empty() {
        out.push_str(&format!("\n(以下文件的补丁已省略:{})\n", omitted.iter().take(12).cloned().collect::<Vec<_>>().join(", ")));
    }
    out
}

// ---------- 提示词 ----------

/// 最近提交的标题里是否以中文为主(决定默认输出语言)。没有历史时默认中文。
pub fn prefers_chinese(subjects: &[String]) -> bool {
    let sample: String = subjects.iter().take(6).flat_map(|s| s.chars()).collect();
    if sample.is_empty() {
        return true;
    }
    let cjk = sample.chars().filter(|c| ('\u{4E00}'..='\u{9FFF}').contains(c)).count();
    cjk > 0 && cjk * 10 >= sample.chars().count()
}

const CONVENTIONAL: [&str; 11] = ["feat", "fix", "docs", "style", "refactor", "perf", "test", "build", "ci", "chore", "revert"];

/// 最近提交是否基本遵循 Conventional Commits(没有历史时默认是)。
pub fn uses_conventional(subjects: &[String]) -> bool {
    let sample: Vec<&String> = subjects.iter().take(8).collect();
    if sample.is_empty() {
        return true;
    }
    let hits = sample
        .iter()
        .filter(|s| {
            let Some(i) = s.find([':', '：']) else { return false };
            let head = &s[..i];
            let ty = head.split('(').next().unwrap_or(head).trim_end_matches('!').trim();
            CONVENTIONAL.contains(&ty)
        })
        .count();
    hits * 2 >= sample.len()
}

/// 构造发给模型的完整单轮提示词。
pub fn prompt(ctx: &Context) -> String {
    let chinese = prefers_chinese(&ctx.recent_subjects);
    let conventional = uses_conventional(&ctx.recent_subjects);
    let files: Vec<String> = ctx.files.iter().take(40).map(|f| format!("{} {} (+{} −{})", f.status, f.path, f.added, f.removed)).collect();
    let recent: Vec<String> = ctx.recent_subjects.iter().take(8).map(|s| format!("- {s}")).collect();
    let mut rules = vec![
        "你是一位资深工程师,正在为下面这次代码改动写 git 提交说明。".to_string(),
        format!("格式:第一行是标题(不超过 {SUBJECT_LIMIT} 个字符,不加句号),概括这次改动的整体目的,可用「并」串联 2 个相关方面;空一行后写 3 到 6 条「- 」要点,每条一行,以句号结尾。"),
        "每条要点说清「在哪个文件/模块、新增或修改了什么、起什么作用」;文件名、路径、类型名、函数名、常量、依赖、配置键用反引号包裹(如 `Cargo.toml`、`ScmView`)。".to_string(),
        "要点按重要性排序,同类改动(如多个文档/测试/参数说明)合并为一条;不要逐个罗列文件,也不要写「更新了若干文件」这类空话。".to_string(),
        "只描述这次改动的实际内容,不要编造补丁里没有的内容;纯重命名/格式化等琐碎改动可以只写标题。".to_string(),
        "要点示例(仅示意风格,勿照抄内容):\n- 在`Cargo.toml`中添加`lsp-types 0.97`依赖。\n- 在`scm.rs`中新增`ScmView::generate_message`方法。\n- 修改`README.md`,补充 Git 图谱的说明。".to_string(),
        format!("输出语言:{}。", if chinese { "中文(标识符和专有名词保持原样)" } else { "英文" }),
        if conventional {
            "标题使用 Conventional Commits:`type(scope): 描述`,type 取 feat/fix/refactor/docs/test/chore/perf/style/build/ci 之一,scope 取受影响的模块(可省略)。type 必须按改动实质判断:feat 仅用于全新的用户可见能力;修改、调整、优化已有行为或文案、修正问题用 fix;不改行为的结构调整用 refactor;只动文档/测试/配置/脚本分别用 docs/test/chore。拿不准时不要默认 feat。".to_string()
        } else {
            "标题模仿下面「最近的提交」的写法与粒度。".to_string()
        },
        "只输出提交说明本身,不要代码块围栏、引号、解释或前缀。".to_string(),
    ];
    if !ctx.branch.is_empty() {
        rules.push(format!("当前分支:{}", ctx.branch));
    }
    let mut text = rules.join("\n");
    if !recent.is_empty() {
        text += &format!("\n\n本仓库最近的提交(学习其风格):\n{}", recent.join("\n"));
    }
    text += &format!("\n\n改动文件:\n{}", files.join("\n"));
    text += &format!("\n\n补丁:\n{}", condense_patch(&ctx.patch, PATCH_BUDGET, PER_FILE_BUDGET));
    text
}

// ---------- 结果清洗 ----------

/// 清洗模型输出:去代码围栏、整体引号、「提交说明:」前缀,压缩多余空行,标题行超长则截断。
/// 没有有效内容返回 `None`。
pub fn clean(raw: &str) -> Option<String> {
    let normalized = raw.replace("\r\n", "\n");
    let mut text = normalized.split('\n').filter(|l| !l.trim().starts_with("```")).collect::<Vec<_>>().join("\n").trim().to_string();
    for prefix in ["提交说明:", "提交说明：", "提交信息:", "提交信息：", "Commit message:", "commit message:", "Commit Message:"] {
        if let Some(rest) = text.strip_prefix(prefix) {
            text = rest.trim().to_string();
            break;
        }
    }
    let quotes = ['"', '\'', '“', '”', '‘', '’', '`'];
    let chars: Vec<char> = text.chars().collect();
    if chars.len() > 2 && quotes.contains(&chars[0]) && quotes.contains(&chars[chars.len() - 1]) {
        text = chars[1..chars.len() - 1].iter().collect::<String>().trim().to_string();
    }
    if text.is_empty() {
        return None;
    }
    let mut out: Vec<String> = Vec::new();
    for line in text.split('\n') {
        let l = line.trim_end().to_string();
        if l.is_empty() && out.last().is_none_or(|p| p.is_empty()) {
            continue;
        }
        out.push(l);
    }
    let first = out.first()?.clone();
    let mut subject = first.trim_end_matches(['。', '.']).to_string();
    if subject.chars().count() > SUBJECT_LIMIT + 20 {
        subject = subject.chars().take(SUBJECT_LIMIT + 17).collect::<String>() + "…";
    }
    out[0] = subject;
    Some(out.join("\n"))
}

// ---------- 启发式兜底 ----------

/// 推断 Conventional Commits 的 type。
pub fn infer_type(files: &[FileChange], patch: &str) -> &'static str {
    let real: Vec<&FileChange> = files.iter().filter(|f| !is_noise(&f.path)).collect();
    let pool: Vec<&FileChange> = if real.is_empty() { files.iter().collect() } else { real };
    let all = |test: &dyn Fn(&str) -> bool| pool.iter().all(|f| test(&f.path.to_lowercase()));
    if all(&|p| p.contains("test") || p.contains("spec")) {
        return "test";
    }
    if all(&|p| p.ends_with(".md") || p.starts_with("docs/") || p.contains("/docs/") || p.ends_with(".txt")) {
        return "docs";
    }
    if all(&|p| p.starts_with(".github/") || (p.contains("ci") && p.ends_with(".yml"))) {
        return "ci";
    }
    if all(&|p| [".json", ".plist", ".yml", ".yaml", ".toml", ".sh", ".strings"].iter().any(|e| p.ends_with(e)) || p.starts_with("scripts/")) {
        return "chore";
    }
    let added: Vec<String> = patch.lines().filter(|l| l.starts_with('+') && !l.starts_with("+++")).map(str::to_lowercase).collect();
    let fix_hits = added.iter().filter(|l| ["fix", "bug", "crash", "修复", "fallback"].iter().any(|k| l.contains(k))).count();
    let new_sources = pool.iter().filter(|f| (f.status == 'A' || f.status == '?') && !f.path.to_lowercase().contains("test")).count();
    if new_sources > 0 && new_sources * 2 >= pool.len() {
        return "feat";
    }
    let total_added: usize = pool.iter().map(|f| f.added).sum();
    let total_removed: usize = pool.iter().map(|f| f.removed).sum();
    if fix_hits > 0 && fix_hits * 6 >= added.len().max(1) && total_added < 120 {
        return "fix";
    }
    if total_removed > total_added && total_removed > 20 {
        return "refactor";
    }
    if total_added >= 200 && total_removed * 3 < total_added { "feat" } else { "fix" }
}

/// 推断作用域:所有文件共享的模块 / crate / 目录;否则取改动最多的(过半才算)。
pub fn infer_scope(files: &[FileChange]) -> String {
    let modules: Vec<String> = files
        .iter()
        .filter_map(|f| {
            let parts: Vec<&str> = f.path.split('/').collect();
            // crates/<名>/src/... 取 crate 名;src|Sources|Tests/<名>/... 取第二段
            if parts.len() >= 3 && (parts[0] == "crates" || parts[0] == "packages") {
                return Some(parts[1].to_string());
            }
            if parts.len() >= 3 && ["Sources", "Tests", "src"].contains(&parts[0]) {
                return Some(parts[1].to_string());
            }
            (parts.len() >= 2).then(|| parts[0].to_string())
        })
        .collect();
    if modules.is_empty() {
        return String::new();
    }
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for m in &modules {
        *counts.entry(m).or_default() += 1;
    }
    let Some((top, n)) = counts.iter().max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0))).map(|(k, v)| (k.to_string(), *v)) else { return String::new() };
    if counts.len() == 1 || n * 2 >= modules.len() { top } else { String::new() }
}

/// 取新增行里声明的类型 / 函数名(去重,保持出现顺序;覆盖 Rust / Swift / Go / Python / JS / TS)。
pub fn added_symbols(patch: &str, limit: usize) -> Vec<String> {
    const KEYWORDS: [&str; 12] = ["fn", "struct", "enum", "trait", "impl", "class", "func", "def", "function", "interface", "protocol", "actor"];
    let mut seen: Vec<String> = Vec::new();
    for line in patch.lines().filter(|l| l.starts_with('+') && !l.starts_with("+++")) {
        let toks: Vec<&str> = line[1..].split(|c: char| c.is_whitespace() || c == '(' || c == '<' || c == '{' || c == ':').filter(|t| !t.is_empty()).collect();
        let Some(i) = toks.iter().position(|t| KEYWORDS.contains(t)) else { continue };
        // 关键字之前只允许修饰词
        if toks[..i].iter().any(|t| !["pub", "pub(crate)", "async", "const", "unsafe", "public", "private", "internal", "open", "static", "final", "export", "default", "extension", "@MainActor", "nonisolated", "fileprivate", "abstract"].contains(t)) {
            continue;
        }
        if let Some(name) = toks.get(i + 1) {
            let name: String = name.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
            if name.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_') && !seen.contains(&name) {
                seen.push(name);
            }
        }
        if seen.len() >= limit {
            break;
        }
    }
    seen
}

fn describe(ty: &str, files: &[FileChange], symbols: &[String], chinese: bool) -> String {
    let names: Vec<&str> = files.iter().take(2).map(|f| f.path.rsplit('/').next().unwrap_or(&f.path)).collect();
    let sep = if chinese { "、" } else { ", " };
    let more = if files.len() > 2 { if chinese { format!(" 等 {} 个文件", files.len()) } else { format!(" and {} more", files.len() - 2) } } else { String::new() };
    let target = names.join(sep) + &more;
    if !symbols.is_empty() && ["feat", "refactor", "fix"].contains(&ty) {
        let list = symbols.join(sep);
        return match (ty, chinese) {
            ("feat", true) => format!("新增 {list}"),
            ("feat", false) => format!("add {list}"),
            ("refactor", true) => format!("重构 {list}"),
            ("refactor", false) => format!("refactor {list}"),
            (_, true) => format!("修复 {list} 相关问题"),
            (_, false) => format!("fix issues in {list}"),
        };
    }
    match (ty, chinese) {
        ("docs", true) => format!("更新文档 {target}"),
        ("docs", false) => format!("update docs {target}"),
        ("test", true) => format!("补充测试 {target}"),
        ("test", false) => format!("update tests {target}"),
        ("feat", true) => format!("新增 {target}"),
        ("feat", false) => format!("add {target}"),
        ("fix", true) => format!("修复 {target} 中的问题"),
        ("fix", false) => format!("fix issues in {target}"),
        ("refactor", true) => format!("重构 {target}"),
        ("refactor", false) => format!("refactor {target}"),
        (_, true) => format!("更新 {target}"),
        (_, false) => format!("update {target}"),
    }
}

/// 基于补丁内容的启发式生成(没有可用模型时使用)。没有文件时返回空串。
pub fn heuristic(ctx: &Context) -> String {
    let files = &ctx.files;
    if files.is_empty() {
        return String::new();
    }
    let chinese = prefers_chinese(&ctx.recent_subjects);
    let conventional = uses_conventional(&ctx.recent_subjects);
    let ty = infer_type(files, &ctx.patch);
    let scope = infer_scope(files);
    let symbols = added_symbols(&ctx.patch, 3);
    let subject = describe(ty, files, &symbols, chinese);
    let mut head = match (conventional, scope.is_empty()) {
        (false, _) => subject,
        (true, true) => format!("{ty}: {subject}"),
        (true, false) => format!("{ty}({scope}): {subject}"),
    };
    if head.chars().count() > SUBJECT_LIMIT + 20 {
        head = head.chars().take(SUBJECT_LIMIT + 17).collect::<String>() + "…";
    }
    if files.len() == 1 {
        return head;
    }
    let mut bullets: Vec<String> = files
        .iter()
        .take(6)
        .map(|f| {
            let name = f.path.rsplit('/').next().unwrap_or(&f.path);
            let verb = match (f.status, chinese) {
                ('A' | '?', true) => "新增",
                ('A' | '?', false) => "add",
                ('D', true) => "删除",
                ('D', false) => "remove",
                ('R', true) => "重命名",
                ('R', false) => "rename",
                (_, true) => "更新",
                (_, false) => "update",
            };
            let delta = if f.added + f.removed > 0 { format!(" (+{} −{})", f.added, f.removed) } else { String::new() };
            format!("- {verb} {name}{delta}")
        })
        .collect();
    if files.len() > 6 {
        bullets.push(if chinese { format!("- …等共 {} 个文件", files.len()) } else { format!("- … and {} more files", files.len() - 6) });
    }
    format!("{head}\n\n{}", bullets.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(path: &str, status: char, a: usize, r: usize) -> FileChange {
        FileChange { path: path.into(), status, added: a, removed: r }
    }

    #[test]
    fn 合并文件清单并按路径去重() {
        let files = parse_files("M\ta.rs\nR100\told.rs\tnew.rs\nM\ta.rs\n", "3\t1\ta.rs\n5\t0\tnew.rs\n", &["u.txt".into()]);
        assert_eq!(files.len(), 3);
        assert_eq!(files[0], f("a.rs", 'M', 3, 1));
        assert_eq!(files[1], f("new.rs", 'R', 5, 0));
        assert_eq!(files[2].status, 'A');
    }

    #[test]
    fn 噪声文件被识别() {
        assert!(is_noise("Cargo.lock") && is_noise("web/package-lock.json") && is_noise("assets/icon.png") && is_noise("vendor/x.rs"));
        assert!(!is_noise("src/main.rs"));
    }

    #[test]
    fn 精简补丁去噪声_截断并保序() {
        let patch = "diff --git a/Cargo.lock b/Cargo.lock\n+lock\ndiff --git a/a.rs b/a.rs\n+fn a() {}\ndiff --git a/b.rs b/b.rs\n+fn b() {}\n";
        let out = condense_patch(patch, 10_000, 3_000);
        assert!(!out.contains("+lock") && out.contains("Cargo.lock"), "噪声补丁被省略并在末尾列出");
        assert!(out.find("a.rs").unwrap() < out.find("b.rs").unwrap());
        let long = format!("diff --git a/a.rs b/a.rs\n{}\n", "+x\n".repeat(5000));
        assert!(condense_patch(&long, 100_000, 200).contains("已截断"));
        // 总预算:后面的文件被省略
        let two = format!("diff --git a/a.rs b/a.rs\n{}\ndiff --git a/b.rs b/b.rs\n+tail\n", "+x".repeat(100));
        let out = condense_patch(&two, 240, 3_000);
        assert!(!out.contains("+tail") && out.contains("b.rs"));
    }

    #[test]
    fn 语言与风格判断() {
        let zh: Vec<String> = ["feat: 新增图谱", "fix(scm): 修复提交"].iter().map(|s| s.to_string()).collect();
        assert!(prefers_chinese(&zh) && uses_conventional(&zh));
        let en: Vec<String> = ["Add graph", "Fix commit bug"].iter().map(|s| s.to_string()).collect();
        assert!(!prefers_chinese(&en) && !uses_conventional(&en));
        assert!(prefers_chinese(&[]) && uses_conventional(&[]), "没有历史默认中文 + Conventional");
    }

    #[test]
    fn 提示词包含材料与规则() {
        let ctx = Context { branch: "main".into(), files: vec![f("src/a.rs", 'M', 3, 1)], patch: "diff --git a/src/a.rs b/src/a.rs\n+fn a() {}\n".into(), recent_subjects: vec!["feat: 旧提交".into()], staged: true };
        let p = prompt(&ctx);
        assert!(p.contains("Conventional Commits") && p.contains("当前分支:main") && p.contains("- feat: 旧提交") && p.contains("M src/a.rs (+3 −1)") && p.contains("+fn a() {}"));
        assert!(p.contains("中文"));
    }

    #[test]
    fn 清洗去围栏引号前缀和句号() {
        assert_eq!(clean("```\nfeat: 标题。\n\n\n\n- 要点。\n```").as_deref(), Some("feat: 标题\n\n- 要点。"));
        assert_eq!(clean("\"fix: 修复\"").as_deref(), Some("fix: 修复"));
        assert_eq!(clean("提交说明: docs: 更新").as_deref(), Some("docs: 更新"));
        assert_eq!(clean("  \n ```\n```"), None);
        let long = "a".repeat(200);
        assert!(clean(&long).unwrap().chars().count() <= SUBJECT_LIMIT + 18);
    }

    #[test]
    fn 类型与作用域推断() {
        assert_eq!(infer_type(&[f("tests/a.rs", 'M', 1, 1)], ""), "test");
        assert_eq!(infer_type(&[f("README.md", 'M', 1, 1), f("docs/a.md", 'A', 3, 0)], ""), "docs");
        assert_eq!(infer_type(&[f("Cargo.toml", 'M', 1, 1)], ""), "chore");
        assert_eq!(infer_type(&[f("src/new.rs", 'A', 30, 0)], ""), "feat");
        assert_eq!(infer_type(&[f("src/a.rs", 'M', 5, 1)], "+    // fix crash\n"), "fix");
        assert_eq!(infer_type(&[f("src/a.rs", 'M', 2, 60)], ""), "refactor");
        assert_eq!(infer_scope(&[f("crates/loom-app/src/a.rs", 'M', 1, 1), f("crates/loom-app/src/b.rs", 'M', 1, 1)]), "loom-app");
        assert_eq!(infer_scope(&[f("crates/a/x.rs", 'M', 1, 1), f("crates/b/y.rs", 'M', 1, 1), f("docs/z.md", 'M', 1, 1)]), "");
        assert_eq!(infer_scope(&[f("README.md", 'M', 1, 1)]), "");
    }

    #[test]
    fn 新增符号提取() {
        let patch = "+++ b/a.rs\n+pub fn parse_log(out: &str) {\n+    let x = 1;\n+pub struct Commit {\n+class Foo:\n+def bar(x):\n+    return fn_call()\n";
        assert_eq!(added_symbols(patch, 5), vec!["parse_log", "Commit", "Foo", "bar"]);
        assert_eq!(added_symbols(patch, 2).len(), 2);
    }

    #[test]
    fn 启发式生成标题与要点() {
        let ctx = Context {
            files: vec![f("crates/loom-core/src/git_log.rs", 'A', 200, 0), f("crates/loom-core/src/lib.rs", 'M', 1, 0)],
            patch: "+pub fn layout() {}\n".into(),
            ..Default::default()
        };
        let m = heuristic(&ctx);
        assert!(m.starts_with("feat(loom-core): 新增 layout"), "{m}");
        assert!(m.contains("- 新增 git_log.rs (+200 −0)") && m.contains("- 更新 lib.rs"));
        let one = heuristic(&Context { files: vec![f("README.md", 'M', 1, 1)], ..Default::default() });
        assert_eq!(one, "docs: 更新文档 README.md");
        assert_eq!(heuristic(&Context::default()), "");
    }

    #[test]
    fn 真实仓库收集材料_暂存优先() {
        let dir = std::env::temp_dir().join(format!("loom-cmsg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let d = dir.to_str().unwrap();
        let sh = |args: &[&str]| {
            assert!(std::process::Command::new("git").arg("-C").arg(d).args(args).env("GIT_AUTHOR_NAME", "T").env("GIT_AUTHOR_EMAIL", "t@t").env("GIT_COMMITTER_NAME", "T").env("GIT_COMMITTER_EMAIL", "t@t").output().unwrap().status.success());
        };
        sh(&["init", "-q", "-b", "main"]);
        std::fs::write(dir.join("a.txt"), "1\n").unwrap();
        sh(&["add", "."]);
        sh(&["commit", "-q", "-m", "feat: 初始"]);
        std::fs::write(dir.join("a.txt"), "1\n2\n").unwrap();
        std::fs::write(dir.join("new.txt"), "hello\n").unwrap();
        // 没有暂存:看全部改动 + 未跟踪文件
        let ctx = collect(d, &["new.txt".into()]);
        assert!(!ctx.staged && ctx.branch == "main" && ctx.recent_subjects == vec!["feat: 初始"]);
        assert!(ctx.files.iter().any(|x| x.path == "a.txt") && ctx.files.iter().any(|x| x.path == "new.txt"));
        assert!(ctx.patch.contains("+hello"), "未跟踪文件以补丁形式附上");
        // 有暂存:只看暂存区
        sh(&["add", "a.txt"]);
        let ctx = collect(d, &["new.txt".into()]);
        assert!(ctx.staged && ctx.files.len() == 1 && !ctx.patch.contains("hello"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
