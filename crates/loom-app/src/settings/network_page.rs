//! 网络页:给 claude 进程设置代理与请求超时(客户端偏好,以环境变量注入新会话),以及连通性测试。
//!
//! @author ixxxxoooo

use super::widgets::{error_banner, page_title, row, section_title, segmented};
use crate::ui::{MONO, caption, pill};
use crate::workbench::Workbench;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::Input;
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::*;
use loom_config::ProxyMode;

/// 渲染网络页。
pub fn render(wb: &Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    let t = cx.theme().clone();
    let s = &wb.settings;
    let results = wb.forms().ops.net.clone();
    let testing = results.iter().any(|r| r.2.is_none());
    v_flex().w_full()
        .child(page_title("网络"))
        .child(caption("只影响 LoomCode 启动的 claude 进程(对之后新建或重新连接的会话生效);不改系统代理,也不改 ~/.claude", cx))
        .children(error_banner(wb, cx))
        .child(section_title("代理", cx))
        .child(row("代理模式", "跟随系统:沿用继承的 HTTP(S)_PROXY;直连:清空代理变量", segmented("proxy-mode",
            vec![(ProxyMode::System, "跟随系统"), (ProxyMode::Direct, "直连"), (ProxyMode::Http, "HTTP"), (ProxyMode::Socks5, "SOCKS5")], s.proxy_mode, cx,
            |this, v, cx| this.update_settings(cx, |s| s.proxy_mode = v)), cx))
        .child(row("代理地址", "HTTP / SOCKS5 模式使用,写入 HTTP_PROXY、HTTPS_PROXY、ALL_PROXY", h_flex().w(px(300.)).gap_2()
            .child(div().flex_1().child(Input::new(wb.field("proxy")).small()))
            .child(Button::new("save-proxy").small().outline().label("保存").on_click(cx.listener(|this, _, _, cx| {
                let v = this.field_value("proxy", cx);
                this.update_settings(cx, |s| s.proxy_address = v);
            }))), cx))
        .child(row("请求超时", "API_TIMEOUT_MS;留空为 claude 默认", h_flex().w(px(300.)).gap_2()
            .child(div().flex_1().child(Input::new(wb.field("timeout")).small()))
            .child(Button::new("save-timeout").small().outline().label("保存").on_click(cx.listener(|this, _, _, cx| {
                let v = this.field_value("timeout", cx);
                match v.parse::<u64>() {
                    _ if v.is_empty() => this.update_settings(cx, |s| s.request_timeout = None),
                    Ok(n) if n > 0 => this.update_settings(cx, |s| s.request_timeout = Some(n)),
                    _ => { this.forms_mut().error = Some("超时需要是正整数(秒)".into()); cx.notify(); }
                }
            }))), cx))
        .child(h_flex().w_full().items_center().pt_5().pb_1()
            .child(div().flex_1().text_size(px(12.)).font_weight(FontWeight::MEDIUM).text_color(t.muted_foreground).child("连通性测试"))
            .child(Button::new("net-test").small().primary().label(if testing { "测试中…" } else { "开始测试" }).disabled(testing)
                .on_click(cx.listener(|this, _, _, cx| this.run_connectivity(cx)))))
        .child(caption("用 curl 按上面的代理设置访问各站点,任何 HTTP 响应都算连通", cx))
        .child(v_flex().w_full().pt_2().gap_1().children(results.into_iter().map(|(name, url, r)| {
            let (text, color) = match &r {
                None => ("测试中…".to_string(), t.muted_foreground),
                Some(Ok(ms)) => (format!("{ms} ms"), if *ms < 800 { t.success } else { t.warning }),
                Some(Err(e)) => (e.clone(), t.danger),
            };
            h_flex().w_full().h(px(30.)).px_2().gap_3().items_center().rounded(px(6.)).border_1().border_color(t.border)
                .child(div().w(px(130.)).flex_none().text_size(px(13.)).child(name))
                .child(div().flex_1().min_w_0().truncate().font_family(MONO).text_size(px(12.)).text_color(t.muted_foreground).child(url))
                .child(pill(text, color, t.muted))
        })))
        .into_any_element()
}
