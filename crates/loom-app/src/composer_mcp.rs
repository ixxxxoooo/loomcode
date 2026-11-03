//! 输入框的 MCP 选择框:点开看当前项目用了哪些 MCP 服务器(连接状态、工具数),一键开关。
//!
//! 开关只记在**当前项目**下(用户层与本地层的服务器写 `~/.claude.json` 里该项目的 `disabledMcpServers`,
//! 项目层 `.mcp.json` 的写 `settings.local.json`),持久、不影响别的项目。claude 只在启动时读 MCP 配置,
//! 所以切换后该项目下已启动的会话会在下一条消息起用新配置重启(回合进行中的等回合结束)。
//!
//! 数据来源:配置(`Workbench.claude_cfg`)给名称、层、启用与否;会话里 `system init` 给连接状态与工具数
//! (`SessionState.mcp`,会话还没启动时没有);工具总占用取上下文明细里的「MCP tools」。
//!
//! @author ixxxxoooo

use crate::composer::Composer;
use crate::composer_pop::short_tokens;
use crate::ui::icon;
use gpui_kit::assets::IconName;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_config::{McpServer, Scope};
use loom_core::{McpConn, McpServerStatus, Usage};

/// 工具定义占上下文超过这个比例就提醒。
pub const WARN_RATIO: f32 = 0.10;

/// 弹层里的一行。
#[derive(Debug, Clone, PartialEq)]
pub struct McpRow {
    pub name: String,
    /// 配置所在的层;`None` 表示配置里找不到(插件等带来的),只能看不能切
    pub scope: Option<Scope>,
    pub enabled: bool,
    /// 项目层服务器还没被批准
    pub needs_approval: bool,
    /// 本会话里的连接状态(没启动、或已被停用时为 `Unknown`)
    pub conn: McpConn,
    /// 本会话里它提供的工具数(没有信息为 `None`)
    pub tools: Option<u32>,
}

/// 合并配置与会话状态:先是配置里的服务器(已启用的在前,各自按名字排),再是会话里有、配置里没有的。
///
/// 参数:`cfg` 各层配置里的服务器;`status` 本会话 `system init` 报的状态。
pub fn build_rows(cfg: &[McpServer], status: &[McpServerStatus]) -> Vec<McpRow> {
    let live = |name: &str| status.iter().find(|s| s.name == name);
    let mut rows: Vec<McpRow> = cfg
        .iter()
        .map(|s| {
            // 已停用的服务器:进程里可能还连着(要等重启),但对用户来说它已经关了,不显示旧状态
            let l = live(&s.name).filter(|_| s.enabled);
            McpRow { name: s.name.clone(), scope: Some(s.scope), enabled: s.enabled, needs_approval: s.needs_approval, conn: l.map(|l| l.conn).unwrap_or_default(), tools: l.map(|l| l.tools) }
        })
        .collect();
    rows.sort_by(|a, b| b.enabled.cmp(&a.enabled).then_with(|| a.name.cmp(&b.name)));
    for s in status {
        if !cfg.iter().any(|c| c.name == s.name) {
            rows.push(McpRow { name: s.name.clone(), scope: None, enabled: true, needs_approval: false, conn: s.conn, tools: Some(s.tools) });
        }
    }
    rows
}

/// 已启用的服务器个数(输入框上 chip 显示的数字)。
pub fn enabled_count(rows: &[McpRow]) -> usize {
    rows.iter().filter(|r| r.enabled).count()
}

/// 有没有已启用但连不上 / 要授权的服务器(chip 上用警告色提示)。
pub fn has_problem(rows: &[McpRow]) -> bool {
    rows.iter().any(|r| r.enabled && matches!(r.conn, McpConn::Failed | McpConn::NeedsAuth))
}

/// 上下文明细里「MCP tools」的 token 数(没有明细时为 `None`)。
pub fn tool_tokens(usage: &Usage) -> Option<u64> {
    usage.context_categories.iter().find(|c| c.name.to_lowercase().contains("mcp tools")).map(|c| c.tokens).filter(|n| *n > 0)
}

/// 状态点颜色:已停用灰;已启用按连接状态着色,还没信息的用次要色。
fn dot_color(r: &McpRow, cx: &App) -> Hsla {
    let t = cx.theme();
    if !r.enabled {
        return t.muted_foreground.opacity(0.35);
    }
    match r.conn {
        McpConn::Connected => t.success,
        McpConn::Pending | McpConn::NeedsAuth => t.warning,
        McpConn::Failed => t.danger,
        McpConn::Unknown => t.muted_foreground.opacity(0.6),
    }
}

/// 行右侧的说明:非正常状态说状态,否则说工具数。
fn status_text(r: &McpRow) -> Option<(String, bool)> {
    if !r.enabled {
        return None;
    }
    match r.conn {
        McpConn::Failed => Some(("连接失败".into(), true)),
        McpConn::NeedsAuth => Some(("需要授权".into(), true)),
        McpConn::Pending => Some(("连接中…".into(), false)),
        _ => r.tools.map(|n| (format!("{n} 个工具"), false)),
    }
}

/// MCP 弹层。
///
/// 参数:`rows` 各服务器;`usage` 上下文用量(取工具占用);`has_project` 有没有打开项目(没有就不能切换);
/// `running` 回合是否进行中(影响生效时机的说明);`left` 弹层左边距(对齐到 chip);`this` 输入框实体。
pub fn mcp_popup(rows: Vec<McpRow>, usage: &Usage, has_project: bool, running: bool, left: Pixels, this: Entity<Composer>, cx: &App) -> Div {
    let t = cx.theme().clone();
    let on = enabled_count(&rows);
    let total = rows.len();
    // 工具定义占上下文的比例:超过阈值用警告色并给出建议
    let ratio = tool_tokens(usage).filter(|_| usage.context_size > 0).map(|n| (n, n as f32 / usage.context_size as f32));
    let manage = this.clone();
    let list = rows.into_iter().enumerate().map(|(i, r)| {
        let (this, name, scope) = (this.clone(), r.name.clone(), r.scope);
        let toggleable = has_project && scope.is_some();
        let want = !r.enabled;
        let status = status_text(&r);
        h_flex().id(SharedString::from(format!("mcp-row-{i}"))).gap_2().px_2().h(px(32.)).items_center().rounded(px(6.)).hover(|d| d.bg(t.list_hover))
            .child(div().size(px(7.)).flex_none().rounded_full().bg(dot_color(&r, cx)))
            .child(div().flex_1().whitespace_nowrap().text_size(px(13.)).when(!r.enabled, |d| d.text_color(t.muted_foreground)).child(r.name.clone()))
            .when(r.needs_approval, |d| d.child(div().flex_none().text_size(px(11.)).text_color(t.warning).child("待批准")))
            .when_some(status, |d, (text, bad)| d.child(div().flex_none().text_size(px(11.)).text_color(if bad { t.danger } else { t.muted_foreground }).child(text)))
            .child(
                Switch::new(SharedString::from(format!("mcp-sw-{i}"))).checked(r.enabled).disabled(!toggleable)
                    .on_click(move |_, _, cx| {
                        if let Some(scope) = scope {
                            this.update(cx, |c, cx| c.toggle_mcp(&name, scope, want, cx));
                        }
                    }),
            )
    });
    let hint = if !has_project {
        "先打开一个项目才能切换"
    } else if running {
        "仅对此项目生效 · 本轮结束后重启生效"
    } else {
        "仅对此项目生效 · 下一条消息起生效"
    };
    v_flex().absolute().bottom(px(44.)).left(left).min_w(px(300.)).p_1().rounded(px(10.)).border_1().border_color(t.border).bg(t.popover).shadow_lg()
        .child(
            h_flex().px_2().pt_1().pb_0p5().gap_2().items_baseline()
                .child(div().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).child("MCP 服务器"))
                .child(div().flex_1())
                .child(div().text_size(px(11.)).text_color(t.muted_foreground).child(format!("已启用 {on} / {total}"))),
        )
        .when_some(ratio, |d, (n, r)| {
            let hot = r >= WARN_RATIO;
            d.child(
                div().px_2().pb_1().text_size(px(11.)).text_color(if hot { t.warning } else { t.muted_foreground })
                    .child(format!("工具定义占上下文 {} · {:.0}%{}", short_tokens(n), r * 100., if hot { ",关掉不用的可以腾出空间" } else { "" })),
            )
        })
        .child(div().h(px(1.)).my_0p5().bg(t.border))
        .child(v_flex().id("mcp-list").max_h(px(320.)).overflow_y_scroll().children(list).when(total == 0, |d| d.child(div().px_2().py_2().text_size(px(12.)).text_color(t.muted_foreground).child("当前没有配置 MCP 服务器"))))
        .child(div().h(px(1.)).my_0p5().bg(t.border))
        .child(
            h_flex().id("mcp-manage").gap_2().px_2().h(px(30.)).items_center().rounded(px(6.)).cursor_pointer().hover(|d| d.bg(t.list_hover))
                .on_click(move |_, _, cx| { manage.update(cx, |c, cx| c.open_mcp_settings(cx)); })
                .child(icon(IconName::Settings, 13., t.muted_foreground))
                .child(div().text_size(px(13.)).child("管理 MCP…")),
        )
        .child(div().px_2().pb_1().text_size(px(11.)).text_color(t.muted_foreground).child(hint))
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_config::McpSpec;

    fn cfg(name: &str, enabled: bool) -> McpServer {
        McpServer { name: name.into(), spec: McpSpec::default(), enabled, needs_approval: false, scope: Scope::User }
    }
    fn live(name: &str, conn: McpConn, tools: u32) -> McpServerStatus {
        McpServerStatus { name: name.into(), conn, tools }
    }

    #[::core::prelude::v1::test]
    fn 合并配置与会话状态_启用的在前() {
        let rows = build_rows(&[cfg("b", false), cfg("z", true), cfg("a", true)], &[live("a", McpConn::Connected, 5), live("b", McpConn::Connected, 9), live("plug", McpConn::Failed, 2)]);
        let names: Vec<_> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["a", "z", "b", "plug"]);
        assert_eq!((rows[0].conn, rows[0].tools), (McpConn::Connected, Some(5)));
        // 没启动过的服务器没有状态
        assert_eq!((rows[1].conn, rows[1].tools), (McpConn::Unknown, None));
        // 已停用的不显示进程里的旧状态
        assert_eq!((rows[2].enabled, rows[2].conn, rows[2].tools), (false, McpConn::Unknown, None));
        // 配置里没有的(插件带来的)只读
        assert_eq!((rows[3].scope, rows[3].conn), (None, McpConn::Failed));
    }

    #[::core::prelude::v1::test]
    fn 启用数与问题提示() {
        let rows = build_rows(&[cfg("a", true), cfg("b", true), cfg("c", false)], &[live("a", McpConn::Failed, 0)]);
        assert_eq!(enabled_count(&rows), 2);
        assert!(has_problem(&rows));
        // 已停用的失败服务器不算问题
        let rows = build_rows(&[cfg("a", false)], &[live("a", McpConn::Failed, 0)]);
        assert!(!has_problem(&rows));
    }

    #[::core::prelude::v1::test]
    fn 工具占用取上下文明细() {
        let mut u = Usage::default();
        assert_eq!(tool_tokens(&u), None);
        u.context_categories = vec![loom_core::ContextCategory { name: "MCP tools".into(), tokens: 42_500, kind: loom_core::ContextKind::Used }];
        assert_eq!(tool_tokens(&u), Some(42_500));
    }
}
