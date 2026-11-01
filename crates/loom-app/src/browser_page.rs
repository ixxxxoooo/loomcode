//! 内置浏览器页(阅读模式):抓取网页后提取标题、正文与链接,用原生控件渲染。
//! GPUI 没有 WebView,所以不执行脚本也不排版复杂页面;需要完整网页时交给系统浏览器。

use crate::ui::{MONO, caption, empty_state, icon};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_browser::{History, Page, PageBlock};

pub struct BrowserPage {
    input: Entity<InputState>,
    history: History,
    page: Option<Page>,
    loading: bool,
    error: Option<String>,
    _sub: Subscription,
}

impl BrowserPage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("输入网址或搜索词,回车访问"));
        let sub = cx.subscribe_in(&input, window, |this: &mut Self, input, ev: &InputEvent, window, cx| {
            if let InputEvent::PressEnter { .. } = ev {
                let text = input.read(cx).value().to_string();
                if !text.trim().is_empty() {
                    this.open(&text, window, cx);
                }
            }
        });
        Self { input, history: History::default(), page: None, loading: false, error: None, _sub: sub }
    }

    /// 访问地址栏输入(网址或搜索词)。
    pub fn open(&mut self, input: &str, window: &mut Window, cx: &mut Context<Self>) {
        let url = loom_browser::normalize_input(input);
        self.history.visit(&url);
        self.load(url, window, cx);
    }

    fn load(&mut self, url: String, window: &mut Window, cx: &mut Context<Self>) {
        self.loading = true;
        self.error = None;
        self.input.update(cx, |i, cx| i.set_value(url.clone(), window, cx));
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let target = url.clone();
            let result = cx.background_spawn(async move { loom_browser::load(&target) }).await;
            let _ = this.update_in(cx, |this, window, cx| {
                // 期间用户又导航到别处时,丢弃过期结果
                if this.history.current() != Some(url.as_str()) {
                    return;
                }
                this.loading = false;
                match result {
                    Ok(page) => {
                        // 重定向后以最终地址为准
                        if page.url != url {
                            this.input.update(cx, |i, cx| i.set_value(page.url.clone(), window, cx));
                        }
                        this.page = Some(page);
                    }
                    Err(e) => this.error = Some(e.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(url) = self.history.back().map(String::from) {
            self.load(url, window, cx);
        }
    }

    fn forward(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(url) = self.history.forward().map(String::from) {
            self.load(url, window, cx);
        }
    }

    fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(url) = self.history.current().map(String::from) {
            self.load(url, window, cx);
        }
    }

    fn toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let (can_back, can_fwd, has_page) = (self.history.can_back(), self.history.can_forward(), self.history.current().is_some());
        let current = self.history.current().map(String::from);
        h_flex().h(px(44.)).px_3().gap_1().items_center().border_b_1().border_color(t.border)
            .child(Button::new("br-back").small().ghost().icon(IconName::ArrowLeft).disabled(!can_back).on_click(cx.listener(|this, _, window, cx| this.back(window, cx))))
            .child(Button::new("br-fwd").small().ghost().icon(IconName::ArrowRight).disabled(!can_fwd).on_click(cx.listener(|this, _, window, cx| this.forward(window, cx))))
            .child(Button::new("br-reload").small().ghost().icon(IconName::RefreshCw).disabled(!has_page).on_click(cx.listener(|this, _, window, cx| this.reload(window, cx))))
            .child(div().flex_1().px_1().child(Input::new(&self.input)))
            .child(Button::new("br-ext").small().ghost().icon(IconName::ExternalLink).label("系统浏览器").disabled(!has_page).on_click(move |_, _, cx| {
                if let Some(url) = &current {
                    cx.open_url(url);
                }
            }))
    }

    fn page_view(&self, page: &Page, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let blocks = page.blocks.iter().map(|b| match b {
            PageBlock::Heading { level, text } => {
                let size = match level {
                    1 => 24.,
                    2 => 19.,
                    _ => 15.5,
                };
                div().w_full().pt_2().text_size(px(size)).font_weight(FontWeight::SEMIBOLD).child(text.clone()).into_any_element()
            }
            PageBlock::Paragraph(text) => div().w_full().text_size(px(14.)).line_height(px(22.)).child(text.clone()).into_any_element(),
            PageBlock::ListItem(text) => h_flex().w_full().gap_2().items_start().text_size(px(14.)).child(div().text_color(t.muted_foreground).child("•")).child(div().flex_1().child(text.clone())).into_any_element(),
            PageBlock::Quote(text) => div().w_full().pl_3().border_l_2().border_color(t.border).text_color(t.muted_foreground).text_size(px(14.)).child(text.clone()).into_any_element(),
            PageBlock::Code(text) => div().w_full().p_2().rounded(px(6.)).bg(t.muted).font_family(MONO).text_size(px(12.)).child(text.clone()).into_any_element(),
        });
        let links = page.links.iter().enumerate().map(|(i, l)| {
            let url = l.url.clone();
            h_flex().id(SharedString::from(format!("br-link-{i}"))).w_full().gap_2().px_2().py_1().rounded(px(6.)).cursor_pointer().hover(|d| d.bg(t.list_hover))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.history.visit(&url);
                    this.load(url.clone(), window, cx);
                }))
                .child(icon(IconName::Link, 12., t.muted_foreground))
                .child(div().min_w_0().text_size(px(13.)).text_color(t.primary).truncate().child(l.text.clone()))
                .child(div().flex_1().min_w_0().truncate().text_size(px(11.)).text_color(t.muted_foreground).child(l.url.clone()))
        });
        div().id("browser-scroll").size_full().overflow_y_scroll().child(
            v_flex().w_full().max_w(px(820.)).mx_auto().px_6().py_5().gap_3()
                .child(div().text_size(px(11.5)).text_color(t.muted_foreground).child("阅读模式 · 已移除脚本与样式"))
                .child(div().text_size(px(26.)).font_weight(FontWeight::SEMIBOLD).child(page.title.clone()))
                .children(blocks)
                .when(!page.links.is_empty(), |d| {
                    d.child(div().w_full().h(px(1.)).my_2().bg(t.border))
                        .child(caption(format!("页面链接 · {}", page.links.len()), cx))
                        .child(v_flex().w_full().children(links))
                }),
        ).into_any_element()
    }
}

impl Render for BrowserPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let body: AnyElement = if self.loading {
            empty_state(IconName::Globe, "正在加载…", self.history.current().unwrap_or_default(), cx).into_any_element()
        } else if let Some(e) = &self.error {
            empty_state(IconName::TriangleAlert, "无法打开页面", e, cx).into_any_element()
        } else if let Some(page) = self.page.clone() {
            self.page_view(&page, cx)
        } else {
            empty_state(IconName::Globe, "内置浏览器", "输入网址或搜索词开始浏览(阅读模式,不执行网页脚本)", cx).into_any_element()
        };
        v_flex().size_full().bg(t.background).child(self.toolbar(cx)).child(div().flex_1().min_h_0().child(body))
    }
}
