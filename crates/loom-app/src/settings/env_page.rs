//! 环境变量页:当前配置层 settings.json 的 `env`。值一律遮盖,只有白名单里的开关 / 数值显示原值。
//!
//! @author ixxxxoooo

use super::widgets::{error_banner, page_title, scope_header, section_title};
use crate::ui::{MONO, caption, icon};
use crate::workbench::Workbench;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::Input;
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::*;

/// 渲染环境变量页。
pub fn render(wb: &Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    let t = cx.theme().clone();
    let scope = wb.effective_scope();
    let layer = wb.claude_cfg.layer(scope);
    let target = match wb.scope_settings_file(scope) {
        Some(p) => format!("写入 {} 的 env", p.display()),
        None => "先打开一个项目会话".into(),
    };
    let names = layer.env_names.clone();
    v_flex().w_full()
        .child(page_title("环境变量"))
        .child(caption("Claude Code 启动时把这些变量注入每个会话(包括终端里的 claude)。为避免泄露,值保存后不再显示,只有开关 / 数值类变量显示原值;要修改就用同名变量重新保存", cx))
        .child(scope_header(wb, target, cx))
        .children(error_banner(wb, cx))
        .child(section_title(&format!("{}层({})", scope.label(), names.len()), cx))
        .children((names.is_empty()).then(|| caption("这一层没有环境变量", cx)))
        .child(v_flex().w_full().gap_1().children(names.into_iter().enumerate().map(|(i, name)| {
            let shown = layer.env(&name).map(str::to_string).unwrap_or_else(|| "••••••".into());
            let key = name.clone();
            h_flex().w_full().h(px(32.)).px_2().gap_2().items_center().rounded(px(6.)).border_1().border_color(t.border)
                .child(icon(IconName::Variable, 13., t.muted_foreground))
                .child(div().w(px(260.)).flex_none().truncate().font_family(MONO).text_size(px(12.5)).child(name))
                .child(div().flex_1().min_w_0().truncate().font_family(MONO).text_size(px(12.)).text_color(t.muted_foreground).child(shown))
                .child(Button::new(SharedString::from(format!("env-del-{i}"))).xsmall().ghost().icon(IconName::Trash)
                    .on_click(cx.listener(move |this, _, _, cx| this.remove_env(&key, cx))))
        })))
        .child(section_title("新增或覆盖", cx))
        .child(h_flex().w_full().gap_2()
            .child(div().w(px(220.)).child(Input::new(wb.field("env_key")).small()))
            .child(div().flex_1().child(Input::new(wb.field("env_value")).small()))
            .child(Button::new("env-add").small().primary().label("保存").on_click(cx.listener(|this, _, window, cx| this.add_env(window, cx)))))
        .child(div().pt_2().child(caption("常用:HTTPS_PROXY、ANTHROPIC_BASE_URL、ANTHROPIC_MODEL、BASH_DEFAULT_TIMEOUT_MS、DISABLE_AUTOUPDATER", cx)))
        .into_any_element()
}
