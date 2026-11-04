//! 大纲(对标 VS Code 资源管理器下方的「大纲」区):当前文件的符号树,点击跳到对应位置,光标所在的符号高亮。
//!
//! 数据来源:Markdown 直接按标题行解析(纯文本、同步);其他语言走语言服务器的 `textDocument/documentSymbol`
//! (后台请求,编辑后 0.5 秒防抖刷新)。没有服务器 / 服务器不支持时大纲为空,显示提示。
//!
//! @author ixxxxoooo

use crate::ide::IdeView;
use crate::ui::icon;
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_json::Value;
use std::rc::Rc;

/// 大纲里的一项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineItem {
    pub name: String,
    /// 补充说明(函数签名、类型等,服务器给的 `detail`)
    pub detail: String,
    /// LSP `SymbolKind` 数值;Markdown 标题固定为 0
    pub kind: u32,
    /// 0 起始的行 / 列
    pub line: u32,
    pub col: u32,
    /// 嵌套深度(0 = 顶层)
    pub depth: usize,
}

/// 把 `textDocument/documentSymbol` 的应答展平成带深度的列表(按出现顺序)。
/// 两种形态都支持:层级的 `DocumentSymbol[]`(有 `selectionRange` 与 `children`)与扁平的 `SymbolInformation[]`(有 `location`)。
pub fn parse_symbols(v: &Value) -> Vec<OutlineItem> {
    fn walk(items: &[Value], depth: usize, out: &mut Vec<OutlineItem>) {
        for it in items {
            let name = it.get("name").and_then(Value::as_str).unwrap_or("").to_string();
            if name.is_empty() {
                continue;
            }
            let pos = it.pointer("/selectionRange/start").or_else(|| it.pointer("/location/range/start"));
            let (line, col) = pos.map_or((0, 0), |p| (p.get("line").and_then(Value::as_u64).unwrap_or(0) as u32, p.get("character").and_then(Value::as_u64).unwrap_or(0) as u32));
            out.push(OutlineItem {
                name,
                detail: it.get("detail").and_then(Value::as_str).unwrap_or("").to_string(),
                kind: it.get("kind").and_then(Value::as_u64).unwrap_or(0) as u32,
                line,
                col,
                depth,
            });
            if let Some(children) = it.get("children").and_then(Value::as_array) {
                walk(children, depth + 1, out);
            }
        }
    }
    let mut out = Vec::new();
    if let Some(arr) = v.as_array() {
        walk(arr, 0, &mut out);
        // 扁平的 SymbolInformation 没有层级,按位置排序;层级形态保持服务器给的顺序
        if arr.first().is_some_and(|f| f.get("location").is_some()) {
            out.sort_by_key(|i| (i.line, i.col));
        }
    }
    out
}

/// Markdown 标题(ATX `#` 形式)→ 大纲项。跳过围栏代码块与 YAML 头(`---` 包起来的前置元数据)。
pub fn markdown_headings(text: &str) -> Vec<OutlineItem> {
    let mut out = Vec::new();
    let mut lines = text.lines().enumerate().peekable();
    // 开头的 YAML 前置元数据整段跳过
    if text.starts_with("---") {
        if let Some((_, first)) = lines.peek() && first.trim_end() == "---" {
            lines.next();
            for (_, l) in lines.by_ref() {
                if matches!(l.trim_end(), "---" | "...") {
                    break;
                }
            }
        }
    }
    let mut fence: Option<char> = None;
    for (ix, line) in lines {
        let t = line.trim_start();
        if let Some(c) = t.chars().next().filter(|c| *c == '`' || *c == '~') && t.starts_with(&c.to_string().repeat(3)) {
            fence = match fence {
                None => Some(c),
                Some(open) if open == c => None,
                other => other,
            };
            continue;
        }
        if fence.is_some() || line.len() - t.len() > 3 {
            continue;
        }
        let level = t.chars().take_while(|c| *c == '#').count();
        if !(1..=6).contains(&level) {
            continue;
        }
        let rest = &t[level..];
        // `#` 后必须是空白(`#hashtag` 不是标题);行尾的 `###` 闭合符去掉
        if !rest.starts_with(char::is_whitespace) {
            continue;
        }
        let title = rest.trim().trim_end_matches('#').trim_end().to_string();
        if title.is_empty() {
            continue;
        }
        out.push(OutlineItem { name: title, detail: String::new(), kind: 0, line: ix as u32, col: 0, depth: level - 1 });
    }
    // 深度按相对层级规整:跳级(# 之后直接 ###)只算比上一级深一层
    let mut stack: Vec<usize> = Vec::new();
    for it in &mut out {
        let level = it.depth;
        while stack.last().is_some_and(|l| *l >= level) {
            stack.pop();
        }
        it.depth = stack.len();
        stack.push(level);
    }
    out
}

/// 光标所在行对应的大纲项:起始行不超过光标行的最后一项(嵌套时取最深的那个,因为子项排在父项之后)。
pub fn active_index(items: &[OutlineItem], cursor_line: u32) -> Option<usize> {
    items.iter().rposition(|i| i.line <= cursor_line)
}

/// 符号种类 → 图标(LSP `SymbolKind`)。
pub(crate) fn kind_icon(kind: u32) -> IconName {
    match kind {
        // 模块 / 命名空间 / 包
        2..=4 => IconName::Boxes,
        // 类 / 接口 / 结构体 / 枚举 / 类型参数
        5 | 10 | 11 | 23 | 26 => IconName::Box,
        // 方法 / 构造 / 函数
        6 | 9 | 12 => IconName::Braces,
        // 属性 / 字段 / 变量 / 常量 / 枚举成员 / 事件 …
        0 | 7 | 8 | 13 | 14 | 22 | 24 => IconName::Hash,
        _ => IconName::Component,
    }
}

impl IdeView {
    /// 当前标签(跟随最近点击的编辑器组)的大纲;没有文件或没有可用的符号时为空。
    pub(crate) fn active_outline(&self, cx: &App) -> Rc<Vec<OutlineItem>> {
        if let Some(c) = self.focused_child() {
            return c.read(cx).active_outline(cx);
        }
        self.tabs.active().and_then(|p| self.docs.get(p)).map(|d| d.outline.clone()).unwrap_or_default()
    }

    /// 立即重新计算某文档的大纲。
    pub(crate) fn refresh_outline(&mut self, path: &std::path::Path, cx: &mut Context<Self>) {
        if crate::ide_untitled::is_untitled(path) {
            return;
        }
        let Some(doc) = self.docs.get_mut(path) else { return };
        doc.outline_gen += 1;
        let g = doc.outline_gen;
        let text = doc.editor.read(cx).value().to_string();
        if crate::ide_markdown::is_markdown(path) {
            doc.outline = Rc::new(markdown_headings(&text));
            cx.notify();
            return;
        }
        let Some(sync) = doc.lsp.clone().filter(|s| s.client.supports("documentSymbolProvider")) else {
            if !doc.outline.is_empty() {
                doc.outline = Default::default();
                cx.notify();
            }
            return;
        };
        let path = path.to_path_buf();
        cx.spawn(async move |this, cx| {
            let res: Result<Value, String> = cx
                .background_spawn(async move {
                    sync.sync(&text);
                    let rx = sync.client.request("textDocument/documentSymbol", serde_json::json!({ "textDocument": { "uri": sync.uri } }));
                    rx.recv().await.map_err(|e| e.to_string())?
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if let Some(doc) = this.docs.get_mut(&path).filter(|d| d.outline_gen == g) {
                    match res {
                        Ok(v) => doc.outline = Rc::new(parse_symbols(&v)),
                        Err(e) => loom_diag::debug!("大纲请求失败:{e}"),
                    }
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// 编辑后 0.5 秒刷新大纲(防抖)。
    pub(crate) fn schedule_outline(&mut self, path: &std::path::Path, cx: &mut Context<Self>) {
        if crate::ide_untitled::is_untitled(path) {
            return;
        }
        let Some(doc) = self.docs.get_mut(path) else { return };
        doc.outline_gen += 1;
        let (path, g) = (path.to_path_buf(), doc.outline_gen);
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(std::time::Duration::from_millis(500)).await;
            let _ = this.update(cx, |this, cx| {
                if this.docs.get(&path).is_some_and(|d| d.outline_gen == g) {
                    this.refresh_outline(&path, cx);
                }
            });
        })
        .detach();
    }
}

/// 大纲视图:读取编辑器当前标签的大纲,点击跳转。
pub struct OutlineView {
    editor: Entity<IdeView>,
    _sub: Subscription,
}

impl OutlineView {
    pub fn new(editor: Entity<IdeView>, cx: &mut Context<Self>) -> Self {
        // 标签切换、光标移动、大纲刷新都会让编辑器 notify;大纲是独立视图,重绘不牵连文件树
        let sub = cx.observe(&editor, |_, _, cx| cx.notify());
        Self { editor, _sub: sub }
    }
}

impl Render for OutlineView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let hint = |text: &'static str| div().p_3().text_size(px(12.)).text_color(t.muted_foreground).child(text).into_any_element();
        let (items, cursor) = {
            let group = self.editor.read(cx).focused_child().unwrap_or_else(|| self.editor.clone());
            let ed = group.read(cx);
            let Some(doc) = ed.tabs.active().filter(|p| !crate::ide_diff::is_diff(p)).and_then(|p| ed.docs.get(p)) else {
                return hint("没有打开的文件");
            };
            (doc.outline.clone(), doc.editor.read(cx).cursor_position().line)
        };
        if items.is_empty() {
            return hint("文件没有可用的大纲");
        }
        let active = active_index(&items, cursor);
        let editor = self.editor.clone();
        v_flex().id("outline-scroll").size_full().overflow_y_scroll().py_1()
            .children(items.iter().enumerate().map(|(i, it)| {
                let (line, col) = (it.line, it.col);
                let ed = editor.clone();
                h_flex().id(SharedString::from(format!("outline-{i}"))).h(px(22.)).mx_1().pr_2().gap_1p5().items_center().rounded(px(5.)).cursor_pointer().flex_none()
                    .pl(px(6. + it.depth as f32 * 12.))
                    .when(active == Some(i), |d| d.bg(t.list_active))
                    .hover(|d| d.bg(t.list_hover))
                    .on_click(move |_, window, cx| {
                        ed.update(cx, |v, cx| {
                            v.goto(line + 1, col + 1, window, cx);
                            v.focus_active(window, cx);
                        });
                    })
                    .child(icon(kind_icon(it.kind), 12., t.muted_foreground))
                    .child(div().min_w_0().truncate().text_size(px(12.5)).child(it.name.clone()))
                    .when(!it.detail.is_empty(), |d| d.child(div().flex_1().min_w_0().truncate().text_size(px(11.)).text_color(t.muted_foreground).child(it.detail.clone())))
            }))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[::core::prelude::v1::test]
    fn 层级符号展平并带深度() {
        let v = json!([
            { "name": "Server", "kind": 23, "selectionRange": { "start": { "line": 3, "character": 5 } }, "children": [
                { "name": "port", "kind": 8, "detail": "int", "selectionRange": { "start": { "line": 4, "character": 1 } } }
            ] },
            { "name": "main", "kind": 12, "selectionRange": { "start": { "line": 9, "character": 5 } } }
        ]);
        let items = parse_symbols(&v);
        assert_eq!(items.iter().map(|i| (i.name.as_str(), i.depth, i.line)).collect::<Vec<_>>(), [("Server", 0, 3), ("port", 1, 4), ("main", 0, 9)]);
        assert_eq!(items[1].detail, "int");
    }

    #[::core::prelude::v1::test]
    fn 扁平符号按位置排序() {
        let v = json!([
            { "name": "b", "kind": 12, "location": { "uri": "file:///x", "range": { "start": { "line": 8, "character": 0 } } } },
            { "name": "a", "kind": 12, "location": { "uri": "file:///x", "range": { "start": { "line": 2, "character": 0 } } } }
        ]);
        assert_eq!(parse_symbols(&v).iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), ["a", "b"]);
        assert!(parse_symbols(&Value::Null).is_empty());
    }

    #[::core::prelude::v1::test]
    fn markdown_标题跳过代码块与前置元数据() {
        let md = "---\ndescription: x\n# 不是标题\n---\n# 一级\n正文\n```sh\n# 注释不是标题\n```\n## 二级\n#标签不是标题\n#### 跳级 ##\n";
        let items = markdown_headings(md);
        assert_eq!(items.iter().map(|i| (i.name.as_str(), i.depth, i.line)).collect::<Vec<_>>(), [("一级", 0, 4), ("二级", 1, 9), ("跳级", 2, 11)]);
    }

    #[::core::prelude::v1::test]
    fn 光标所在的最近项() {
        let mk = |line| OutlineItem { name: "x".into(), detail: String::new(), kind: 0, line, col: 0, depth: 0 };
        let items = vec![mk(2), mk(10), mk(20)];
        assert_eq!(active_index(&items, 1), None);
        assert_eq!(active_index(&items, 10), Some(1));
        assert_eq!(active_index(&items, 99), Some(2));
    }
}
