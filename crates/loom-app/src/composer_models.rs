//! 输入框的模型选择器与模型映射的衔接:启用映射后,选择器显示的是映射到的真实模型(如 deepseek-chat),不是 claude 的别名。
//!
//! @author ixxxxoooo

use gpui_kit::*;
use loom_config::providers::ProviderStore;

/// 当前已启用的模型映射(没有启用 / 没有工作台返回 `None`)。
pub fn mapped(cx: &App) -> Option<ProviderStore> {
    let wb = cx.try_global::<crate::WorkbenchRef>()?.0.upgrade()?;
    let store = wb.read(cx).forms().providers.store.clone();
    store.mapping.enabled.then_some(store)
}
