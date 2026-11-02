//! Go 悬停里的 `pkg.go.dev` 链接清理。
//!
//! gopls 会在悬停末尾附一行 ``[`pkg.Name` on pkg.go.dev](https://pkg.go.dev/<包路径>#Name)``。
//! 私有模块(公司内部仓库、当前工作区自己的模块)在 pkg.go.dev 上根本不存在,点开只会 404。
//! gopls 自己会按 `GOPRIVATE` 省掉这行,但从图形界面启动的应用拿不到 shell 里导出的 `GOPRIVATE`,
//! 所以这里一方面把用户的 `GOPRIVATE` 找出来传给 gopls,另一方面对悬停结果再兜底过滤一次
//! (按 `GOPRIVATE` 与工作区 `go.mod` 的模块路径判断),公开依赖与标准库的链接照旧保留。
//!
//! @author ixxxxoooo

use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::Duration;

const PKG_SITE: &str = "https://pkg.go.dev/";

/// 单个 glob 段匹配(`*` 匹配任意非 `/` 序列,`?` 匹配一个字符),与 `path.Match` 的常用子集一致。
fn glob(pat: &[char], s: &[char]) -> bool {
    match pat.first() {
        None => s.is_empty(),
        Some('*') => (0..=s.len()).any(|i| s[..i].iter().all(|c| *c != '/') && glob(&pat[1..], &s[i..])),
        Some('?') => !s.is_empty() && s[0] != '/' && glob(&pat[1..], &s[1..]),
        Some(c) => s.first() == Some(c) && glob(&pat[1..], &s[1..]),
    }
}

/// `path` 是否命中 `GOPRIVATE` 风格的逗号分隔模式(与 Go 一致:模式匹配的是路径的前缀,按 `/` 分段数对齐)。
pub fn is_private(path: &str, patterns: &str) -> bool {
    patterns.split(',').map(str::trim).filter(|p| !p.is_empty()).any(|pat| {
        let n = pat.split('/').count();
        let prefix: String = path.splitn(n + 1, '/').take(n).collect::<Vec<_>>().join("/");
        glob(&pat.chars().collect::<Vec<_>>(), &prefix.chars().collect::<Vec<_>>())
    })
}

/// 从 `file` 所在目录向上找 `go.mod`,返回它声明的模块路径。
pub fn module_path(file: &Path) -> Option<String> {
    for dir in file.ancestors().skip(1) {
        if let Ok(text) = std::fs::read_to_string(dir.join("go.mod")) {
            return text.lines().find_map(|l| {
                let rest = l.trim().strip_prefix("module")?;
                let m = rest.trim().trim_matches('"');
                (!m.is_empty() && rest.starts_with(char::is_whitespace)).then(|| m.to_string())
            });
        }
    }
    None
}

/// 执行命令并带超时取标准输出(超时就杀掉)。
fn run_timeout(mut cmd: Command, limit: Duration) -> Option<String> {
    let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let mut out = child.stdout.take()?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut s = String::new();
        let _ = std::io::Read::read_to_string(&mut out, &mut s);
        let _ = tx.send(s);
    });
    match rx.recv_timeout(limit) {
        Ok(s) => {
            let _ = child.wait();
            Some(s)
        }
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            None
        }
    }
}

/// 用户的 `GOPRIVATE`:先看本进程环境,再问 `go env`(读得到 `go env -w` 写的值),最后问登录 shell(读得到 `.zshrc` 里导出的值)。结果缓存。
pub fn private_patterns() -> &'static str {
    static CACHE: OnceLock<String> = OnceLock::new();
    CACHE.get_or_init(|| {
        let from_env = std::env::var("GOPRIVATE").unwrap_or_default();
        if !from_env.trim().is_empty() {
            return from_env;
        }
        let mut go = Command::new("go");
        go.args(["env", "GOPRIVATE"]).env("PATH", crate::servers::child_path());
        if let Some(v) = run_timeout(go, Duration::from_secs(3)).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
            return v;
        }
        // 交互式登录 shell 会加载 .zshrc;加标记行,免得把 shell 启动时打印的杂项当成值
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
        let mut sh = Command::new(shell);
        sh.args(["-ilc", "printf '<<GOPRIVATE>>%s<<END>>' \"$GOPRIVATE\""]);
        let out = run_timeout(sh, Duration::from_secs(3)).unwrap_or_default();
        let v = out.split("<<GOPRIVATE>>").nth(1).and_then(|r| r.split("<<END>>").next()).unwrap_or("").trim().to_string();
        loom_diag::info!("从登录 shell 取得 GOPRIVATE:{}", if v.is_empty() { "(空)" } else { &v });
        v
    })
}

/// 链接指向的 Go 包路径(去掉站点前缀、`@版本`、`#锚点`)。
fn link_pkg_path(url: &str) -> Option<String> {
    let rest = url.strip_prefix(PKG_SITE)?;
    let rest = rest.split(['#', '?']).next().unwrap_or(rest);
    // `modpath@v1.2.3/sub` → `modpath/sub`
    let cleaned: Vec<String> = rest.split('/').map(|seg| seg.split('@').next().unwrap_or(seg).to_string()).collect();
    Some(cleaned.join("/"))
}

/// 去掉悬停里指向私有模块的 `pkg.go.dev` 链接行。`file` 用来找工作区模块路径,`patterns` 是 `GOPRIVATE` 风格的模式。
pub fn strip_private_links(md: &str, file: &Path, patterns: &str) -> String {
    if !md.contains(PKG_SITE) {
        return md.to_string();
    }
    let module = module_path(file);
    let private = |p: &str| {
        is_private(p, patterns) || module.as_deref().is_some_and(|m| p == m || p.strip_prefix(m).is_some_and(|r| r.starts_with('/')))
    };
    let mut kept: Vec<&str> = Vec::new();
    for line in md.lines() {
        let t = line.trim();
        // 整行就是一个链接 `[文字](https://pkg.go.dev/…)`
        let whole_link = t.starts_with('[') && t.ends_with(')') && t.matches("](").count() == 1;
        let url = whole_link.then(|| t.split_once("](").map(|(_, u)| u.trim_end_matches(')'))).flatten();
        if let Some(path) = url.and_then(link_pkg_path) && private(&path) {
            continue;
        }
        kept.push(line);
    }
    // 链接行前的分隔线 `---` 与空行一并收掉
    while kept.last().is_some_and(|l| l.trim().is_empty() || l.trim() == "---") {
        kept.pop();
    }
    kept.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn goprivate_按前缀分段匹配() {
        assert!(is_private("git.corp.com/team/repo/pkg", "git.corp.com"));
        assert!(is_private("git.corp.com/team/repo", "git.corp.com/team"));
        assert!(is_private("git.corp.com/a", "*.corp.com,foo.org"));
        assert!(!is_private("github.com/gin-gonic/gin", "git.corp.com,*.corp.com"));
        assert!(!is_private("git.corp.com.evil/x", "git.corp.com"));
        assert!(!is_private("x/y", ""));
    }

    #[test]
    fn 链接路径去掉版本与锚点() {
        assert_eq!(link_pkg_path("https://pkg.go.dev/git.corp.com/a/b#Recent").unwrap(), "git.corp.com/a/b");
        assert_eq!(link_pkg_path("https://pkg.go.dev/github.com/x/y@v1.2.3/sub#F").unwrap(), "github.com/x/y/sub");
        assert!(link_pkg_path("https://example.com/").is_none());
    }

    #[test]
    fn 私有包链接被去掉_公开的保留() {
        let dir = std::env::temp_dir().join(format!("loom-golinks-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("go.mod"), "module git.corp.com/team/bw-go\n\ngo 1.22\n").unwrap();
        let file = dir.join("sub/a.go");
        let priv_md = "```go\nfunc F()\n```\n\n文档\n\n---\n\n[`a.F` on pkg.go.dev](https://pkg.go.dev/git.corp.com/team/bw-go/sub#F)";
        // 工作区自己的模块:不需要 GOPRIVATE 也过滤
        assert_eq!(strip_private_links(priv_md, &file, ""), "```go\nfunc F()\n```\n\n文档");
        // GOPRIVATE 命中的别的私有模块
        let other = "文档\n\n---\n\n[`b.G` on pkg.go.dev](https://pkg.go.dev/git.corp.com/other/lib#G)";
        assert_eq!(strip_private_links(other, &file, "git.corp.com"), "文档");
        // 公开依赖保留
        let public = "文档\n\n---\n\n[`gin.New` on pkg.go.dev](https://pkg.go.dev/github.com/gin-gonic/gin#New)";
        assert_eq!(strip_private_links(public, &file, "git.corp.com"), public);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
