//! MCP 页的 Workbench 方法:打开表单(新增 / 编辑 / 粘贴)、保存、删除、启停。
//!
//! @author ixxxxoooo

use crate::workbench::Workbench;
use gpui_kit::*;
use loom_config::{McpServer, McpSpec};

impl Workbench {
    /// 打开 MCP 表单。
    ///
    /// 参数:`server` 要编辑的服务器(新增时为 `None`);`paste` 是否进入粘贴 JSON 模式。
    pub fn open_mcp_form(&mut self, server: Option<&McpServer>, paste: bool, window: &mut Window, cx: &mut Context<Self>) {
        let f = &mut self.forms_mut().mcp;
        f.open = true;
        f.paste = paste;
        f.editing = server.map(|s| (s.scope, s.name.clone()));
        f.transport = match server.map(|s| s.spec.transport.as_str()) {
            Some("http") => "http",
            Some("sse") => "sse",
            _ => "stdio",
        };
        let spec = server.map(|s| s.spec.clone()).unwrap_or_default();
        let values = [
            (f.name.clone(), server.map(|s| s.name.clone()).unwrap_or_default()),
            (f.command.clone(), spec.command.clone()),
            (f.args.clone(), spec.args.join(" ")),
            (f.url.clone(), spec.url.clone()),
        ];
        let json = f.json.clone();
        if let Some(s) = server {
            // 编辑时配置层固定为服务器所在层
            self.cfg_scope = s.scope;
        }
        for (input, v) in values {
            input.update(cx, |i, cx| i.set_value(v, window, cx));
        }
        if paste {
            json.update(cx, |i, cx| i.set_value(String::new(), window, cx));
        }
        self.forms_mut().error = None;
        cx.notify();
    }

    /// 收起 MCP 表单。
    pub fn close_mcp_form(&mut self, cx: &mut Context<Self>) {
        self.forms_mut().mcp.open = false;
        cx.notify();
    }

    /// 保存表单:粘贴模式逐个写入,表单模式新增或修改。
    pub fn save_mcp_form(&mut self, cx: &mut Context<Self>) {
        let scope = self.effective_scope();
        let project = self.cfg_project().map(|p| p.to_path_buf());
        let home = self.claude_home.clone();
        let f = &self.forms().mcp;
        let result = if f.paste {
            let text = f.json.read(cx).value().to_string();
            match loom_config::claude::parse_mcp_paste(&text) {
                Ok(list) => list.into_iter().try_for_each(|(name, v)| loom_config::claude::upsert_mcp_raw(&home, project.as_deref(), scope, &name, v)),
                Err(msg) => Err(std::io::Error::other(msg)),
            }
        } else {
            let name = f.name.read(cx).value().trim().to_string();
            let spec = McpSpec {
                transport: f.transport.to_string(),
                command: f.command.read(cx).value().trim().to_string(),
                args: f.args.read(cx).value().split_whitespace().map(str::to_string).collect(),
                url: f.url.read(cx).value().trim().to_string(),
                ..Default::default()
            };
            let old = f.editing.clone();
            match old {
                // 编辑时如果换了层:先写新层,再删旧层
                Some((old_scope, old_name)) if old_scope != scope => loom_config::claude::upsert_mcp(&home, project.as_deref(), scope, None, &name, &spec)
                    .and_then(|_| loom_config::claude::remove_mcp(&home, project.as_deref(), old_scope, &old_name)),
                Some((_, old_name)) => loom_config::claude::upsert_mcp(&home, project.as_deref(), scope, Some(&old_name), &name, &spec),
                None => loom_config::claude::upsert_mcp(&home, project.as_deref(), scope, None, &name, &spec),
            }
        };
        if self.finish_cfg_write(result, cx) {
            self.close_mcp_form(cx);
        }
    }

    /// 删除服务器(写回它所在的层)。
    pub fn delete_mcp(&mut self, server: &McpServer, cx: &mut Context<Self>) {
        let result = loom_config::claude::remove_mcp(&self.claude_home, self.cfg_project(), server.scope, &server.name);
        self.finish_cfg_write(result, cx);
    }

    /// 启用 / 禁用服务器;项目层服务器启用即批准。
    pub fn toggle_mcp(&mut self, server: &McpServer, enabled: bool, cx: &mut Context<Self>) {
        let result = loom_config::claude::set_mcp_enabled(&self.claude_home, self.cfg_project(), server.scope, &server.name, enabled);
        self.finish_cfg_write(result, cx);
    }
}

impl Workbench {
    /// 展开 / 收起一个服务器的详情。
    pub fn toggle_mcp_open(&mut self, name: &str, cx: &mut Context<Self>) {
        let open = &mut self.forms_mut().mcp_open;
        *open = if open.as_deref() == Some(name) { None } else { Some(name.to_string()) };
        cx.notify();
    }

    /// 切换范围筛选(`None` 为全部)。
    pub fn set_mcp_filter(&mut self, scope: Option<loom_config::Scope>, cx: &mut Context<Self>) {
        self.forms_mut().mcp_filter = scope;
        cx.notify();
    }

    /// 检测连接:后台运行 `claude mcp list`(最多 90 秒),按名称记下每个服务器的状态。
    /// 演示模式不运行命令,给出一组示例状态。
    pub fn check_mcp_health(&mut self, cx: &mut Context<Self>) {
        use loom_config::claude::Health;
        if self.forms().mcp_checking {
            return;
        }
        if self.demo {
            // 演示:模拟 1.5 秒的检测过程,方便看到转圈动画
            self.forms_mut().mcp_checking = true;
            cx.notify();
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(std::time::Duration::from_millis(1500)).await;
                let _ = this.update(cx, |this, cx| {
                    let f = this.forms_mut();
                    f.mcp_checking = false;
                    f.mcp_health.clear();
                    f.mcp_health.insert("filesystem".into(), (Health::Connected, "Connected".into()));
                    f.mcp_health.insert("github".into(), (Health::NeedsAuth, "Needs authentication".into()));
                    f.mcp_health.insert("playwright".into(), (Health::Failed, "Failed to connect".into()));
                    cx.notify();
                });
            })
            .detach();
            return;
        }
        let Some(bin) = loom_claude::options::find_claude(self.settings.claude_path.as_deref()) else {
            self.forms_mut().error = Some("未找到 claude 命令,可以在「高级」页指定路径".into());
            cx.notify();
            return;
        };
        self.forms_mut().mcp_checking = true;
        self.forms_mut().error = None;
        let (cwd, env) = (self.claude_cfg_root.clone(), self.settings.session_env());
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx.background_spawn(async move { super::cli::run(&bin, &["mcp".to_string(), "list".to_string()], cwd.as_deref(), env, 90) }).await;
            let _ = this.update(cx, |this, cx| {
                this.forms_mut().mcp_checking = false;
                match result {
                    Ok(text) => {
                        this.forms_mut().mcp_health = loom_config::claude::parse_mcp_list(&text).into_iter().map(|(n, h, d)| (n, (h, d))).collect();
                    }
                    Err(e) => {
                        loom_config::diag::error(&format!("claude mcp list 失败:{e}"));
                        this.forms_mut().error = Some(format!("检测失败:{e}"));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 从 Claude Desktop 导入服务器到用户层(Desktop 的配置本来就是全局的)。
    pub fn import_mcp_desktop(&mut self, cx: &mut Context<Self>) {
        let scope = loom_config::Scope::User;
        let desktop = loom_config::claude::desktop_config_path(&std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default()));
        let result = loom_config::claude::import_from_desktop(&desktop, &self.claude_home, self.cfg_project(), scope);
        match result {
            Ok(0) => self.forms_mut().error = Some("Claude Desktop 里没有可导入的 MCP 服务器".into()),
            Ok(_) => self.forms_mut().error = None,
            Err(e) => self.forms_mut().error = Some(format!("导入失败:{e}")),
        }
        self.reload_claude_cfg();
        cx.notify();
    }
}
