//! 切换「当前服务商」(Claude 官方也算一个):写回 providers.json,并让已打开的空闲会话换上新环境变量。
//!
//! 环境变量只能在启动 claude 时注入,所以给会话发 `SetEnv`:惰性后端结束当前进程,下一条提示用新环境 `--resume` 继续,
//! 时间线不动。回合进行中的会话不打断,等它下次重启时再生效。
//!
//! @author ixxxxoooo

use crate::workbench::Workbench;
use gpui_kit::*;
use loom_config::providers::{self, ProviderStore};
use loom_core::{AgentCommand, NoticeLevel, Phase};
use std::collections::HashMap;

impl Workbench {
    /// 各服务商的 API Key:设置页打开过就用页里的,否则读磁盘(按修改时间缓存;**不能调用 `forms()`**)。
    pub fn provider_keys(&self) -> HashMap<String, String> {
        if let Some(f) = self.settings_forms.as_ref() {
            return f.providers.keys.clone();
        }
        thread_local! {
            static CACHE: std::cell::RefCell<Option<(Option<std::time::SystemTime>, HashMap<String, String>)>> = const { std::cell::RefCell::new(None) };
        }
        let path = self.provider_keys_path();
        let mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        CACHE.with(|c| {
            let mut c = c.borrow_mut();
            match c.as_ref() {
                Some((t, keys)) if *t == mtime => keys.clone(),
                _ => {
                    let keys = providers::load_keys(&path);
                    *c = Some((mtime, keys.clone()));
                    keys
                }
            }
        })
    }

    /// 让映射指向某个服务商(及第三方的模型)并写盘;设置页开着时走它的表单,否则直接读写文件。成功返回 true。
    fn activate_persisted(&mut self, provider: Option<&str>, third: Option<&str>, cx: &mut Context<Self>) -> bool {
        if self.settings_forms.is_some() {
            self.forms_mut().providers.store.activate(provider, third);
            self.persist_providers(false, cx);
            true
        } else {
            let path = self.providers_path();
            let mut store = ProviderStore::load(&path);
            store.activate(provider, third);
            match store.save(&path) {
                Ok(()) => true,
                Err(e) => {
                    loom_diag::error!("保存服务商配置失败:{e}");
                    false
                }
            }
        }
    }

    /// 记住在输入框里选的模型(不动会话):官方记进偏好的默认模型,第三方记进服务商映射。之后新开会话、重启应用都用它。
    pub fn remember_model(&mut self, model: &str, cx: &mut Context<Self>) {
        match self.mapping_store().active_id().map(str::to_string) {
            None => {
                let m = model.to_string();
                self.update_settings(cx, |s| s.default_model = m);
            }
            Some(p) => {
                self.activate_persisted(Some(&p), Some(model), cx);
            }
        }
    }

    /// 切换当前服务商并选定模型。
    ///
    /// 参数:`provider` 目标服务商 id(`None` 为 Claude 官方);`model` 选中的模型(官方为别名,如 `sonnet`;第三方为真实模型名),
    /// `None` 时官方用 `default`、第三方用它的「默认」槽位。
    pub fn switch_provider(&mut self, provider: Option<String>, model: Option<String>, cx: &mut Context<Self>) {
        loom_diag::crumb!("切换服务商 {provider:?} · {model:?}");
        let third = provider.as_deref().and(model.as_deref());
        if !self.activate_persisted(provider.as_deref(), third, cx) {
            return;
        }
        // 官方的模型选择记进偏好,新会话与重启后沿用(第三方的选择已记在服务商映射里)
        if provider.is_none()
            && let Some(m) = model.clone()
        {
            self.update_settings(cx, |s| s.default_model = m);
        }
        let store = self.mapping_store();
        let model = model.unwrap_or_else(|| if store.active_id().is_some() { store.mapping.default.clone() } else { "default".into() });
        let label = match store.active_name() {
            Some(name) => format!("{name} · {model}"),
            None => format!("Claude 官方 · {model}"),
        };
        let env = self.session_extra_env();
        let active = self.active.clone();
        for (id, v) in &self.views {
            let is_active = active.as_deref() == Some(id.as_str());
            v.model.update(cx, |m, cx| {
                // 回合进行中的其它会话不打断(当前会话的入口在进行中是禁用的)
                if m.state.phase == Phase::Running && !is_active {
                    return;
                }
                m.handle.send(AgentCommand::SetEnv(env.clone()));
                if is_active {
                    m.set_model(model.clone(), cx);
                    m.local_notice(NoticeLevel::Info, format!("已切换到 {label},下一条消息起生效(claude 会用新服务商恢复当前会话)"), cx);
                }
            });
        }
        cx.notify();
    }
}
