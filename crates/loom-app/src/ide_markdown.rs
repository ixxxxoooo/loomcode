//! Markdown 文件的所见即所得视图(IDE 与 Agent 右侧面板共用)。
//!
//! 基于开源的 `gpui-kit-markdown-editor`(块级所见即所得:标题、列表、任务列表、引用、
//! 围栏代码、表格、链接与粗 / 斜 / 删除线 / 行内代码,键入 `# `、`- `、``` 等自动转换)。
//! 文本的唯一来源是源码编辑器 `editor`:所见即所得里的改动写回它(IDE 由此标记未保存,右侧面板直接落盘),
//! 在源码里改动则同步到所见即所得。注意:第一次在所见即所得里编辑后,整份文档会按规范 Markdown 重新输出。
//!
//! @author ixxxxoooo

use gpui_kit::component::input::{EditorState, InputEvent};
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use markdown_editor::{BlockEditor, BlockEditorEvent, BlockEditorState, EditOrigin};
use std::path::{Path, PathBuf};

/// Markdown 的显示模式。
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum MdMode {
    /// 所见即所得(默认)
    #[default]
    Preview,
    /// 纯源码编辑器
    Source,
    /// 左源码右所见即所得
    Split,
}

/// 是否 Markdown 文件(`.mdc` 是 Cursor 规则文件,本质是带 YAML 头的 Markdown)。
pub fn is_markdown(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| ["md", "markdown", "mdc"].iter().any(|m| e.eq_ignore_ascii_case(m)))
}

/// 开头的 YAML 前置元数据(`---` 包起来的头):返回它(含结尾的 `---` 行与换行)占的字节数,没有则为 0。
pub fn front_matter_len(text: &str) -> usize {
    let mut lines = text.split_inclusive('\n');
    match lines.next() {
        Some(first) if first.trim_end() == "---" => {}
        _ => return 0,
    }
    let mut len = text.split_inclusive('\n').next().map_or(0, str::len);
    for line in lines {
        len += line.len();
        if matches!(line.trim_end(), "---" | "...") {
            return len;
        }
    }
    // 没有结尾的 `---`:不是前置元数据
    0
}

/// 前置元数据里的「键: 值」行(顶层、单行值;列表与缩进内容并入上一个键的值,换行以空格连接),用来在预览顶部显示。
pub fn front_matter_fields(front: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for line in front.lines().skip(1) {
        let t = line.trim_end();
        if matches!(t, "---" | "...") {
            break;
        }
        if t.is_empty() {
            continue;
        }
        let indented = line.starts_with(char::is_whitespace) || t.trim_start().starts_with("- ");
        match t.split_once(':').filter(|(k, _)| !indented && !k.is_empty() && !k.contains(char::is_whitespace)) {
            Some((k, v)) => out.push((k.to_string(), v.trim().trim_matches('"').to_string())),
            None => {
                if let Some(last) = out.last_mut() {
                    if !last.1.is_empty() {
                        last.1.push(' ');
                    }
                    last.1.push_str(t.trim().trim_start_matches("- "));
                }
            }
        }
    }
    out
}

/// 一个 Markdown 文档的所见即所得视图。
pub struct MdDocView {
    /// 源码编辑器:文本的唯一来源
    pub editor: Entity<EditorState>,
    pub mode: MdMode,
    block: Entity<BlockEditorState>,
    pub(crate) scroll: ScrollHandle,
    /// 源码开头的 YAML 前置元数据(原样保存):预览里不当正文渲染(否则 `---` 与键值会被当成分隔线和标题),
    /// 在顶部显示成只读的属性块;所见即所得写回时原样接在正文前面
    front: String,
    /// 写回后同步保存到这个路径(右侧面板用;IDE 自己管保存时为 `None`)
    save_to: Option<PathBuf>,
    /// 分栏同步滚动:上次看到的 (源码已滚距离, 预览已滚距离)
    pub(crate) sync_last: (f32, f32),
    _subs: Vec<Subscription>,
}

impl MdDocView {
    /// 创建视图。`editor` 文本来源;`save_to` 写回后自动落盘的路径(IDE 传 `None`)。
    pub fn new(editor: Entity<EditorState>, save_to: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let scroll = ScrollHandle::new();
        let text = editor.read(cx).value().to_string();
        let front_len = front_matter_len(&text);
        let (front, body) = (text[..front_len].to_string(), text[front_len..].to_string());
        let block = cx.new(|cx| {
            let mut s = BlockEditorState::new(window, cx).with_source(&body);
            s.set_scroll_handle(scroll.clone());
            s
        });
        // 所见即所得里的用户编辑 → 写回源码编辑器(撤销 / 重做与程序化载入不回写,避免循环)
        let to_editor = cx.subscribe_in(&block, window, |this: &mut MdDocView, block, ev: &BlockEditorEvent, window, cx| {
            let BlockEditorEvent::Changed { origin, .. } = ev else { return };
            if matches!(origin, EditOrigin::Api) {
                return;
            }
            let src = format!("{}{}", this.front, block.read(cx).source());
            if this.editor.read(cx).value().as_ref() == src.as_str() {
                return;
            }
            if let Some(path) = &this.save_to {
                let _ = std::fs::write(path, src.as_bytes());
            }
            this.editor.update(cx, |e, cx| e.replace_all(src, window, cx));
        });
        // 源码里的改动 → 同步到所见即所得(内容相同就跳过,所以不会来回触发)
        let to_block = cx.subscribe(&editor, |this: &mut MdDocView, editor, ev: &InputEvent, cx| {
            if !matches!(ev, InputEvent::Change) {
                return;
            }
            let text = editor.read(cx).value().to_string();
            let n = front_matter_len(&text);
            let body = &text[n..];
            if this.front != text[..n] {
                this.front = text[..n].to_string();
                cx.notify();
            }
            if this.block.read(cx).source() != body {
                this.block.update(cx, |b, cx| { let _ = b.set_source(body, cx); });
            }
        });
        // 分栏时,源码滚动 / 光标变化要让本视图重绘一次,好把滚动同步给预览
        let follow = cx.observe(&editor, |this: &mut MdDocView, _, cx| {
            if this.mode == MdMode::Split {
                cx.notify();
            }
        });
        Self { editor, mode: MdMode::Preview, block, scroll, front, save_to, sync_last: (0., 0.), _subs: vec![to_editor, to_block, follow] }
    }

    /// 切换显示模式。
    pub fn set_mode(&mut self, mode: MdMode, cx: &mut Context<Self>) {
        self.mode = mode;
        cx.notify();
    }
}

impl Render for MdDocView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.mode == MdMode::Split {
            self.sync_scroll(cx);
        }
        div().id("md-wysiwyg").flex_1().min_w_0().h_full().overflow_y_scroll().track_scroll(&self.scroll)
            .child(
                div().w_full().max_w(px(860.)).mx_auto().min_h_full()
                    .when(!self.front.is_empty(), |d| d.child(front_card(&self.front, cx)))
                    .child(BlockEditor::new(&self.block).min_h_full().px_6().py_4()),
            )
            .bg(cx.theme().background)
    }
}

/// 预览顶部的前置元数据属性块(只读;要改就切到源码)。
fn front_card(front: &str, cx: &App) -> impl IntoElement {
    let t = cx.theme();
    let fields = front_matter_fields(front);
    v_flex().mx_6().mt_4().mb_1().px_3().py_2().gap_1().rounded(px(8.)).border_1().border_color(t.border).bg(t.muted.opacity(0.4)).text_size(px(12.))
        .children(fields.into_iter().map(|(k, v)| {
            h_flex().gap_2().items_start()
                .child(div().flex_none().min_w(px(88.)).text_color(t.muted_foreground).child(k))
                .child(div().flex_1().min_w_0().child(if v.is_empty() { "—".to_string() } else { v }))
        }))
}

/// 所见即所得 / 源码 / 分栏切换按钮组(IDE 用)。
///
/// 不再单独占一行:作为 `ide_chrome::breadcrumbs` 的 `extra` 放在面包屑行右侧。
pub fn mode_bar<T: 'static>(view: &Entity<MdDocView>, cur: MdMode, cx: &mut Context<T>) -> AnyElement {
    let t = cx.theme().clone();
    h_flex().flex_none().gap_0p5().items_center()
        .children([(MdMode::Preview, "预览"), (MdMode::Source, "源码"), (MdMode::Split, "分栏")].into_iter().map(|(m, label)| {
            let v = view.clone();
            let active = m == cur;
            div().id(SharedString::from(format!("md-mode-{label}"))).px_2().h(px(20.)).flex().items_center().rounded(px(4.)).cursor_pointer().text_size(px(12.))
                .when(active, |d| d.bg(t.muted).text_color(t.foreground))
                .when(!active, |d| d.text_color(t.muted_foreground).hover(|d| d.text_color(t.foreground)))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| { v.update(cx, |v, cx| v.set_mode(m, cx)); })
                .child(label)
        })).into_any_element()
}

#[cfg(test)]
mod front_tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 前置元数据的范围与字段() {
        let text = "---\ndescription: 规则说明\nglobs: \"**/*.go\"\nalwaysApply: false\n---\n# 标题\n";
        let n = front_matter_len(text);
        assert_eq!(&text[n..], "# 标题\n");
        let f = front_matter_fields(&text[..n]);
        assert_eq!(f, vec![("description".into(), "规则说明".into()), ("globs".into(), "**/*.go".into()), ("alwaysApply".into(), "false".into())]);
        // 没有结尾 / 不在开头:不算
        assert_eq!(front_matter_len("---\na: 1\n正文"), 0);
        assert_eq!(front_matter_len("# 标题\n---\na: 1\n---\n"), 0);
        // 列表值并入上一个键
        let list = "---\ntags:\n  - a\n  - b\n---\n";
        assert_eq!(front_matter_fields(&list[..front_matter_len(list)]), vec![("tags".into(), "a b".into())]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn recognizes_markdown_extensions() {
        assert!(is_markdown(Path::new("a/B.MD")));
        assert!(is_markdown(Path::new("x.markdown")));
        assert!(is_markdown(Path::new(".cursor/rules/a.mdc")));
        assert!(!is_markdown(Path::new("x.rs")));
    }
}
