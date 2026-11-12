//! 网络 / 用量 / 诊断页的 Workbench 方法:连通性测试、用量扫描、`claude doctor`、进程与日志、导出诊断包。
//! 耗时操作都放后台线程,结果写回 `Forms.ops`。
//!
//! @author ixxxxoooo

use crate::workbench::Workbench;
use gpui_kit::*;
use loom_config::ProxyMode;
use std::time::Duration;

/// 连通性测试目标。
const TARGETS: &[(&str, &str)] = &[
    ("Anthropic API", "https://api.anthropic.com"),
    ("Claude 登录", "https://claude.ai"),
    ("GitHub", "https://github.com"),
    ("npm", "https://registry.npmjs.org"),
    ("Google", "https://www.google.com"),
    ("百度", "https://www.baidu.com"),
];

/// 去掉终端颜色控制符。
pub(super) fn strip_ansi(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            // 跳过 ESC [ ... 字母
            while let Some(&n) = chars.peek() {
                chars.next();
                if n.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

impl Workbench {
    /// 进入某个分类时按需加载数据(只在第一次进入时触发)。
    pub fn enter_tab(&mut self, tab: super::SettingsTab, cx: &mut Context<Self>) {
        use super::SettingsTab::*;
        match tab {
            Usage if self.forms().ops.usage.is_none() && !self.forms().ops.usage_busy => self.refresh_usage(cx),
            Diagnostics => {
                if self.forms().ops.procs.is_empty() && self.forms().ops.log.is_empty() {
                    self.refresh_diag(cx);
                }
                self.start_diag_ticker(cx);
            }
            Trust => self.refresh_trust(cx),
            _ => {}
        }
    }

    /// 重新读取每个项目的信任状态(`~/.claude.json`)。
    pub fn refresh_trust(&mut self, cx: &mut Context<Self>) {
        let paths: Vec<String> = self.projects.iter().map(|p| p.path.clone()).collect();
        let flags = loom_config::claude::read_trust(&self.claude_home, &paths);
        self.forms_mut().ops.trust = self.projects.iter().zip(flags).map(|(p, t)| (p.name.clone(), p.path.clone(), t)).collect();
        cx.notify();
    }

    /// 切换某个项目的信任状态并写回 `~/.claude.json`。
    pub fn set_project_trust(&mut self, path: &str, trusted: bool, cx: &mut Context<Self>) {
        match loom_config::claude::set_trust(&self.claude_home, std::path::Path::new(path), trusted) {
            Ok(()) => self.forms_mut().error = None,
            Err(e) => self.forms_mut().error = Some(format!("写入 ~/.claude.json 失败:{e}")),
        }
        self.refresh_trust(cx);
    }

    /// 当前设置下 curl 应使用的代理。
    fn probe_proxy(&self) -> Option<String> {
        let addr = self.settings.proxy_address.trim();
        match self.settings.proxy_mode {
            ProxyMode::Http if !addr.is_empty() => Some(if addr.contains("://") { addr.into() } else { format!("http://{addr}") }),
            ProxyMode::Socks5 if !addr.is_empty() => Some(if addr.contains("://") { addr.into() } else { format!("socks5h://{addr}") }),
            _ => None,
        }
    }

    /// 并发测试各站点(含已启用映射的服务商地址)。
    pub fn run_connectivity(&mut self, cx: &mut Context<Self>) {
        let mut targets: Vec<(String, String)> = TARGETS.iter().map(|(n, u)| (n.to_string(), u.to_string())).collect();
        let ui = &self.forms().providers;
        if let Some(p) = ui.store.get(&ui.store.mapping.provider) {
            if !p.base_url.is_empty() {
                targets.push((format!("服务商 {}", p.name), p.base_url.clone()));
            }
        }
        self.forms_mut().ops.net = targets.iter().map(|(n, u)| (n.clone(), u.clone(), None)).collect();
        let proxy = self.probe_proxy();
        for (i, (_, url)) in targets.into_iter().enumerate() {
            let proxy = proxy.clone();
            cx.spawn(async move |this, cx| {
                let r = cx.background_spawn(async move { loom_config::diag::probe(&url, proxy.as_deref()) }).await;
                let _ = this.update(cx, |this, cx| {
                    if let Some(slot) = this.forms_mut().ops.net.get_mut(i) {
                        slot.2 = Some(r);
                    }
                    cx.notify();
                });
            })
            .detach();
        }
        cx.notify();
    }

    /// 后台扫描用量(带增量缓存)。
    pub fn refresh_usage(&mut self, cx: &mut Context<Self>) {
        if self.forms().ops.usage_busy {
            return;
        }
        self.forms_mut().ops.usage_busy = true;
        let projects = self.claude_home.join(".claude/projects");
        let cache = self.settings_path.with_file_name("usage-index.json");
        cx.spawn(async move |this, cx| {
            let report = cx.background_spawn(async move { loom_claude::usage::scan(&projects, &cache) }).await;
            let _ = this.update(cx, |this, cx| {
                let ops = &mut this.forms_mut().ops;
                ops.usage = Some(report);
                ops.usage_busy = false;
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// 后台运行 `claude doctor`(最多 30 秒)。
    pub fn run_doctor(&mut self, cx: &mut Context<Self>) {
        let bin = loom_claude::options::find_claude(self.settings.claude_path.as_deref());
        self.forms_mut().ops.doctor_busy = true;
        cx.spawn(async move |this, cx| {
            let out = cx
                .background_spawn(async move {
                    let Some(bin) = bin else { return "未找到 claude 命令".to_string() };
                    let child = std::process::Command::new(&bin).arg("doctor").stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn();
                    let Ok(mut child) = child else { return format!("无法运行 {}", bin.display()) };
                    for _ in 0..300 {
                        if let Ok(Some(_)) = child.try_wait() {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(100));
                    }
                    let _ = child.kill();
                    match child.wait_with_output() {
                        Ok(o) => strip_ansi(&format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))),
                        Err(e) => format!("读取输出失败:{e}"),
                    }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                let ops = &mut this.forms_mut().ops;
                ops.doctor = Some(loom_config::diag::redact_home(out.trim()));
                ops.doctor_busy = false;
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// 刷新进程资源、日志末尾与崩溃记录。
    pub fn refresh_diag(&mut self, cx: &mut Context<Self>) {
        let demo = self.demo;
        let ops = &mut self.forms_mut().ops;
        ops.procs = loom_config::diag::process_stats();
        ops.log = loom_config::diag::tail(200);
        ops.crashes = loom_diag::records::list(30);
        if demo && ops.crashes.is_empty() {
            ops.crashes = super::diag_demo::fake_records();
        }
        cx.notify();
    }

    /// 诊断页打开期间每秒重绘一次(性能曲线实时走),每 10 秒在后台刷新进程与日志;离开页面自动停止。
    pub fn start_diag_ticker(&mut self, cx: &mut Context<Self>) {
        if self.forms().ops.diag_ticking {
            return;
        }
        self.forms_mut().ops.diag_ticking = true;
        cx.spawn(async move |this, cx| {
            let mut n = 0u32;
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                n += 1;
                let refresh = n % 10 == 0;
                let data = if refresh {
                    Some(cx.background_executor().spawn(async { (loom_config::diag::process_stats(), loom_config::diag::tail(200), loom_diag::records::list(30)) }).await)
                } else {
                    None
                };
                let alive = this
                    .update(cx, |this, cx| {
                        let open = (this.page == crate::workbench::Page::Settings || this.ide_settings) && this.settings_tab == super::SettingsTab::Diagnostics;
                        let demo = this.demo;
                        let ops = &mut this.forms_mut().ops;
                        if !open {
                            ops.diag_ticking = false;
                            return false;
                        }
                        if let Some((procs, log, crashes)) = data {
                            ops.procs = procs;
                            ops.log = log;
                            if !(demo && crashes.is_empty()) {
                                ops.crashes = crashes;
                            }
                        }
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if !alive {
                    break;
                }
            }
        })
        .detach();
    }

    /// 导出诊断包到 `~/.loomcode/diagnostics/` 并在 Finder 中显示。不含任何密钥与环境变量值。
    pub fn export_diagnostics(&mut self, cx: &mut Context<Self>) {
        let c = &self.claude_cfg;
        let os = std::process::Command::new("sw_vers").output().map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();
        let ui = &self.forms().providers;
        let mapping = match ui.store.mapping_status(&ui.keys) {
            Ok(()) => format!("生效:{}", ui.store.get(&ui.store.mapping.provider).map(|p| p.name.clone()).unwrap_or_default()),
            Err(e) => e,
        };
        let procs = loom_config::diag::process_stats().iter().map(|p| format!("{} {:.0}MB {:.1}% {}", p.0, p.1, p.2, p.3)).collect::<Vec<_>>().join("\n");
        let sections = [
            ("版本", format!("LoomCode {}\nClaude Code {}\n{os}", env!("CARGO_PKG_VERSION"), self.claude_info.clone().unwrap_or_default())),
            ("运行", format!("模式:{}\n配置:{}\n当前项目:{}", if self.demo { "演示" } else { "真实" }, self.settings_path.display(), self.claude_cfg_root.clone().unwrap_or_else(|| "无".into()))),
            ("Claude 配置概况", format!(
                "权限规则:允许 {} / 询问 {} / 拒绝 {}\n钩子:{} 条\nMCP:{}\n环境变量名:{}",
                c.permissions.allow.len(), c.permissions.ask.len(), c.permissions.deny.len(), c.hooks.len(),
                c.mcp_servers.iter().map(|m| format!("{}({}{})", m.name, m.scope.label(), if m.enabled { "" } else { ",已禁用" })).collect::<Vec<_>>().join("、"),
                c.env_names.join("、"),
            )),
            ("模型映射", mapping),
            ("进程", procs),
            ("性能", perf_report()),
            ("崩溃与卡顿记录", crash_report(&loom_diag::records::list(10))),
            ("最近的操作(面包屑)", loom_diag::crumbs::recent(100).join("\n")),
        ];
        match loom_config::diag::export_bundle(&self.settings_path.with_file_name("diagnostics"), &sections) {
            Ok(path) => {
                let _ = std::process::Command::new("open").arg("-R").arg(&path).spawn();
                self.forms_mut().error = None;
            }
            Err(e) => self.forms_mut().error = Some(format!("导出失败:{e}")),
        }
        cx.notify();
    }
}

/// 诊断包里的性能段:当前读数、构建耗时分位数、实体计数、最近一次卡顿、近 60 秒的内存与 CPU 曲线。
fn perf_report() -> String {
    use loom_diag::{frames, hang, monitor, stats};
    let f = frames::summary();
    let mut out = format!(
        "内存 {} MB;界面构建耗时(最近 {} 次)p50 {:.1} ms / p95 {:.1} ms / 最大 {:.1} ms\n",
        stats::rss_mb(), f.count, f.p50_us as f64 / 1000., f.p95_us as f64 / 1000., f.max_us as f64 / 1000.
    );
    out.push_str(&format!("实体计数:{}\n", stats::gauges().iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(" ")));
    match hang::last_stall() {
        Some((ms, at)) => out.push_str(&format!("最近一次卡顿:{ms} ms(Unix 时间 {at})\n")),
        None => out.push_str("最近一次卡顿:无\n"),
    }
    let s = monitor::samples();
    out.push_str("近 60 秒(Unix 秒 内存MB CPU%):\n");
    for x in s.iter().rev().take(60).rev() {
        out.push_str(&format!("{} {} {:.0}\n", x.t, x.rss_mb, x.cpu));
    }
    out
}

/// 诊断包里的崩溃段:每条一块,含类别、时间、摘要与详情(家目录由导出函数统一脱敏)。
fn crash_report(records: &[loom_diag::CrashRecord]) -> String {
    if records.is_empty() {
        return "没有记录".into();
    }
    records.iter().map(|r| format!("[{}] {} pid={} v{} {} 已运行 {} 秒\n{}\n{}\n", r.kind_label(), r.time, r.pid, r.version, r.mode, r.uptime_s, r.summary, r.detail)).collect::<Vec<_>>().join("\n-----\n")
}

#[cfg(test)]
mod tests {
    use super::strip_ansi;

    #[::core::prelude::v1::test]
    fn 去掉颜色控制符() {
        assert_eq!(strip_ansi("\u{1b}[32m✓\u{1b}[0m ok"), "✓ ok");
    }
}
