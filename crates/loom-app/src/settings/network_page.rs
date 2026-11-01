//! 网络页:给 claude 进程设置代理、绕过列表与请求超时(客户端偏好,以环境变量注入新会话),以及连通性测试。
//! 文本框失焦或回车即生效,和开关一样不需要「保存」。
//!
//! @author ixxxxoooo

use super::widgets::{error_banner, group, item, note, page_header, row, rows, segmented};
use crate::ui::{MONO, caption, pill};
use crate::workbench::Workbench;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::Input;
use gpui_kit::component::{ActiveTheme, Sizable, v_flex};
use gpui_kit::*;
use loom_config::ProxyMode;

/// 渲染网络页。
pub fn render(wb: &Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    let t = cx.theme().clone();
    let s = &wb.settings;
    let results = wb.forms().ops.net.clone();
    let testing = results.iter().any(|r| r.2.is_none());
    let uses_proxy = matches!(s.proxy_mode, ProxyMode::Http | ProxyMode::Socks5);
    let mut proxy_rows = rows![
        row("代理模式", "跟随系统:沿用继承的 HTTP(S)_PROXY;直连:清空代理变量", segmented("proxy-mode",
            vec![(ProxyMode::System, "跟随系统"), (ProxyMode::Direct, "直连"), (ProxyMode::Http, "HTTP"), (ProxyMode::Socks5, "SOCKS5")], s.proxy_mode, cx,
            |this, v, cx| this.update_settings(cx, |s| s.proxy_mode = v)), cx),
    ];
    if uses_proxy {
        proxy_rows.push(row("代理地址", "写入 HTTP_PROXY、HTTPS_PROXY、ALL_PROXY", div().w(px(260.)).child(Input::new(wb.field("proxy")).small()), cx).into_any_element());
        proxy_rows.push(row("绕过代理", "NO_PROXY:这些主机直连", div().w(px(260.)).child(Input::new(wb.field("no_proxy")).small()), cx).into_any_element());
    }
    let net_rows: Vec<AnyElement> = if results.is_empty() {
        rows![item(cx).h(px(44.)).child(caption("点右上角「开始测试」,用 curl 按上面的代理设置访问各站点", cx))]
    } else {
        results.into_iter().map(|(name, url, r)| {
            let (text, color) = match &r {
                None => ("测试中…".to_string(), t.muted_foreground),
                Some(Ok(ms)) => (format!("{ms} ms"), if *ms < 800 { t.success } else { t.warning }),
                Some(Err(e)) => (e.clone(), t.danger),
            };
            item(cx).h(px(36.)).gap_3()
                .child(div().w(px(130.)).flex_none().text_size(px(13.)).child(name))
                .child(div().flex_1().min_w_0().truncate().font_family(MONO).text_size(px(12.)).text_color(t.muted_foreground).child(url))
                .child(pill(text, color, t.muted))
                .into_any_element()
        }).collect()
    };
    v_flex().w_full()
        .child(page_header("网络", "只影响 LoomCode 启动的 claude 进程;不改系统代理,也不改 ~/.claude", div(), cx))
        .children(error_banner(wb, cx))
        .child(group("代理", proxy_rows, cx))
        .child(group("请求", rows![
            row("请求超时(秒)", "API_TIMEOUT_MS;留空为 claude 默认", div().w(px(120.)).child(Input::new(wb.field("timeout")).small()), cx),
        ], cx))
        .child(note("设置对之后新建或重新连接的会话生效;文本框失焦或回车自动应用", cx))
        .child(gpui_kit::component::h_flex().w_full().items_center().pt_5()
            .child(div().flex_1().text_size(px(12.)).font_weight(FontWeight::MEDIUM).text_color(t.muted_foreground).child("连通性测试"))
            .child(Button::new("net-test").small().primary().label(if testing { "测试中…" } else { "开始测试" }).disabled(testing)
                .on_click(cx.listener(|this, _, _, cx| this.run_connectivity(cx)))))
        .child(div().pt_1p5().child(card_of(net_rows, cx)))
        .child(note("任何 HTTP 响应都算连通;结果不保存", cx))
        .into_any_element()
}

/// 无标题的卡片(连通性结果)。
fn card_of(rows: Vec<AnyElement>, cx: &App) -> Div {
    super::widgets::card(rows, 0., cx)
}
