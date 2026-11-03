//! 输入框的 MCP 选择框:点开看当前项目用了哪些 MCP 服务器(连接状态、工具数、占用),一键开关,
//! 展开一个服务器可以逐个开关它的工具。
//!
//! - **服务器开关**只记在**当前项目**下(用户层与本地层的服务器写 `~/.claude.json` 里该项目的 `disabledMcpServers`,
//!   项目层 `.mcp.json` 的写 `settings.local.json`),持久、不影响别的项目;同时用 `mcp_toggle` 控制请求**热切换**
//!   已启动的会话(实测立即生效、不用重启),后端没启动的会话启动时读配置。
//! - **工具开关**写成本地层 `permissions.deny` 的 `mcp__<服务器>__<工具>` 规则,claude 实时重读设置文件,工具随即从
//!   模型可用列表里消失(实测)。来自别的层(用户 / 项目)的禁用规则只读,去设置里改。
//!
//! 数据来源:配置(`Workbench.claude_cfg`)给名称、层、启用与否和禁用规则;会话里 `mcp_status` / `system init`
//! 给连接状态、错误与工具列表(`SessionState.mcp`,进程没启动时没有);`get_context_usage` 的 `mcpTools` 给各工具的
//! token 占用与是否已加载(按需加载的不占窗口)。点开弹层时刷新一次(`RefreshMcp`,不查账户限额)。
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
use loom_config::{McpServer, Rule, Scope};
use loom_core::{McpConn, McpServerStatus, McpToolUsage, Usage, mcp_norm_name};

/// 工具定义占上下文超过这个比例就提醒。
pub const WARN_RATIO: f32 = 0.10;

/// 一个工具。
#[derive(Debug, Clone, PartialEq)]
pub struct ToolRow {
    pub name: String,
    /// 完整定义占的 token(没有明细为 0)
    pub tokens: u64,
    /// 完整定义已加载进上下文;`false` = 按需加载
    pub loaded: bool,
    /// 被 `permissions.deny` 禁用
    pub disabled: bool,
    /// 能不能在这里切换:禁用规则不在本地层时只读
    pub toggleable: bool,
}

/// 弹层里的一行(一个服务器)。
#[derive(Debug, Clone, PartialEq)]
pub struct McpRow {
    pub name: String,
    /// 工具名里的写法(`mcp__<写法>__<工具>`)
    pub norm: String,
    /// 配置所在的层;`None` 表示配置里找不到(插件、claude.ai 连接器带来的),只能看不能切
    pub scope: Option<Scope>,
    pub enabled: bool,
    /// 项目层服务器还没被批准
    pub needs_approval: bool,
    /// 本会话里的连接状态(没启动、或已被停用时为 `Unknown`)
    pub conn: McpConn,
    /// 连接失败的原因(第一行)
    pub error: Option<String>,
    /// 本会话里它提供的工具(进程没启动时为空)
    pub tools: Vec<ToolRow>,
    /// 是否有会话里的信息(区分「0 个工具」与「还不知道」)
    pub live: bool,
}

impl McpRow {
    /// 已加载进上下文的 token 数。
    pub fn loaded_tokens(&self) -> u64 {
        self.tools.iter().filter(|t| t.loaded && !t.disabled).map(|t| t.tokens).sum()
    }

    /// 按需加载(暂不占窗口)的 token 数。
    pub fn deferred_tokens(&self) -> u64 {
        self.tools.iter().filter(|t| !t.loaded && !t.disabled).map(|t| t.tokens).sum()
    }
}

/// 合并配置、会话状态、占用明细与禁用规则:先是配置里的服务器(按名字排,切换启停时位置不动),再是会话里有、配置里没有的。
///
/// 参数:`cfg` 各层配置里的服务器;`status` 本会话报的状态;`usage` 各工具占用;`deny` 各层的禁用规则。
pub fn build_rows(cfg: &[McpServer], status: &[McpServerStatus], usage: &[McpToolUsage], deny: &[Rule]) -> Vec<McpRow> {
    let live = |name: &str| status.iter().find(|s| s.name == name);
    let tools_of = |name: &str, tools: &[String]| -> Vec<ToolRow> {
        let norm = mcp_norm_name(name);
        let mut list: Vec<ToolRow> = tools
            .iter()
            .map(|t| {
                let rule = format!("mcp__{norm}__{t}");
                let denied = deny.iter().filter(|r| r.text == rule);
                let (disabled, local_only) = (denied.clone().next().is_some(), denied.clone().all(|r| r.scope == Scope::Local));
                let u = usage.iter().find(|u| u.server == norm && &u.tool == t);
                ToolRow { name: t.clone(), tokens: u.map_or(0, |u| u.tokens), loaded: u.is_none_or(|u| u.loaded), disabled, toggleable: !disabled || local_only }
            })
            .collect();
        list.sort_by(|a, b| a.name.cmp(&b.name));
        list
    };
    let mut rows: Vec<McpRow> = cfg
        .iter()
        .map(|s| {
            // 已停用的服务器:进程里可能还连着,但对用户来说它已经关了,不显示旧状态
            let l = live(&s.name).filter(|_| s.enabled);
            McpRow {
                name: s.name.clone(),
                norm: mcp_norm_name(&s.name),
                scope: Some(s.scope),
                enabled: s.enabled,
                needs_approval: s.needs_approval,
                conn: l.map(|l| l.conn).unwrap_or_default(),
                error: l.and_then(|l| l.error.clone()),
                tools: l.map(|l| tools_of(&s.name, &l.tools)).unwrap_or_default(),
                live: l.is_some(),
            }
        })
        .collect();
    // 按名字排:开关切换时行不能跳位置(不按启用与否分组)
    rows.sort_by(|a, b| a.name.cmp(&b.name));
    for s in status {
        if !cfg.iter().any(|c| c.name == s.name) {
            rows.push(McpRow {
                name: s.name.clone(),
                norm: mcp_norm_name(&s.name),
                scope: None,
                enabled: s.conn != McpConn::Disabled,
                needs_approval: false,
                conn: s.conn,
                error: s.error.clone(),
                tools: tools_of(&s.name, &s.tools),
                live: true,
            });
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

/// 上下文明细里指定类别的 token 数(`deferred` 取「按需加载」那一类);没有明细时为 `None`。
fn category_tokens(usage: &Usage, deferred: bool) -> Option<u64> {
    usage
        .context_categories
        .iter()
        .find(|c| {
            let n = c.name.to_lowercase();
            // 真实名称是 `MCP tools` / `MCP tools (deferred)`,演示数据是中文
            let is_tools = n.contains("mcp") && (n.contains("tools") || n.contains("工具")) && !n.contains("instruction");
            is_tools && (n.contains("deferred") || n.contains("按需")) == deferred
        })
        .map(|c| c.tokens)
}

/// 已加载进上下文的 MCP 工具定义占用。
pub fn tool_tokens(usage: &Usage) -> Option<u64> {
    category_tokens(usage, false)
}

/// 按需加载(不占窗口)的 MCP 工具定义大小。
pub fn deferred_tokens(usage: &Usage) -> Option<u64> {
    category_tokens(usage, true).filter(|n| *n > 0)
}

/// 状态点颜色:已停用灰;已启用按连接状态着色,还没信息的用次要色。
fn dot_color(r: &McpRow, cx: &App) -> Hsla {
    let t = cx.theme();
    if !r.enabled || r.conn == McpConn::Disabled {
        return t.muted_foreground.opacity(0.35);
    }
    match r.conn {
        McpConn::Connected => t.success,
        McpConn::Pending | McpConn::NeedsAuth => t.warning,
        McpConn::Failed => t.danger,
        _ => t.muted_foreground.opacity(0.6),
    }
}

/// 行右侧的说明:`(文字, 是否异常)`。非正常状态说状态,正常的说工具数与占用(已加载)或「按需加载」。
fn status_text(r: &McpRow) -> Option<(String, bool)> {
    if !r.enabled {
        return None;
    }
    match r.conn {
        McpConn::Failed => Some((r.error.clone().unwrap_or_else(|| "连接失败".into()), true)),
        McpConn::NeedsAuth => Some(("需要授权".into(), true)),
        McpConn::Pending => Some(("连接中…".into(), false)),
        _ if r.live => {
            let n = r.tools.iter().filter(|t| !t.disabled).count();
            let size = match (r.loaded_tokens(), r.deferred_tokens()) {
                (l, _) if l > 0 => format!(" · {}", short_tokens(l)),
                (_, d) if d > 0 => " · 按需加载".to_string(),
                _ => String::new(),
            };
            Some((format!("{n} 个工具{size}"), false))
        }
        _ => None,
    }
}

/// MCP 弹层。
///
/// 参数:`rows` 各服务器;`open` 展开的服务器名;`usage` 上下文用量(取工具占用);`has_project` 有没有打开项目(没有就不能切换);
/// `left` 弹层左边距(对齐到 chip);`this` 输入框实体。
pub fn mcp_popup(rows: Vec<McpRow>, open: Option<&str>, usage: &Usage, has_project: bool, left: Pixels, this: Entity<Composer>, cx: &App) -> Div {
    let t = cx.theme().clone();
    let on = enabled_count(&rows);
    let total = rows.len();
    // 工具定义占上下文的比例:超过阈值用警告色并给出建议;按需加载的另外说明(不占窗口)
    let ratio = tool_tokens(usage).filter(|_| usage.context_size > 0).map(|n| (n, n as f32 / usage.context_size as f32));
    let deferred = deferred_tokens(usage);
    let manage = this.clone();
    let mut list: Vec<AnyElement> = Vec::new();
    for (i, r) in rows.into_iter().enumerate() {
        let expandable = !r.tools.is_empty();
        let expanded = expandable && open == Some(r.name.as_str());
        let (this_sw, this_open, name, scope) = (this.clone(), this.clone(), r.name.clone(), r.scope);
        let name_open = r.name.clone();
        let toggleable = has_project && scope.is_some();
        let want = !r.enabled;
        let status = status_text(&r);
        list.push(
            h_flex().id(SharedString::from(format!("mcp-row-{i}"))).gap_1p5().pr_2().pl_1().h(px(32.)).items_center().rounded(px(6.)).hover(|d| d.bg(t.list_hover))
                .child(
                    div().id(SharedString::from(format!("mcp-exp-{i}"))).size(px(18.)).flex_none().flex().items_center().justify_center().rounded(px(4.))
                        .when(expandable, |d| d.cursor_pointer().hover(|d| d.bg(t.foreground.opacity(0.08))).on_click(move |_, _, cx| { this_open.update(cx, |c, cx| c.toggle_mcp_open(&name_open, cx)); }))
                        .when(expandable, |d| d.child(icon(if expanded { IconName::ChevronDown } else { IconName::ChevronRight }, 12., t.muted_foreground))),
                )
                .child(div().size(px(7.)).flex_none().rounded_full().bg(dot_color(&r, cx)))
                .child(div().flex_1().whitespace_nowrap().text_size(px(13.)).when(!r.enabled, |d| d.text_color(t.muted_foreground)).child(r.name.clone()))
                .when(r.needs_approval, |d| d.child(div().flex_none().text_size(px(11.)).text_color(t.warning).child("待批准")))
                .when_some(status, |d, (text, bad)| d.child(div().flex_none().max_w(px(220.)).truncate().text_size(px(11.)).text_color(if bad { t.danger } else { t.muted_foreground }).child(text)))
                .child(
                    Switch::new(SharedString::from(format!("mcp-sw-{i}"))).checked(r.enabled).disabled(!toggleable)
                        .on_click(move |_, _, cx| {
                            if let Some(scope) = scope {
                                this_sw.update(cx, |c, cx| c.toggle_mcp(&name, scope, want, cx));
                            }
                        }),
                )
                .into_any_element(),
        );
        if expanded {
            for (j, tool) in r.tools.iter().enumerate() {
                let (this_t, norm, tname) = (this.clone(), r.norm.clone(), tool.name.clone());
                let want_tool = tool.disabled;
                let size = if tool.disabled { "已禁用".to_string() } else if tool.loaded && tool.tokens > 0 { short_tokens(tool.tokens) } else if !tool.loaded { "按需".to_string() } else { String::new() };
                list.push(
                    h_flex().id(SharedString::from(format!("mcp-tool-{i}-{j}"))).gap_2().pl(px(34.)).pr_2().h(px(26.)).items_center().rounded(px(6.)).hover(|d| d.bg(t.list_hover))
                        .child(div().flex_1().min_w_0().truncate().text_size(px(12.)).font_family(crate::ui::MONO).when(tool.disabled, |d| d.text_color(t.muted_foreground)).child(tool.name.clone()))
                        .child(div().flex_none().text_size(px(11.)).text_color(t.muted_foreground).child(size))
                        .child(
                            Switch::new(SharedString::from(format!("mcp-tsw-{i}-{j}"))).checked(!tool.disabled).disabled(!has_project || !tool.toggleable)
                                .on_click(move |_, _, cx| { this_t.update(cx, |c, cx| c.toggle_mcp_tool(&norm, &tname, want_tool, cx)); }),
                        )
                        .into_any_element(),
                );
            }
        }
    }
    let hint = if !has_project { "先打开一个项目才能切换" } else { "仅对此项目生效 · 立即生效,无需重启" };
    v_flex().absolute().bottom(px(44.)).left(left).min_w(px(340.)).p_1().rounded(px(10.)).border_1().border_color(t.border).bg(t.popover).shadow_lg()
        .child(
            h_flex().px_2().pt_1().pb_0p5().gap_2().items_baseline()
                .child(div().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).child("MCP 服务器"))
                .child(div().flex_1())
                .child(div().text_size(px(11.)).text_color(t.muted_foreground).child(format!("已启用 {on} / {total}"))),
        )
        .when_some(ratio, |d, (n, r)| {
            let hot = r >= WARN_RATIO;
            let extra = deferred.map(|x| format!(" · 另有 {} 按需加载,不占窗口", short_tokens(x))).unwrap_or_default();
            d.child(
                div().px_2().pb_1().text_size(px(11.)).text_color(if hot { t.warning } else { t.muted_foreground })
                    .child(format!("工具定义占上下文 {} · {:.0}%{}{}", short_tokens(n), r * 100., extra, if hot { ",关掉不用的可以腾出空间" } else { "" })),
            )
        })
        .child(div().h(px(1.)).my_0p5().bg(t.border))
        .child(v_flex().id("mcp-list").max_h(px(360.)).overflow_y_scroll().children(list).when(total == 0, |d| d.child(div().px_2().py_2().text_size(px(12.)).text_color(t.muted_foreground).child("当前没有配置 MCP 服务器"))))
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
    fn live(name: &str, conn: McpConn, tools: &[&str]) -> McpServerStatus {
        McpServerStatus { name: name.into(), conn, tools: tools.iter().map(|t| t.to_string()).collect(), error: None }
    }
    fn deny(text: &str, scope: Scope) -> Rule {
        Rule { text: text.into(), scope }
    }

    #[::core::prelude::v1::test]
    fn 合并配置与会话状态_按名字排且不随启停跳动() {
        let rows = build_rows(&[cfg("b", false), cfg("z", true), cfg("a", true)], &[live("a", McpConn::Connected, &["x"]), live("b", McpConn::Connected, &["y"]), live("plug", McpConn::Failed, &[])], &[], &[]);
        let names: Vec<_> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["a", "b", "z", "plug"]);
        assert_eq!((rows[0].conn, rows[0].tools.len(), rows[0].live), (McpConn::Connected, 1, true));
        // 没启动过的服务器没有状态
        assert_eq!((rows[2].conn, rows[2].live), (McpConn::Unknown, false));
        // 已停用的不显示进程里的旧状态
        assert_eq!((rows[1].enabled, rows[1].conn, rows[1].live), (false, McpConn::Unknown, false));
        // 配置里没有的(插件带来的)只读
        assert_eq!((rows[3].scope, rows[3].conn), (None, McpConn::Failed));
    }

    #[::core::prelude::v1::test]
    fn 启用数与问题提示() {
        let rows = build_rows(&[cfg("a", true), cfg("b", true), cfg("c", false)], &[live("a", McpConn::Failed, &[])], &[], &[]);
        assert_eq!(enabled_count(&rows), 2);
        assert!(has_problem(&rows));
        // 已停用的失败服务器不算问题
        let rows = build_rows(&[cfg("a", false)], &[live("a", McpConn::Failed, &[])], &[], &[]);
        assert!(!has_problem(&rows));
    }

    #[::core::prelude::v1::test]
    fn 工具行带占用_按需加载与禁用规则() {
        let usage = [
            McpToolUsage { server: "claude_ai_Docs".into(), tool: "read".into(), tokens: 300, loaded: true },
            McpToolUsage { server: "claude_ai_Docs".into(), tool: "write".into(), tokens: 500, loaded: false },
        ];
        let rules = [deny("mcp__claude_ai_Docs__delete", Scope::Local), deny("mcp__claude_ai_Docs__admin", Scope::User)];
        let rows = build_rows(&[cfg("claude.ai Docs", true)], &[live("claude.ai Docs", McpConn::Connected, &["write", "read", "delete", "admin"])], &usage, &rules);
        let t = &rows[0].tools;
        assert_eq!(t.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), ["admin", "delete", "read", "write"]);
        // 已加载 300、按需 500;被禁用的不计
        assert_eq!((rows[0].loaded_tokens(), rows[0].deferred_tokens()), (300, 500));
        // 本地层规则可在这里切回来,用户层规则只读
        assert_eq!((t[0].disabled, t[0].toggleable), (true, false));
        assert_eq!((t[1].disabled, t[1].toggleable), (true, true));
        assert_eq!((t[2].disabled, t[2].loaded), (false, true));
        assert_eq!(status_text(&rows[0]).map(|s| s.0), Some("2 个工具 · 300".to_string()));
    }

    #[::core::prelude::v1::test]
    fn 工具占用取上下文明细() {
        let mut u = Usage::default();
        assert_eq!(tool_tokens(&u), None);
        let cat = |name: &str, tokens, kind| loom_core::ContextCategory { name: name.into(), tokens, kind };
        u.context_categories = vec![cat("MCP tools (deferred)", 46_000, loom_core::ContextKind::Deferred), cat("MCP tools", 831, loom_core::ContextKind::Used)];
        assert_eq!((tool_tokens(&u), deferred_tokens(&u)), (Some(831), Some(46_000)));
    }
}
