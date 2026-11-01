//! 插件页的 Workbench 方法:刷新列表、启停(写 `enabledPlugins`)、通过 claude CLI 安装 / 更新 / 卸载插件与管理市场。
//! CLI 在后台线程运行(最多 3 分钟),输出去掉颜色后显示在页面上;演示模式不运行 CLI。
//!
//! @author ixxxxoooo

use super::ops_actions::strip_ansi;
use crate::workbench::Workbench;
use gpui_kit::*;
use loom_config::Scope;
use loom_config::plugins::{self, PluginEntry};
use std::time::Duration;

impl Workbench {
    /// 重新读取已安装插件与市场。
    pub fn refresh_plugins(&mut self) {
        let installed = plugins::list_installed(&self.claude_home, self.cfg_project());
        let markets = plugins::list_markets(&self.claude_home);
        if let Some(f) = self.settings_forms.as_mut() {
            f.plugins.installed = installed;
            f.plugins.markets = markets;
        }
    }

    /// 启用 / 禁用一个插件(写它所在层的 `enabledPlugins`,对新会话生效)。
    pub fn toggle_plugin(&mut self, p: &PluginEntry, on: bool, cx: &mut Context<Self>) {
        let result = plugins::set_plugin_enabled(&self.claude_home, self.cfg_project(), p.scope, &p.id, on);
        self.forms_mut().error = result.err().map(|e| format!("写入失败:{e}"));
        self.refresh_plugins();
        cx.notify();
    }

    /// 选择安装层;没有项目时只能装到用户层。
    pub fn plugin_install_scope(&self) -> Scope {
        match self.forms().plugins.install_scope {
            Some(s) if !s.needs_project() || self.claude_cfg_root.is_some() => s,
            _ => Scope::User,
        }
    }

    /// 后台运行 `claude <args>`,结束后刷新列表并显示输出。
    ///
    /// 参数:`label` 进行中的说明;`args` 命令参数(已校验);`cwd` 工作目录(项目层 / 本地层安装需要在项目里运行)。
    fn run_claude(&mut self, label: String, args: Vec<String>, cwd: Option<String>, cx: &mut Context<Self>) {
        if self.demo {
            self.forms_mut().plugins.message = Some((false, "演示模式不会运行 claude 命令".into()));
            cx.notify();
            return;
        }
        if self.forms().plugins.busy.is_some() {
            return;
        }
        let Some(bin) = loom_claude::options::find_claude(self.settings.claude_path.as_deref()) else {
            self.forms_mut().plugins.message = Some((false, "未找到 claude 命令,可以在「高级」页指定路径".into()));
            cx.notify();
            return;
        };
        loom_config::diag::info(&format!("运行 claude {}", args.join(" ")));
        self.forms_mut().plugins.busy = Some(label);
        self.forms_mut().plugins.message = None;
        cx.notify();
        let extra_env = self.settings.session_env();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let mut cmd = std::process::Command::new(&bin);
                    cmd.args(&args).envs(extra_env).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
                    if let Some(dir) = cwd.filter(|d| std::path::Path::new(d).is_dir()) {
                        cmd.current_dir(dir);
                    }
                    let mut child = cmd.spawn().map_err(|e| format!("无法运行 {}:{e}", bin.display()))?;
                    for _ in 0..1800 {
                        if child.try_wait().ok().flatten().is_some() {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(100));
                    }
                    let timed_out = child.try_wait().ok().flatten().is_none();
                    if timed_out {
                        let _ = child.kill();
                    }
                    let out = child.wait_with_output().map_err(|e| format!("读取输出失败:{e}"))?;
                    let text = strip_ansi(&format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)));
                    let text = loom_config::diag::redact_home(text.trim());
                    if timed_out { Err(format!("超过 3 分钟未完成,已终止。{text}")) } else if out.status.success() { Ok(text) } else { Err(text) }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                let message = match result {
                    Ok(t) => (true, if t.is_empty() { "完成".to_string() } else { t }),
                    Err(e) => {
                        loom_config::diag::error(&format!("claude 插件命令失败:{e}"));
                        (false, if e.is_empty() { "命令失败,没有输出".to_string() } else { e })
                    }
                };
                this.forms_mut().plugins.busy = None;
                this.forms_mut().plugins.message = Some(message);
                this.refresh_plugins();
                cx.notify();
            });
        })
        .detach();
    }

    /// 安装输入框里的插件(`名称@市场`)到所选层。
    pub fn install_plugin(&mut self, cx: &mut Context<Self>) {
        let id = self.field_value("plugin_id", cx);
        let scope = self.plugin_install_scope();
        let Some(args) = plugins::plugin_args("install", &id, scope) else {
            self.forms_mut().plugins.message = Some((false, "插件标识格式不对,应为「名称」或「名称@市场」,不能以 - 开头、不含空格".into()));
            cx.notify();
            return;
        };
        self.run_claude(format!("正在安装 {id}…"), args, self.claude_cfg_root.clone(), cx);
    }

    /// 更新或卸载一个已安装插件(`action` 为 `update` / `uninstall`)。
    pub fn plugin_action(&mut self, action: &'static str, p: &PluginEntry, cx: &mut Context<Self>) {
        let Some(args) = plugins::plugin_args(action, &p.id, p.scope) else { return };
        let verb = if action == "update" { "更新" } else { "卸载" };
        self.run_claude(format!("正在{verb} {}…", p.id), args, self.claude_cfg_root.clone(), cx);
    }

    /// 添加市场(输入框)、更新或移除市场(`action` 为 `add` / `update` / `remove`;`name` 为空且 `update` 表示全部)。
    pub fn market_action(&mut self, action: &'static str, name: &str, cx: &mut Context<Self>) {
        let arg = if action == "add" { self.field_value("market_src", cx) } else { name.to_string() };
        let Some(args) = plugins::market_args(action, &arg) else {
            self.forms_mut().plugins.message = Some((false, "格式不对:来源应为 owner/repo、Git URL 或本地路径,不能以 - 开头、不含空格".into()));
            cx.notify();
            return;
        };
        let label = match action {
            "add" => format!("正在添加市场 {arg}…"),
            "remove" => format!("正在移除市场 {arg}…"),
            _ if arg.is_empty() => "正在更新全部市场…".to_string(),
            _ => format!("正在更新市场 {arg}…"),
        };
        self.run_claude(label, args, None, cx);
    }
}
