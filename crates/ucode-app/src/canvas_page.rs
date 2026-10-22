//! Canvas 画布页:左侧文档列表,右侧把声明式文档渲染成原生控件(统计卡、图表、表格、提示框……),
//! 也可切到源码视图查看 JSON。Mermaid 暂不做排版引擎,以源码卡片展示。

use crate::ui::{MONO, card, caption, empty_state, icon, pill};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::chart::{AreaChart, BarChart, CandlestickChart, PieChart};
use gpui_kit::component::input::{Editor, EditorState};
use gpui_kit::component::text::TextView;
use gpui_kit::component::{ActiveTheme, Selectable, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::collections::HashSet;
use std::path::PathBuf;
use ucode_canvas::*;

pub struct CanvasPage {
    store: CanvasStore,
    list: Vec<CanvasSummary>,
    current: Option<CanvasDocument>,
    /// 是否显示源码视图
    source: bool,
    source_editor: Option<Entity<EditorState>>,
    /// 用户手动翻转过折叠状态的分区(以路径标识)
    toggled: HashSet<String>,
    /// 读取失败的说明
    error: Option<String>,
}

impl CanvasPage {
    pub fn new(store: CanvasStore) -> Self {
        let mut page = Self { store, list: Vec::new(), current: None, source: false, source_editor: None, toggled: HashSet::new(), error: None };
        page.reload();
        if let Some(first) = page.list.first().map(|s| s.id.clone()) {
            page.load(&first);
        }
        page
    }

    /// 重新读取文档列表。
    pub fn reload(&mut self) {
        self.list = self.store.list();
    }

    fn load(&mut self, id: &str) {
        match self.store.load(id) {
            Ok(doc) => {
                self.current = Some(doc);
                self.error = None;
            }
            Err(e) => self.error = Some(e.to_string()),
        }
        self.toggled.clear();
        self.source_editor = None;
    }

    /// 选中某个文档(其它模块请求打开画布时调用)。
    pub fn select(&mut self, id: &str, cx: &mut Context<Self>) {
        self.reload();
        self.load(id);
        cx.notify();
    }

    fn set_source(&mut self, on: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.source = on;
        if on && let Some(doc) = &self.current {
            let json = doc.to_json();
            self.source_editor = Some(cx.new(|cx| EditorState::new(window, cx).language("json").default_value(json)));
        }
        cx.notify();
    }

    fn delete_current(&mut self, cx: &mut Context<Self>) {
        if let Some(doc) = self.current.take() {
            if let Err(e) = self.store.delete(&doc.id) {
                self.error = Some(e.to_string());
            }
            self.reload();
            if let Some(first) = self.list.first().map(|s| s.id.clone()) {
                self.load(&first);
            }
            cx.notify();
        }
    }

    // ---------- 渲染 ----------

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let current = self.current.as_ref().map(|d| d.id.clone());
        v_flex().w(px(220.)).h_full().flex_none().border_r_1().border_color(t.border).bg(t.sidebar)
            .child(h_flex().h(px(36.)).px_3().items_center().text_size(px(12.)).text_color(t.muted_foreground).child(div().flex_1().child("画布")).child(
                Button::new("canvas-refresh").small().ghost().icon(IconName::RefreshCw).on_click(cx.listener(|this, _, _, cx| {
                    this.reload();
                    cx.notify();
                })),
            ))
            .child(v_flex().id("canvas-list").flex_1().min_h_0().overflow_y_scroll().px_2().gap_0p5().children(self.list.iter().map(|s| {
                let on = current.as_deref() == Some(s.id.as_str());
                let id = s.id.clone();
                v_flex().id(SharedString::from(format!("cv-{}", s.id))).px_2().py_1p5().rounded(px(6.)).cursor_pointer().gap_0p5()
                    .when(on, |d| d.bg(t.list_active))
                    .hover(|d| d.bg(t.list_hover))
                    .on_click(cx.listener(move |this, _, _, cx| this.select(&id, cx)))
                    .child(div().text_size(px(12.5)).truncate().child(s.title.clone()))
                    .child(caption(ucode_automation_like_time(s.updated_at), cx))
            })))
    }

    fn header(&self, doc: &CanvasDocument, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let source = self.source;
        h_flex().h(px(40.)).px_4().gap_2().items_center().border_b_1().border_color(t.border)
            .child(icon(IconName::LayoutDashboard, 15., t.primary))
            .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(doc.title.clone()))
            .child(pill(format!("v{}", doc.revision), t.muted_foreground, t.muted))
            .child(pill(format!("{} 个块", doc.block_count()), t.muted_foreground, t.muted))
            .child(div().flex_1())
            .child(Button::new("cv-view").small().ghost().label("渲染").selected(!source).on_click(cx.listener(|this, _, window, cx| this.set_source(false, window, cx))))
            .child(Button::new("cv-src").small().ghost().label("源码").selected(source).on_click(cx.listener(|this, _, window, cx| this.set_source(true, window, cx))))
            .child(Button::new("cv-del").small().ghost().icon(IconName::Trash).on_click(cx.listener(|this, _, _, cx| this.delete_current(cx))))
    }

    fn render_blocks(&self, blocks: &[Block], path: &str, cx: &mut Context<Self>) -> Vec<AnyElement> {
        blocks.iter().enumerate().map(|(i, b)| self.render_block(b, &format!("{path}.{i}"), cx)).collect()
    }

    fn render_block(&self, block: &Block, path: &str, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        match block {
            Block::Heading { text, level } => {
                let size = match level {
                    1 => 24.,
                    2 => 19.,
                    _ => 15.,
                };
                div().w_full().pt_2().text_size(px(size)).font_weight(FontWeight::SEMIBOLD).child(text.clone()).into_any_element()
            }
            Block::Markdown { text } => div().w_full().text_size(px(14.)).child(TextView::markdown(SharedString::from(format!("cvmd-{path}")), text.clone()).selectable(true)).into_any_element(),
            Block::Stats { items } => h_flex().w_full().gap_3().flex_wrap().children(items.iter().map(|s| {
                let color = tone_color(s.tone, &t);
                card(cx).w(px(180.)).p_3().gap_1()
                    .child(caption(s.label.clone(), cx))
                    .child(div().text_size(px(22.)).font_weight(FontWeight::SEMIBOLD).child(s.value.clone()))
                    .when_some(s.delta.clone(), |d, delta| d.child(div().text_size(px(11.5)).text_color(color).child(delta)))
            })).into_any_element(),
            Block::Table { columns, rows, title } => {
                let cols = columns.len().max(1);
                card(cx).overflow_hidden()
                    .when_some(title.clone(), |d, title| d.child(div().px_3().py_2().text_size(px(12.5)).font_weight(FontWeight::MEDIUM).child(title)))
                    .child(h_flex().w_full().bg(t.muted).children(columns.iter().map(|c| div().flex_1().min_w_0().px_3().py_1p5().text_size(px(12.)).text_color(t.muted_foreground).truncate().child(c.clone()))))
                    .children(rows.iter().enumerate().map(|(ri, row)| {
                        h_flex().w_full().border_t_1().border_color(t.border).children((0..cols).map(|ci| {
                            div().flex_1().min_w_0().px_3().py_1p5().text_size(px(12.5)).truncate().child(row.get(ci).cloned().unwrap_or_default())
                        })).id(SharedString::from(format!("cvrow-{path}-{ri}")))
                    }))
                    .into_any_element()
            }
            Block::Chart(c) => chart_view(&c.chart, path, cx),
            Block::Callout { tone, title, text } => {
                let color = tone_color(*tone, &t);
                v_flex().w_full().gap_1().p_3().rounded(px(8.)).border_l_2().border_color(color).bg(color.opacity(0.08))
                    .when_some(title.clone(), |d, title| d.child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).text_color(color).child(title)))
                    .child(div().text_size(px(13.)).child(TextView::markdown(SharedString::from(format!("cvco-{path}")), text.clone())))
                    .into_any_element()
            }
            Block::Code { language, text } => code_card(language.as_deref().unwrap_or("代码"), text, cx),
            Block::Mermaid { text } => code_card("mermaid 源码", text, cx),
            Block::Image { source, caption: cap } => {
                let local = !source.starts_with("http");
                let exists = local && std::path::Path::new(source).exists();
                v_flex().w_full().gap_1()
                    .child(if exists {
                        img(PathBuf::from(source)).max_h(px(260.)).rounded(px(8.)).into_any_element()
                    } else {
                        h_flex().w_full().h(px(80.)).items_center().justify_center().gap_2().rounded(px(8.)).bg(t.muted).text_size(px(12.)).text_color(t.muted_foreground)
                            .child(icon(IconName::Image, 16., t.muted_foreground))
                            .child(if local { "图片文件不存在".to_string() } else { format!("远程图片未加载:{source}") })
                            .into_any_element()
                    })
                    .when_some(cap.clone(), |d, c| d.child(caption(c, cx)))
                    .into_any_element()
            }
            Block::Section { title, collapsed, blocks } => {
                let open = *collapsed == self.toggled.contains(path);
                let key = path.to_string();
                v_flex().w_full().gap_2()
                    .child(
                        h_flex().id(SharedString::from(format!("cvsec-{path}"))).gap_1p5().items_center().cursor_pointer().py_1()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if !this.toggled.remove(&key) {
                                    this.toggled.insert(key.clone());
                                }
                                cx.notify();
                            }))
                            .child(icon(if open { IconName::ChevronDown } else { IconName::ChevronRight }, 14., t.muted_foreground))
                            .child(div().text_size(px(13.5)).font_weight(FontWeight::MEDIUM).child(title.clone())),
                    )
                    .when(open, |d| d.child(v_flex().w_full().gap_3().pl_4().border_l_1().border_color(t.border).children(self.render_blocks(blocks, path, cx))))
                    .into_any_element()
            }
            Block::Divider => div().w_full().h(px(1.)).bg(t.border).into_any_element(),
        }
    }
}

/// Unix 秒 → `MM-DD HH:MM`。
fn ucode_automation_like_time(ts: i64) -> String {
    ucode_automation::format_ts(ts)
}

fn tone_color(tone: Tone, t: &gpui_kit::component::Theme) -> Hsla {
    match tone {
        Tone::Neutral => t.muted_foreground,
        Tone::Info => t.info,
        Tone::Success => t.success,
        Tone::Warning => t.warning,
        Tone::Danger => t.danger,
    }
}

fn code_card(label: &str, text: &str, cx: &App) -> AnyElement {
    let t = cx.theme();
    card(cx).p_3().gap_1p5()
        .child(h_flex().gap_2().items_center().child(icon(IconName::Code, 14., t.muted_foreground)).child(div().text_size(px(12.)).text_color(t.muted_foreground).child(label.to_string())))
        .child(div().w_full().p_2().rounded(px(6.)).bg(t.muted).font_family(MONO).text_size(px(12.)).child(text.trim_end().to_string()))
        .into_any_element()
}

/// 图表配色。
fn palette(t: &gpui_kit::component::Theme) -> [Hsla; 5] {
    [t.chart_1, t.chart_2, t.chart_3, t.chart_4, t.chart_5]
}

/// 渲染一个图表块。多系列的柱状图拆成并排的小图(组件本身只支持单系列),折线 / 面积图用多条曲线叠加。
fn chart_view(c: &Chart, path: &str, cx: &App) -> AnyElement {
    let t = cx.theme().clone();
    let colors = palette(&t);
    let id = |suffix: &str| SharedString::from(format!("cvchart-{path}-{suffix}"));
    let legend = |c: &Chart| {
        h_flex().gap_3().flex_wrap().children(c.series.iter().enumerate().map(|(i, s)| {
            h_flex().gap_1().items_center().child(div().size(px(8.)).rounded_full().bg(colors[i % colors.len()])).child(caption(s.name.clone(), cx))
        }))
    };

    let body: AnyElement = match c.kind {
        ChartKind::Bar => v_flex().w_full().gap_2()
            .children(c.series.iter().enumerate().map(|(si, s)| {
                let data: Vec<(SharedString, f64)> = c.labels.iter().cloned().map(SharedString::from).zip(s.values.iter().copied()).collect();
                let color = colors[si % colors.len()];
                v_flex().w_full().gap_0p5()
                    .when(c.series.len() > 1, |d| d.child(caption(s.name.clone(), cx)))
                    .child(div().h(px(150.)).w_full().child(
                        BarChart::new(data).id(id(&si.to_string())).band(|d: &(SharedString, f64)| d.0.clone()).value(|d: &(SharedString, f64)| d.1).fill(move |_, _, _, _| color).value_axis(true),
                    ))
            }))
            .into_any_element(),
        ChartKind::Line | ChartKind::Area => {
            let rows: Vec<(SharedString, Vec<f64>)> = c.labels.iter().enumerate()
                .map(|(i, l)| (SharedString::from(l.clone()), c.series.iter().map(|s| s.values.get(i).copied().unwrap_or(0.)).collect()))
                .collect();
            let mut chart = AreaChart::new(rows).id(id("a")).x(|r: &(SharedString, Vec<f64>)| r.0.clone());
            let area = c.kind == ChartKind::Area;
            for si in 0..c.series.len() {
                let color = colors[si % colors.len()];
                chart = chart.y(move |r: &(SharedString, Vec<f64>)| r.1[si]).stroke(color).fill(color.opacity(if area { 0.25 } else { 0.0 })).natural();
            }
            div().h(px(200.)).w_full().child(chart).into_any_element()
        }
        ChartKind::Pie => {
            let data: Vec<(SharedString, f32, Hsla)> = c.labels.iter().enumerate()
                .map(|(i, l)| (SharedString::from(l.clone()), c.series.first().and_then(|s| s.values.get(i)).copied().unwrap_or(0.) as f32, colors[i % colors.len()]))
                .collect();
            div().h(px(220.)).w_full().child(
                PieChart::new(data).id(id("p")).value(|d: &(SharedString, f32, Hsla)| d.1).color(|d: &(SharedString, f32, Hsla)| d.2).label(|d: &(SharedString, f32, Hsla)| d.0.clone()).outer_radius(80.).inner_radius(36.),
            ).into_any_element()
        }
        ChartKind::Candle => {
            let rows: Vec<(SharedString, Candle)> = c.labels.iter().cloned().map(SharedString::from).zip(c.candles.iter().copied()).collect();
            let (up, down) = if c.up_is_red { (t.danger, t.success) } else { (t.success, t.danger) };
            let closes: Vec<f64> = c.candles.iter().map(|k| k.close).collect();
            let ma_note: Vec<String> = c.moving_averages.iter()
                .filter_map(|p| moving_average(&closes, *p as usize).last().copied().flatten().map(|v| format!("MA{p} {v:.2}")))
                .collect();
            v_flex().w_full().gap_1()
                .child(div().h(px(220.)).w_full().child(
                    CandlestickChart::new(rows).id(id("k")).x(|r: &(SharedString, Candle)| r.0.clone())
                        .open(|r: &(SharedString, Candle)| r.1.open).high(|r: &(SharedString, Candle)| r.1.high)
                        .low(|r: &(SharedString, Candle)| r.1.low).close(|r: &(SharedString, Candle)| r.1.close)
                        .bullish(up).bearish(down),
                ))
                .when(!ma_note.is_empty(), |d| d.child(caption(ma_note.join("  "), cx)))
                .into_any_element()
        }
    };

    card(cx).p_3().gap_2()
        .when_some(c.title.clone(), |d, title| d.child(h_flex().gap_2().items_center().child(icon(IconName::ChartLine, 14., t.muted_foreground)).child(div().text_size(px(12.5)).font_weight(FontWeight::MEDIUM).child(title))))
        .child(body)
        .when(c.series.len() > 1 && matches!(c.kind, ChartKind::Line | ChartKind::Area), |d| d.child(legend(c)))
        .into_any_element()
}

impl Render for CanvasPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let body: AnyElement = match (&self.current, &self.error) {
            (_, Some(e)) => empty_state(IconName::TriangleAlert, "画布读取失败", e, cx).into_any_element(),
            (None, None) => empty_state(IconName::LayoutDashboard, "还没有画布", "让 Claude 调用 canvas 工具生成,或在演示模式查看示例", cx).into_any_element(),
            (Some(doc), None) => {
                let doc = doc.clone();
                v_flex().size_full()
                    .child(self.header(&doc, cx))
                    .child(if self.source {
                        match &self.source_editor {
                            Some(state) => div().flex_1().min_h_0().child(Editor::new(state).readonly(true).bordered(false).appearance(false).size_full()).into_any_element(),
                            None => div().into_any_element(),
                        }
                    } else {
                        // 调试用:UCODE_CANVAS_SKIP=n 跳过前 n 个块,方便截图检查靠后的内容
                        let skip = std::env::var("UCODE_CANVAS_SKIP").ok().and_then(|v| v.parse().ok()).unwrap_or(0usize);
                        let blocks = self.render_blocks(&doc.blocks[skip.min(doc.blocks.len())..], "b", cx);
                        div().id("canvas-doc").flex_1().min_h_0().overflow_y_scroll().child(
                            v_flex().w_full().max_w(px(860.)).mx_auto().px_6().py_5().gap_4().children(blocks),
                        ).into_any_element()
                    })
                    .into_any_element()
            }
        };
        h_flex().size_full().bg(t.background).child(self.sidebar(cx)).child(div().flex_1().min_w_0().h_full().child(body))
    }
}
