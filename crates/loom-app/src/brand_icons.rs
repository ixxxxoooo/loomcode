//! 模型厂商图标:按模型 id / 名称推断厂商(规则与 OhMyAgent 一致),渲染 lobe-icons 单色图标;
//! 认不出厂商时回退为首字母徽标。SVG 按颜色着色后缓存。
//!
//! @author ixxxxoooo

use crate::brand_icon_data::ICONS;
use gpui_kit::*;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

/// 模型名片段 → 厂商图标名(顺序即优先级,更具体的在前)。
const MODEL_RULES: &[(&str, &str)] = &[
    ("claude", "claude"), ("sonnet", "claude"), ("opus", "claude"), ("haiku", "claude"), ("fable", "claude"), ("default", "claude"),
    ("anthropic", "anthropic"), ("gpt", "openai"), ("chatgpt", "openai"), ("codex", "codex"), ("gemma", "gemma"), ("gemini", "gemini"),
    ("deepseek", "deepseek"), ("qwen", "qwen"), ("qwq", "qwen"), ("kimi", "kimi"), ("moonshot", "moonshot"), ("glm", "zhipu"),
    ("zhipu", "zhipu"), ("minimax", "minimax"), ("grok", "grok"), ("mistral", "mistral"), ("mixtral", "mistral"), ("llama", "meta"),
    ("doubao", "doubao"), ("seed-", "doubao"), ("step-", "stepfun"), ("baichuan", "baichuan"), ("ernie", "wenxin"),
    ("hunyuan", "hunyuan"), ("mimo", "xiaomimimo"), ("longcat", "longcat"), ("command-r", "cohere"), ("nemotron", "nvidia"),
    ("sonar", "perplexity"), ("internlm", "internlm"), ("jamba", "ai21"),
];

/// 推断模型所属厂商的图标名;认不出时返回 `None`。
///
/// 参数:`model_id` 模型 id;`name` 显示名(两者一起匹配)。
pub fn resolve(model_id: &str, name: &str) -> Option<&'static str> {
    let text = format!("{} {}", model_id.to_lowercase(), name.to_lowercase());
    MODEL_RULES.iter().find(|(needle, icon)| text.contains(needle) && ICONS.iter().any(|(n, _, _)| n == icon)).map(|(_, icon)| *icon)
}

/// 生成指定颜色的 SVG 文本;图标名不存在返回 `None`。
fn svg_text(icon: &str, hex: &str) -> Option<String> {
    let (_, paths, even_odd) = ICONS.iter().find(|(n, _, _)| *n == icon)?;
    let rule = if *even_odd { " fill-rule=\"evenodd\" clip-rule=\"evenodd\"" } else { "" };
    let body: String = paths.iter().map(|d| format!("<path d=\"{d}\" fill=\"{hex}\"{rule}/>")).collect();
    Some(format!("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\">{body}</svg>"))
}

/// 颜色转 `#rrggbb`。
fn hex_of(color: Hsla) -> String {
    let c = color.to_rgb();
    let byte = |v: f32| (v.clamp(0., 1.) * 255.).round() as u8;
    format!("#{:02x}{:02x}{:02x}", byte(c.r), byte(c.g), byte(c.b))
}

/// 厂商图标元素:已识别画矢量图标,否则画首字母圆角徽标。
///
/// 参数:`model_id`、`name` 同 [`resolve`];`size` 边长(像素);`color` 着色。
pub fn brand_icon(model_id: &str, name: &str, size: f32, color: Hsla) -> AnyElement {
    static CACHE: OnceLock<Mutex<HashMap<String, Arc<Image>>>> = OnceLock::new();
    if let Some(icon) = resolve(model_id, name) {
        let key = format!("{icon}|{}", hex_of(color));
        let image = {
            let mut cache = CACHE.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
            cache.entry(key).or_insert_with_key(|k| {
                let svg = svg_text(icon, k.split('|').nth(1).unwrap_or("#888888")).unwrap_or_default();
                Arc::new(Image::from_bytes(ImageFormat::Svg, svg.into_bytes()))
            }).clone()
        };
        return img(image).size(px(size)).flex_none().into_any_element();
    }
    let letter = name.trim().chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "?".into());
    div().size(px(size)).flex_none().flex().items_center().justify_center().rounded(px(size * 0.28)).bg(color.opacity(0.15))
        .text_size(px(size * 0.62)).font_weight(FontWeight::SEMIBOLD).text_color(color).child(letter).into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 识别厂商() {
        assert_eq!(resolve("opus[1m]", "Opus"), Some("claude"));
        assert_eq!(resolve("default", "Default (recommended)"), Some("claude"));
        assert_eq!(resolve("deepseek-chat", ""), Some("deepseek"));
        assert_eq!(resolve("glm-4.6", "GLM"), Some("zhipu"));
        assert_eq!(resolve("foo-bar", "Foo"), None);
    }

    #[::core::prelude::v1::test]
    fn 生成_svg() {
        let svg = svg_text("claude", "#112233").unwrap();
        assert!(svg.contains("viewBox=\"0 0 24 24\"") && svg.contains("fill=\"#112233\""));
        assert!(svg_text("不存在", "#000000").is_none());
    }
}
