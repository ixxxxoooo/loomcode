//! 模型与思考页:当前配置层的 model、扩展思考、思考预算、自动压缩、输出风格、apiKeyHelper,
//! 以及客户端的「追加系统提示词」与 CLAUDE.md 开关。
//!
//! @author ixxxxoooo

use super::layer_actions::Target;
use super::widgets::{error_banner, page_title, row, scope_header, section_title};
use crate::ui::{caption, pill};
use crate::workbench::Workbench;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, Textarea};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::*;

/// 一行「文本框 + 保存」设置。
///
/// 参数:`id` 按钮 id;`title` / `desc` 说明;`field` 字段键;`target` 写入位置。
pub fn text_row(wb: &Workbench, id: &'static str, title: &str, desc: &str, field: &'static str, target: Target, cx: &mut Context<Workbench>) -> Div {
    row(title, desc, h_flex().w(px(300.)).gap_2()
        .child(div().flex_1().child(Input::new(wb.field(field)).small()))
        .child(Button::new(id).small().outline().label("保存").on_click(cx.listener(move |this, _, _, cx| this.save_field(field, target, cx)))), cx)
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

/// 渲染模型与思考页。
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
        .child(page_title("模型与思考"))
        .child(caption("这些是 Claude Code 自己的设置(settings.json),对终端里的 claude 同样生效;LoomCode 新会话的默认模型 / 模式在「对话」页", cx))
        .child(scope_header(wb, target, cx))
        .children(error_banner(wb, cx))
        .child(section_title(&format!("{}层", scope.label()), cx))
        .child(text_row(wb, "save-model", "默认模型", &format!("model;各层合并后当前生效:{effective_model}"), "model", Target::Json(&["model"]), cx))
        .child(h_flex().w_full().gap_1().pb_2().justify_end().children(presets.into_iter().map(|p| {
            Button::new(SharedString::from(format!("model-preset-{p}"))).xsmall().ghost().label(p)
                .on_click(cx.listener(move |this, _, window, cx| this.set_field("model", p.to_string(), window, cx)))
        })))
        .child(row("默认开启扩展思考", "alwaysThinkingEnabled", Switch::new("sw-think-always").checked(layer.always_thinking.unwrap_or(false))
            .on_click(cx.listener(|this, c: &bool, _, cx| this.set_layer_bool(&["alwaysThinkingEnabled"], Some(*c), cx))), cx))
        .child(text_row(wb, "save-thinking", "思考预算(tokens)", "env MAX_THINKING_TOKENS", "thinking", Target::Env("MAX_THINKING_TOKENS", true), cx))
        .child(text_row(wb, "save-compact", "自动压缩阈值(%)", "env CLAUDE_AUTOCOMPACT_PCT_OVERRIDE:上下文用到多少时自动压缩", "compact", Target::Env("CLAUDE_AUTOCOMPACT_PCT_OVERRIDE", true), cx))
        .child(text_row(wb, "save-style", "输出风格", "outputStyle;内置 Default / Explanatory / Learning,也可以是 ~/.claude/output-styles 里的自定义风格", "output_style", Target::Json(&["outputStyle"]), cx))
        .child(section_title("API", cx))
        .child(row("当前 API 提供方", "按环境变量判断", pill(api_provider(wb), t.foreground, t.muted), cx))
        .child(text_row(wb, "save-helper", "apiKeyHelper", "运行脚本获取 API Key(适合密钥放在钥匙串或密码管理器里)", "key_helper", Target::Json(&["apiKeyHelper"]), cx))
        .child(section_title("提示词(客户端,所有新会话)", cx))
        .child(v_flex().w_full().py_3().gap_2().border_b_1().border_color(t.border)
            .child(div().text_size(px(13.5)).child("追加系统提示词"))
            .child(caption("以 --append-system-prompt 传给 claude;对之后新建或重新连接的会话生效", cx))
            .child(div().w_full().h(px(110.)).p_2().rounded(px(6.)).border_1().border_color(t.border)
                .child(Textarea::new(&wb.forms().fields.prompt).appearance(false).bordered(false).size_full()))
            .child(h_flex().w_full().justify_end()
                .child(Button::new("save-prompt").small().outline().label("保存").on_click(cx.listener(|this, _, _, cx| this.save_append_prompt(cx))))))
        .child(row("加载 CLAUDE.md", "关闭时注入 CLAUDE_CODE_DISABLE_CLAUDE_MDS=1,新会话不读任何记忆文件", Switch::new("sw-load-md").checked(wb.settings.load_claude_md)
            .on_click(cx.listener(|this, c: &bool, _, cx| { let v = *c; this.update_settings(cx, |s| s.load_claude_md = v) })), cx))
        .into_any_element()
}
