//! 输入框的模型选择器与模型映射的衔接:启用映射后,选择器显示的是映射到的真实模型(如 deepseek-chat),不是 claude 的别名。
//!
//! @author ixxxxoooo

use gpui_kit::*;
use loom_config::providers::ProviderStore;

/// 当前已启用的模型映射(没有启用 / 没有工作台返回 `None`)。
pub fn mapped(cx: &App) -> Option<ProviderStore> {
    let wb = cx.try_global::<crate::WorkbenchRef>()?.0.upgrade()?;
    // 不能用 `forms()`:设置页没打开过时表单还没创建,会 panic(启动即崩溃过)
    let store = wb.read(cx).mapping_store();
    store.mapping.enabled.then_some(store)
}

#[cfg(test)]
mod tests {
    /// 回归:输入框每帧都会调 `mapped`,设置页没打开过时 `Workbench::forms()` 会 panic(表单尚未创建),
    /// 这里不允许调用它——要用 `mapping_store()`。
    #[::core::prelude::v1::test]
    fn 不调用设置页表单() {
        let src = include_str!("composer_models.rs");
        let code = src.split("#[cfg(test)]").next().unwrap_or("");
        assert!(!code.contains(".forms()"), "composer_models 不能调用 forms()");
        assert!(code.contains("mapping_store()"));
    }
}
