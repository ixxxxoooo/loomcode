//! 输入框的模型选择器与模型服务商的衔接:选择器按服务商分组(Claude 官方在最前),选别家的模型即切换服务商;
//! 芯片显示「服务商 · 真实模型名」。
//!
//! @author ixxxxoooo

use gpui_kit::*;
use loom_config::providers::{PickerGroup, ProviderStore};
use loom_core::{ModelInfo, SessionState};

/// 模型弹层的数据。
pub struct Picker {
    /// 当前服务商 id(`None` 为 Claude 官方)
    pub active: Option<String>,
    /// Claude 官方分组的模型
    pub official: Vec<ModelInfo>,
    /// 第三方服务商分组
    pub groups: Vec<PickerGroup>,
    /// 当前选中的模型(官方为别名 / id,第三方为真实模型名)
    pub current: String,
}

/// 当前服务商配置(没有工作台时返回 `None`)。
fn store(cx: &App) -> Option<ProviderStore> {
    let wb = cx.try_global::<crate::WorkbenchRef>()?.0.upgrade()?;
    // 不能用 `forms()`:设置页没打开过时表单还没创建,会 panic(启动即崩溃过)
    Some(wb.read(cx).mapping_store())
}

/// Claude 官方的模型别名(正在用第三方服务时 claude 报的列表是映射后的,不能用来展示官方)。
fn official_aliases() -> Vec<ModelInfo> {
    [("default", "默认(推荐)", "Claude 官方"), ("opus", "Opus", "最强"), ("sonnet", "Sonnet", "日常"), ("haiku", "Haiku", "最快")]
        .iter().map(|(id, name, d)| ModelInfo::new(id, name, d)).collect()
}

/// 组装模型弹层的数据。
///
/// 参数:`state` 当前会话状态(官方模型列表与当前模型)。
pub fn picker(state: &SessionState, cx: &App) -> Picker {
    let store = store(cx).unwrap_or_default();
    let keys = cx.try_global::<crate::WorkbenchRef>().and_then(|r| r.0.upgrade()).map(|wb| wb.read(cx).provider_keys()).unwrap_or_default();
    let active = store.active_id().map(str::to_string);
    let (official, current) = match active {
        None if !state.models.is_empty() => (state.models.clone(), state.model.clone()),
        None => (official_aliases(), state.model.clone()),
        Some(_) => (official_aliases(), store.resolve_model(&state.model)),
    };
    Picker { groups: store.picker_groups(&keys), active, official, current }
}

/// 芯片上的文字:第三方服务时「服务商 · 真实模型名」,官方时为模型显示名。
pub fn chip_label(state: &SessionState, cx: &App) -> String {
    let store = store(cx).unwrap_or_default();
    match store.active_name() {
        Some(name) => format!("{name} · {}", store.resolve_model(&state.model)),
        None => state.models.iter().find(|x| x.id == state.model).map(|x| x.name.clone()).unwrap_or_else(|| state.model.clone()),
    }
}

/// 切换到另一个服务商的某个模型(交给工作台写配置、给会话换环境变量)。
pub fn switch(provider: Option<String>, model: String, cx: &mut App) {
    if let Some(wb) = cx.try_global::<crate::WorkbenchRef>().and_then(|r| r.0.upgrade()) {
        wb.update(cx, |wb, cx| wb.switch_provider(provider, Some(model), cx));
    }
}

/// 记住输入框里选的模型,之后新会话与重启都沿用(交给工作台写偏好或服务商映射)。
pub fn remember_model(model: String, cx: &mut App) {
    if let Some(wb) = cx.try_global::<crate::WorkbenchRef>().and_then(|r| r.0.upgrade()) {
        wb.update(cx, |wb, cx| wb.remember_model(&model, cx));
    }
}

/// 记住输入框里选的思考强度,之后新会话与重启都沿用。
pub fn remember_effort(effort: loom_core::Effort, cx: &mut App) {
    if let Some(wb) = cx.try_global::<crate::WorkbenchRef>().and_then(|r| r.0.upgrade()) {
        wb.update(cx, |wb, cx| wb.update_settings(cx, |s| s.default_effort = effort));
    }
}

/// 打开设置 → 模型服务商(选择器里服务商不可用时的入口)。
pub fn open_settings(cx: &mut App) {
    if let Some(wb) = cx.try_global::<crate::WorkbenchRef>().and_then(|r| r.0.upgrade()) {
        wb.update(cx, |wb, cx| {
            wb.select_settings_tab(crate::settings::SettingsTab::Providers, cx);
            // 在发起操作的窗口里打开(独立 IDE 窗口里就在它自己里面)
            let in_ide = wb.ide_window.is_some_and(|h| cx.active_window().is_some_and(|a| a.window_id() == h.window_id()));
            wb.open_settings_in(in_ide, cx);
        });
    }
}

#[cfg(test)]
mod tests {
    /// 回归:输入框每帧都会读服务商配置,设置页没打开过时 `Workbench::forms()` 会 panic(表单尚未创建),
    /// 这里不允许调用它——要用 `mapping_store()` / `provider_keys()`。
    #[::core::prelude::v1::test]
    fn 不调用设置页表单() {
        let src = include_str!("composer_models.rs");
        let code = src.split("#[cfg(test)]").next().unwrap_or("");
        assert!(!code.contains(".forms()"), "composer_models 不能调用 forms()");
        assert!(code.contains("mapping_store()"));
        let sw = include_str!("settings/provider_switch.rs");
        let keys_fn = sw.split("pub fn switch_provider").next().unwrap_or("");
        assert!(!keys_fn.contains(".forms()"), "provider_keys 不能调用 forms()");
    }
}
