//! 模型服务商页的 Workbench 方法:增删改服务商、密钥、模型清单、测试连接 / 获取模型(后台线程)、模型映射,
//! 以及新会话启动时要注入的环境变量。
//!
//! @author ixxxxoooo

use crate::workbench::Workbench;
use gpui_kit::*;
use loom_config::providers::{self, AuthHeader, PRESETS, Provider, ProviderStore};
use std::collections::HashMap;
use std::path::PathBuf;

impl Workbench {
    /// `providers.json` 路径(与 settings.json 同目录;演示模式在沙盒里)。
    fn providers_path(&self) -> PathBuf {
        self.settings_path.with_file_name("providers.json")
    }

    /// 密钥文件路径。
    fn provider_keys_path(&self) -> PathBuf {
        self.settings_path.with_file_name("provider-keys.json")
    }

    /// 从磁盘读入服务商与密钥到设置页状态。
    pub fn load_providers(&mut self) {
        let (store, keys) = (ProviderStore::load(&self.providers_path()), providers::load_keys(&self.provider_keys_path()));
        let ui = &mut self.forms_mut().providers;
        ui.selected = ui.selected.take().filter(|id| store.get(id).is_some()).or_else(|| store.providers.first().map(|p| p.id.clone()));
        ui.store = store;
        ui.keys = keys;
    }

    /// 新会话要注入 claude 的环境变量:客户端偏好(代理 / 超时 / CLAUDE.md)+ 模型映射。
    pub fn session_extra_env(&self) -> Vec<(String, String)> {
        let mut env = self.settings.session_env();
        env.extend(ProviderStore::load(&self.providers_path()).mapping_env(&providers::load_keys(&self.provider_keys_path())));
        env
    }

    /// 模型映射是否生效。
    pub fn provider_mapping_active(&self) -> bool {
        match self.settings_forms.as_ref() {
            Some(f) => f.providers.store.mapping_status(&f.providers.keys).is_ok(),
            None => ProviderStore::load(&self.providers_path()).mapping_status(&providers::load_keys(&self.provider_keys_path())).is_ok(),
        }
    }

    /// 写回两个文件并刷新;`keys_changed` 时才写密钥文件。
    fn persist_providers(&mut self, keys_changed: bool, cx: &mut Context<Self>) {
        let (cfg, keys_path) = (self.providers_path(), self.provider_keys_path());
        let ui = &self.forms().providers;
        let mut result = ui.store.save(&cfg);
        if keys_changed && result.is_ok() {
            result = providers::save_keys(&keys_path, &ui.keys);
        }
        self.forms_mut().error = result.err().map(|e| format!("保存失败:{e}"));
        cx.notify();
    }

    /// 选中一个服务商并把它的名称、地址填进表单(密钥不回填)。
    pub fn select_provider(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(p) = self.forms().providers.store.get(id).cloned() else { return };
        self.forms_mut().providers.selected = Some(p.id.clone());
        self.forms_mut().providers.status = None;
        self.set_field("p_name", p.name, window, cx);
        self.set_field("p_url", p.base_url, window, cx);
        self.set_field("p_key", String::new(), window, cx);
        self.set_field("p_model", String::new(), window, cx);
        cx.notify();
    }

    /// 按预设(`None` 为自定义)新增服务商并选中。
    pub fn add_provider(&mut self, preset: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let (name, url, auth, models) = match preset.and_then(|i| PRESETS.get(i)) {
            Some((n, u, a, m)) => (n.to_string(), u.to_string(), *a, m.iter().map(|s| s.to_string()).collect()),
            None => ("自定义服务商".to_string(), String::new(), AuthHeader::Bearer, Vec::new()),
        };
        let store = &mut self.forms_mut().providers.store;
        let id = store.new_id(&name);
        store.providers.push(Provider { id: id.clone(), name, base_url: url, auth, models });
        if store.mapping.provider.is_empty() {
            store.mapping.provider = id.clone();
        }
        self.persist_providers(false, cx);
        self.select_provider(&id, window, cx);
    }

    /// 修改当前服务商:保存名称、地址;密钥框非空时更新密钥。
    pub fn save_provider(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.forms().providers.selected.clone() else { return };
        let (name, url, key) = (self.field_value("p_name", cx), self.field_value("p_url", cx), self.field_value("p_key", cx));
        if !(url.starts_with("https://") || url.starts_with("http://")) {
            self.forms_mut().error = Some("Base URL 需以 https:// 或 http:// 开头".into());
            cx.notify();
            return;
        }
        let ui = &mut self.forms_mut().providers;
        if let Some(p) = ui.store.providers.iter_mut().find(|p| p.id == id) {
            p.name = if name.is_empty() { p.name.clone() } else { name };
            p.base_url = url;
        }
        let keys_changed = !key.is_empty();
        if keys_changed {
            ui.keys.insert(id, key);
        }
        self.persist_providers(keys_changed, cx);
        self.set_field("p_key", String::new(), window, cx);
    }

    /// 删除当前服务商及其密钥;映射指向它时清空映射。
    pub fn delete_provider(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.forms().providers.selected.clone() else { return };
        let ui = &mut self.forms_mut().providers;
        ui.store.providers.retain(|p| p.id != id);
        ui.keys.remove(&id);
        if ui.store.mapping.provider == id {
            ui.store.mapping = Default::default();
        }
        ui.selected = ui.store.providers.first().map(|p| p.id.clone());
        self.persist_providers(true, cx);
    }

    /// 修改当前服务商的属性。
    fn edit_selected(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut Provider)) {
        let Some(id) = self.forms().providers.selected.clone() else { return };
        if let Some(p) = self.forms_mut().providers.store.providers.iter_mut().find(|p| p.id == id) {
            f(p);
        }
        self.persist_providers(false, cx);
    }

    /// 切换鉴权头。
    pub fn set_provider_auth(&mut self, auth: AuthHeader, cx: &mut Context<Self>) {
        self.edit_selected(cx, |p| p.auth = auth);
    }

    /// 把输入框里的模型 ID 加进清单。
    pub fn add_provider_model(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let m = self.field_value("p_model", cx);
        if m.is_empty() {
            return;
        }
        self.edit_selected(cx, |p| if !p.models.contains(&m) { p.models.push(m) });
        self.set_field("p_model", String::new(), window, cx);
    }

    /// 从清单移除模型。
    pub fn remove_provider_model(&mut self, model: &str, cx: &mut Context<Self>) {
        self.edit_selected(cx, |p| p.models.retain(|m| m != model));
    }

    /// 后台测试连接(`fetch` 为真时改为获取模型列表并并入清单)。
    pub fn probe_provider(&mut self, fetch: bool, cx: &mut Context<Self>) {
        let ui = &self.forms().providers;
        let Some(p) = ui.selected.as_ref().and_then(|id| ui.store.get(id)).cloned() else { return };
        let Some(key) = ui.keys.get(&p.id).cloned().filter(|k| !k.is_empty()) else {
            self.forms_mut().providers.status = Some((false, "先填写并保存 API Key".into()));
            cx.notify();
            return;
        };
        let model = ui.store.mapping.default.clone();
        let model = if p.models.contains(&model) { model } else { p.models.first().cloned().unwrap_or_default() };
        self.forms_mut().providers.busy = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let pc = p.clone();
            let result = cx
                .background_spawn(async move {
                    if fetch { providers::fetch_models(&pc, &key).map(Err) } else { providers::test_connection(&pc, &key, &model).map(Ok) }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.forms_mut().providers.busy = false;
                let status = match result {
                    Ok(Ok((ms, reply))) => (true, format!("连接成功,耗时 {ms} ms,回复:{reply}")),
                    Ok(Err(models)) => {
                        let n = models.len();
                        this.edit_selected(cx, |p| for m in models { if !p.models.contains(&m) { p.models.push(m) } });
                        (true, format!("获取到 {n} 个模型,已并入清单"))
                    }
                    Err(e) => (false, e),
                };
                this.forms_mut().providers.status = Some(status);
                cx.notify();
            });
        })
        .detach();
    }

    /// 修改模型映射。
    pub fn edit_mapping(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut loom_config::providers::ModelMapping)) {
        f(&mut self.forms_mut().providers.store.mapping);
        self.persist_providers(false, cx);
    }

    /// 演示与测试用:当前服务商是否已保存密钥。
    pub fn provider_has_key(&self, id: &str) -> bool {
        let keys: &HashMap<String, String> = &self.forms().providers.keys;
        keys.get(id).is_some_and(|k| !k.is_empty())
    }
}
