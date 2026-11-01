//! Agent 与模型页:当前配置层的 model、扩展思考、思考预算、自动压缩、输出风格、apiKeyHelper。
//! 文本框失焦或回车即写入当前配置层,不需要「保存」按钮。
//!
//! @author ixxxxoooo

use super::widgets::{error_banner, group, note, page_header, row, rows, scope_header};
use crate::ui::pill;
use crate::workbench::Workbench;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::Input;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::*;

/// 一行「文本框」设置:失焦或回车自动提交到 `field` 对应的写入位置。
///
/// 参数:`title` / `desc` 说明;`field` 字段键(见 `layer_actions::CLAUDE_FIELDS`)。
pub fn text_row(wb: &Workbench, title: &str, desc: &str, field: &'static str, cx: &App) -> Div {
    row(title, desc, div().w(px(280.)).child(Input::new(wb.field(field)).small()), cx)
}

/// 当前生效的 API 提供方(按各层 env 判断)。
fn api_provider(wb: &Workbench) -> &'static str {
    let names: Vec<String> = wb.claude_cfg.env_names.clone();
    let has = |k: &str| names.iter().any(|n| n == k) || std::env::var_os(k).is_some();
    if wb.provider_mapping_active() {
        "LoomCode 模型服务商(见「模型服务商」页)"
    } else if has("CLAUDE_CODE_USE_BEDROCK") {
        "Amazon Bedrock"
    } else if has("CLAUDE_CODE_USE_VERTEX") {
        "Google Vertex AI"
    } else if has("CLAUDE_CODE_USE_FOUNDRY") {
        "Microsoft Foundry"
    } else if has("ANTHROPIC_BASE_URL") {
        "自定义 ANTHROPIC_BASE_URL"
    } else {
        "Anthropic(登录账号或 API Key)"
    }
}

/// 渲染 Agent 与模型页。
pub fn render(wb: &Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    let t = cx.theme().clone();
    let scope = wb.effective_scope();
    let layer = wb.claude_cfg.layer(scope);
    let effective_model = wb.claude_cfg.default_model.clone().unwrap_or_else(|| "默认".into());
    let target = match wb.scope_settings_file(scope) {
        Some(p) => format!("写入 {}", p.display()),
        None => "先打开一个项目会话".into(),
    };
    let presets = ["opus", "sonnet", "haiku", "opusplan"];
    v_flex().w_full()
        .child(page_header("Agent 与模型", "Claude Code 自己的设置(settings.json),对终端里的 claude 同样生效;新会话的默认模型 / 模式在「对话」页", div(), cx))
        .child(scope_header(wb, target, cx))
        .children(error_banner(wb, cx))
        .child(group(&format!("{}层", scope.label()), rows![
            text_row(wb, "默认模型", &format!("model;各层合并后当前生效:{effective_model}"), "model", cx),
            row("常用模型", "点一下填入并保存", h_flex().gap_1().children(presets.into_iter().map(|p| {
                Button::new(SharedString::from(format!("model-preset-{p}"))).xsmall().ghost().label(p)
                    .on_click(cx.listener(move |this, _, window, cx| { this.set_field("model", p.to_string(), window, cx); this.commit_field("model", cx) }))
            })), cx),
            row("默认开启扩展思考", "alwaysThinkingEnabled", Switch::new("sw-think-always").checked(layer.always_thinking.unwrap_or(false))
                .on_click(cx.listener(|this, c: &bool, _, cx| this.set_layer_bool(&["alwaysThinkingEnabled"], Some(*c), cx))), cx),
            text_row(wb, "思考预算(tokens)", "env MAX_THINKING_TOKENS", "thinking", cx),
            text_row(wb, "自动压缩阈值(%)", "env CLAUDE_AUTOCOMPACT_PCT_OVERRIDE:上下文用到多少时自动压缩", "compact", cx),
            text_row(wb, "输出风格", "outputStyle;内置 Default / Explanatory / Learning,也可以是 ~/.claude/output-styles 里的自定义风格", "output_style", cx),
        ], cx))
        .child(note("文本框失焦或按回车自动保存;留空表示删除该键、继承其它层", cx))
        .child(group("API", rows![
            row("当前 API 提供方", "按环境变量判断", pill(api_provider(wb), t.foreground, t.muted), cx),
            text_row(wb, "apiKeyHelper", "运行脚本获取 API Key(适合密钥放在钥匙串或密码管理器里)", "key_helper", cx),
        ], cx))
        .into_any_element()
}
