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

    /// 切换当前服务商并选定模型。
    ///
    /// 参数:`provider` 目标服务商 id(`None` 为 Claude 官方);`model` 选中的模型(官方为别名,如 `sonnet`;第三方为真实模型名),
    /// `None` 时官方用 `default`、第三方用它的「默认」槽位。
    pub fn switch_provider(&mut self, provider: Option<String>, model: Option<String>, cx: &mut Context<Self>) {
        loom_diag::crumb!("切换服务商 {provider:?} · {model:?}");
        let third = provider.as_deref().and(model.as_deref());
        if self.settings_forms.is_some() {
            self.forms_mut().providers.store.activate(provider.as_deref(), third);
            self.persist_providers(false, cx);
        } else {
            let path = self.providers_path();
            let mut store = ProviderStore::load(&path);
            store.activate(provider.as_deref(), third);
            if let Err(e) = store.save(&path) {
                loom_diag::error!("保存服务商配置失败:{e}");
                return;
            }
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
