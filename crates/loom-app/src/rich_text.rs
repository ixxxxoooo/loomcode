//! 助手正文的富文本切分:把 ```chart / ```mermaid 围栏块从 Markdown 中拆出来,
//! 其余内容仍交给 Markdown 视图渲染。

use crate::ui::{MONO, card, icon};
use gpui_kit::assets::IconName;
use gpui_kit::component::chart::{CandlestickChart, LineChart};
use gpui_kit::component::text::TextView;
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::*;
use serde_json::Value;

/// 内联图表的数据。
#[derive(Debug, Clone, PartialEq)]
pub enum ChartSpec {
    Line { title: String, data: Vec<f64> },
    /// 每项为 [开, 高, 低, 收]
    Candle { title: String, data: Vec<[f64; 4]> },
}

/// 正文片段。
#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Markdown(String),
    Chart(ChartSpec),
    /// 无法渲染成图时回退为源码展示;`kind` 为围栏语言
    Source { kind: String, code: String },
    /// 普通围栏代码块(限高滚动,见 `code_box`);`closed` 为假表示还在流式输出、围栏未闭合
    Code { lang: String, code: String, closed: bool },
}

/// 解析 `chart` 围栏里的 JSON。
pub fn parse_chart(json: &str) -> Option<ChartSpec> {
    let v: Value = serde_json::from_str(json).ok()?;
    let title = v.get("title").and_then(Value::as_str).unwrap_or("").to_string();
    let data = v.get("data")?.as_array()?;
    match v.get("type").and_then(Value::as_str)? {
        "line" => Some(ChartSpec::Line { title, data: data.iter().filter_map(Value::as_f64).collect() }),
        "candle" => {
            let rows: Vec<[f64; 4]> = data
                .iter()
                .filter_map(|r| {
                    let a = r.as_array()?;
                    Some([a.first()?.as_f64()?, a.get(1)?.as_f64()?, a.get(2)?.as_f64()?, a.get(3)?.as_f64()?])
                })
                .collect();
            Some(ChartSpec::Candle { title, data: rows })
        }
        _ => None,
    }
}

/// 把正文按围栏切成片段。未闭合的围栏(流式输出中)保持在 Markdown 里。
pub fn split_blocks(text: &str) -> Vec<Block> {
    let mut out = Vec::new();
    let mut md = String::new();
    let mut lines = text.split_inclusive('\n').peekable();
    while let Some(line) = lines.next() {
        let trimmed = line.trim_start();
        // 四个及以上反引号的外层围栏(里面可以放三个反引号):整段原样交给 Markdown
        if trimmed.starts_with("````") {
            let ticks = trimmed.chars().take_while(|c| *c == '`').count();
            md.push_str(line);
            for next in lines.by_ref() {
                md.push_str(next);
                if next.trim() == "`".repeat(ticks) {
                    break;
                }
            }
            continue;
        }
        let lang = trimmed.strip_prefix("```").map(str::trim);
        match lang {
            Some(lang @ ("chart" | "mermaid")) => {
                // 向后找闭合围栏;找不到就当普通 Markdown
                let mut body = String::new();
                let mut closed = false;
                let mut consumed: Vec<&str> = Vec::new();
                for next in lines.by_ref() {
                    consumed.push(next);
                    if next.trim() == "```" {
                        closed = true;
                        break;
                    }
                    body.push_str(next);
                }
                if closed {
                    if !md.trim().is_empty() {
                        out.push(Block::Markdown(std::mem::take(&mut md)));
                    } else {
                        md.clear();
                    }
                    let block = if lang == "chart" {
                        parse_chart(&body).map(Block::Chart).unwrap_or(Block::Source { kind: "chart".into(), code: body })
                    } else {
                        Block::Source { kind: "mermaid".into(), code: body }
                    };
                    out.push(block);
                } else {
                    md.push_str(line);
                    for c in consumed {
                        md.push_str(c);
                    }
                }
            }
            // 其他围栏(```rust、``` 等):拆出来做成限高的代码块;四个以上反引号交给 Markdown
            Some(lang) => {
                let mut body = String::new();
                let mut closed = false;
                for next in lines.by_ref() {
                    if next.trim() == "```" {
                        closed = true;
                        break;
                    }
                    body.push_str(next);
                }
                if !md.trim().is_empty() {
                    out.push(Block::Markdown(std::mem::take(&mut md)));
                } else {
                    md.clear();
                }
                let lang = lang.split_whitespace().next().unwrap_or("").to_string();
                out.push(Block::Code { lang, code: body, closed });
            }
            _ => md.push_str(line),
        }
    }
    if !md.trim().is_empty() || out.is_empty() {
        out.push(Block::Markdown(md));
    }
    out
}

/// 渲染助手正文。
///
/// 参数:`session` 会话(代码块的展开状态存在它的展开集合里);`id` 条目 id;`text` 正文。
pub fn render_assistant(session: &Entity<crate::model::SessionModel>, id: &str, text: &str, cx: &App) -> AnyElement {
    let blocks = split_blocks(text);
    v_flex().w_full().gap_2().text_size(px(14. * cx.try_global::<crate::UiPrefs>().map_or(1., |p| p.font_scale)))
        .children(blocks.into_iter().enumerate().map(|(n, b)| match b {
            Block::Markdown(md) => div().w_full().child(TextView::markdown(SharedString::from(format!("md-{id}-{n}")), md).selectable(true)).into_any_element(),
            Block::Chart(spec) => render_chart(id, n, &spec, cx),
            Block::Source { kind, code } => render_source(&kind, &code, cx),
            Block::Code { lang, code, closed } => {
                // 展开状态:键 `cb:<条目 id>:<第几块>`,存在会话的展开集合里,切换时重新测量所在行
                let key = format!("cb:{id}:{n}");
                let fold = crate::code_box::Fold {
                    expanded: session.read(cx).is_expanded(&key),
                    on_toggle: std::rc::Rc::new({
                        let s = session.clone();
                        move |_, cx| { s.update(cx, |m, cx| m.toggle(&key, cx)); }
                    }),
                };
                crate::code_box::render_code(SharedString::from(format!("cb-{id}-{n}")), &lang, &code, !closed, Some(&fold), cx)
            }
        }))
        .into_any_element()
}

fn render_chart(id: &str, n: usize, spec: &ChartSpec, cx: &App) -> AnyElement {
    let t = cx.theme();
    let (title, body) = match spec {
        ChartSpec::Line { title, data } => {
            let pts: Vec<(usize, f64)> = data.iter().copied().enumerate().collect();
            (
                title.clone(),
                div().h(px(180.)).w_full().child(
                    LineChart::new(pts).id(SharedString::from(format!("chart-{id}-{n}"))).x(|p: &(usize, f64)| SharedString::from((p.0 + 1).to_string())).y(|p: &(usize, f64)| p.1).stroke(t.primary).dot(),
                ),
            )
        }
        ChartSpec::Candle { title, data } => {
            let rows: Vec<(usize, [f64; 4])> = data.iter().copied().enumerate().collect();
            (
                title.clone(),
                div().h(px(200.)).w_full().child(
                    CandlestickChart::new(rows)
                        .id(SharedString::from(format!("chart-{id}-{n}")))
                        .x(|r: &(usize, [f64; 4])| SharedString::from((r.0 + 1).to_string()))
                        .open(|r: &(usize, [f64; 4])| r.1[0])
                        .high(|r: &(usize, [f64; 4])| r.1[1])
                        .low(|r: &(usize, [f64; 4])| r.1[2])
                        .close(|r: &(usize, [f64; 4])| r.1[3]),
                ),
            )
        }
    };
    card(cx).p_3().gap_2()
        .child(h_flex().gap_2().items_center().child(icon(IconName::ChartLine, 14., t.muted_foreground)).child(div().text_size(px(12.5)).font_weight(FontWeight::MEDIUM).child(title)))
        .child(body)
        .into_any_element()
}

/// 无法渲染成图时,以源码卡片展示(Mermaid 暂不做排版引擎,按 Zed 的取舍显示源码)。
fn render_source(kind: &str, code: &str, cx: &App) -> AnyElement {
    let t = cx.theme();
    card(cx).p_3().gap_1p5()
        .child(h_flex().gap_2().items_center().child(icon(IconName::Code, 14., t.muted_foreground)).child(div().text_size(px(12.)).text_color(t.muted_foreground).child(format!("{kind} 源码"))))
        .child(div().w_full().p_2().rounded(px(6.)).bg(t.muted).font_family(MONO).text_size(px(12.)).child({ let code = code.trim_end().to_string(); let lang = if kind == "chart" { "json" } else { kind }; StyledText::new(code.clone()).with_highlights(crate::code_hl::styles(&code, lang, cx)) }))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 切分图表与正文() {
        let text = "前文\n\n```chart\n{\"type\":\"line\",\"title\":\"t\",\"data\":[1,2,3]}\n```\n\n后文\n";
        let blocks = split_blocks(text);
        assert_eq!(blocks.len(), 3);
        assert!(matches!(&blocks[1], Block::Chart(ChartSpec::Line { data, .. }) if data.len() == 3));
    }

    #[::core::prelude::v1::test]
    fn 未闭合围栏保持为正文() {
        let blocks = split_blocks("```mermaid\nflowchart LR\n");
        assert_eq!(blocks.len(), 1);
        assert!(matches!(&blocks[0], Block::Markdown(_)));
    }

    #[::core::prelude::v1::test]
    fn 坏数据回退为源码() {
        let blocks = split_blocks("```chart\n{bad json}\n```\n");
        assert!(matches!(&blocks[0], Block::Source { kind, .. } if kind == "chart"));
    }

    #[::core::prelude::v1::test]
    fn 普通围栏拆成代码块() {
        let blocks = split_blocks("前文\n```rust title=a\nfn main() {}\n```\n后文\n");
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[1], Block::Code { lang: "rust".into(), code: "fn main() {}\n".into(), closed: true });
        assert!(matches!(&blocks[2], Block::Markdown(m) if m.contains("后文")));
    }

    #[::core::prelude::v1::test]
    fn 流式未闭合的围栏也是代码块() {
        let blocks = split_blocks("```\nlet a = 1;\nlet b");
        assert_eq!(blocks, vec![Block::Code { lang: String::new(), code: "let a = 1;\nlet b".into(), closed: false }]);
    }

    #[::core::prelude::v1::test]
    fn 四个反引号交给_markdown() {
        let blocks = split_blocks("````md\n```x\n```\n````\n");
        assert!(blocks.iter().all(|b| matches!(b, Block::Markdown(_))));
    }

    #[::core::prelude::v1::test]
    fn 解析蜡烛图() {
        let c = parse_chart("{\"type\":\"candle\",\"data\":[[1,2,0,1.5]]}").unwrap();
        assert_eq!(c, ChartSpec::Candle { title: String::new(), data: vec![[1., 2., 0., 1.5]] });
    }
}
