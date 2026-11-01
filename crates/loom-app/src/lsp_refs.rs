//! 右键菜单里更重的语言服务器动作:转到引用、重命名符号、代码操作(重构 / 源代码操作),
//! 以及它们共用的「工作区编辑」应用(改多个文件:已打开的写回编辑器,没打开的直接写盘)。
//!
//! @author ixxxxoooo

use crate::ide::IdeView;
use crate::ide_overlay::{ActionItem, Overlay};
use crate::ide_peek::{PeekState, RawRef};
use crate::lsp::{split_lines, to_lsp_pos, fix_pos, fix_range};
use crate::lsp_actions::apply_edits;
use anyhow::{Result, anyhow};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme as _, Rope, RopeExt as _, WindowExt as _};
use gpui_kit::*;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::PathBuf;

/// 引用列表最多收多少条。
const MAX_REFS: usize = 500;

/// 把 `text` 里所有整词等于 `old` 的出现换成 `new`(词字符 = 字母数字与下划线)。
///
/// 参数:`text` 原文;`old` 旧词;`new` 新词。返回:新文本。
pub fn replace_word(text: &str, old: &str, new: &str) -> String {
    if old.is_empty() {
        return text.to_string();
    }
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let (mut out, mut rest, mut prev) = (String::with_capacity(text.len()), text, None::<char>);
    while let Some(i) = rest.find(old) {
        let before = rest[..i].chars().next_back().or(prev);
        let after = rest[i + old.len()..].chars().next();
        out.push_str(&rest[..i]);
        if before.is_some_and(is_word) || after.is_some_and(is_word) {
            out.push_str(old);
        } else {
            out.push_str(new);
        }
        prev = old.chars().next_back();
        rest = &rest[i + old.len()..];
    }
    out.push_str(rest);
    out
}

impl IdeView {
    fn notify_ui(window: &mut Window, cx: &mut App, n: Notification) {
        window.push_notification(n, cx);
    }

    /// 查找光标处符号的所有引用:一处直接跳转,多处弹出引用预览窗(对标 VS Code 的 Peek References)。
    pub(crate) fn find_references(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((_, sync, full, rope, offset)) = self.lsp_context_pub(cx) else { return };
        cx.spawn_in(window, async move |this, cx| {
            let res: Result<Vec<RawRef>> = cx
                .background_spawn(async move {
                    let lines = split_lines(&full);
                    let pos = to_lsp_pos(&rope, offset, &lines);
                    sync.sync(&full);
                    let rx = sync.client.request("textDocument/references", json!({ "textDocument": { "uri": sync.uri }, "position": pos, "context": { "includeDeclaration": true } }));
                    let v = rx.recv().await.map_err(|e| anyhow!("{e}"))?.map_err(|e| anyhow!(e))?;
                    if v.is_null() {
                        return Ok(vec![]);
                    }
                    let locs: Vec<lsp_types::Location> = serde_json::from_value(v)?;
                    // 按文件分组:每个文件只读一次、只切一次行(以前每条引用都重切整份文件,引用多时很慢)
                    let mut by_file: std::collections::BTreeMap<String, Vec<lsp_types::Location>> = Default::default();
                    for l in locs.into_iter().take(MAX_REFS) {
                        by_file.entry(l.uri.to_string()).or_default().push(l);
                    }
                    let mut items = Vec::new();
                    for (key, group) in by_file {
                        let Some(path) = loom_lsp::uri::to_path(group[0].uri.as_str()) else { continue };
                        let text = if key == sync.uri { full.clone() } else { std::fs::read_to_string(&path).unwrap_or_default() };
                        let file_lines = split_lines(&text);
                        for l in group {
                            let (mut start, mut end) = (l.range.start, l.range.end);
                            fix_pos(&mut start, &file_lines);
                            fix_pos(&mut end, &file_lines);
                            let line_text = file_lines.get(start.line as usize).map(|s| s.to_string()).unwrap_or_default();
                            let end_col = if end.line == start.line { end.character } else { line_text.chars().count() as u32 };
                            items.push(RawRef { path: path.clone(), line: start.line, col: start.character, end_col, line_text });
                        }
                    }
                    Ok(items)
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| match res {
                Ok(items) if items.len() == 1 => {
                    let r = items[0].clone();
                    this.open_at(r.path, r.line, r.col, window, cx);
                }
                Ok(items) if !items.is_empty() => {
                    let st = PeekState::new(items, offset, cx.theme().is_dark(), cx);
                    this.set_overlay(Overlay::Refs(st), cx);
                }
                Ok(_) => this.set_overlay(Overlay::Refs(PeekState::new(vec![], offset, false, cx)), cx),
                Err(e) => Self::notify_ui(window, cx, Notification::error(format!("查找引用失败:{e}"))),
            });
        })
        .detach();
    }

    /// 打开重命名输入框:`local` 为真是「更改所有匹配项」。光标不在单词上时提示。
    pub(crate) fn begin_rename(&mut self, local: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.tabs.active().map(|p| p.to_path_buf()) else { return };
        let Some(doc) = self.docs.get(&path) else { return };
        let ed = doc.editor.read(cx);
        let (text, offset) = (ed.value().to_string(), ed.selected_range().start);
        let word = ed.text().word_range(offset).map(|r| text.get(r).unwrap_or("").to_string()).filter(|w| !w.is_empty());
        let Some(old) = word else {
            self.show_cursor_message("光标不在符号上,无法重命名", cx);
            return;
        };
        let input = cx.new(|cx| InputState::new(window, cx));
        input.update(cx, |i, cx| { i.set_value(old.clone(), window, cx); i.focus(window, cx); });
        self.overlay_sub = Some(cx.subscribe_in(&input, window, |this, input, ev: &InputEvent, window, cx| {
            if let InputEvent::PressEnter { .. } = ev {
                let name = input.read(cx).value().to_string();
                this.confirm_rename(name, window, cx);
            }
        }));
        self.set_overlay(Overlay::Rename { input, local, old }, cx);
    }

    /// 重命名输入框回车:本地替换或向语言服务器要工作区编辑。
    fn confirm_rename(&mut self, new_name: String, window: &mut Window, cx: &mut Context<Self>) {
        let new_name = new_name.trim().to_string();
        let Some(Overlay::Rename { local, old, .. }) = &self.overlay else { return };
        let (local, old) = (*local, old.clone());
        if new_name.is_empty() || new_name == old {
            self.close_overlay(window, cx);
            return;
        }
        if local {
            if let Some(doc) = self.tabs.active().and_then(|p| self.docs.get(p)) {
                let new = replace_word(doc.editor.read(cx).value().as_ref(), &old, &new_name);
                doc.editor.update(cx, |e, cx| e.replace_all(new, window, cx));
            }
            self.close_overlay(window, cx);
            return;
        }
        let Some((_, sync, full, rope, offset)) = self.lsp_context_pub(cx) else { return };
        self.close_overlay(window, cx);
        cx.spawn_in(window, async move |this, cx| {
            let res: Result<Value> = cx
                .background_spawn(async move {
                    let lines = split_lines(&full);
                    let pos = to_lsp_pos(&rope, offset, &lines);
                    sync.sync(&full);
                    let rx = sync.client.request("textDocument/rename", json!({ "textDocument": { "uri": sync.uri }, "position": pos, "newName": new_name }));
                    rx.recv().await.map_err(|e| anyhow!("{e}"))?.map_err(|e| anyhow!(e))
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| match res.and_then(|v| if v.is_null() { Err(anyhow!("服务器不允许在这里重命名")) } else { Ok(serde_json::from_value::<lsp_types::WorkspaceEdit>(v)?) }).and_then(|e| this.apply_workspace_edit(e, window, cx)) {
                Ok(n) => Self::notify_ui(window, cx, Notification::success(format!("已重命名,修改了 {n} 个文件"))),
                Err(e) => Self::notify_ui(window, cx, Notification::error(format!("重命名失败:{e}"))),
            });
        })
        .detach();
    }

    /// 应用一个工作区编辑:已打开的文件写回编辑器(留下未保存标记),没打开的直接写盘。返回改动的文件数。
    pub(crate) fn apply_workspace_edit(&mut self, edit: lsp_types::WorkspaceEdit, window: &mut Window, cx: &mut Context<Self>) -> Result<usize> {
        let mut per_file: HashMap<PathBuf, Vec<lsp_types::TextEdit>> = HashMap::new();
        for (uri, edits) in edit.changes.unwrap_or_default() {
            if let Some(p) = loom_lsp::uri::to_path(uri.as_str()) {
                per_file.entry(p).or_default().extend(edits);
            }
        }
        match edit.document_changes {
            Some(lsp_types::DocumentChanges::Edits(list)) => {
                for d in list {
                    if let Some(p) = loom_lsp::uri::to_path(d.text_document.uri.as_str()) {
                        per_file.entry(p).or_default().extend(d.edits.into_iter().map(|e| match e { lsp_types::OneOf::Left(t) => t, lsp_types::OneOf::Right(a) => a.text_edit }));
                    }
                }
            }
            Some(lsp_types::DocumentChanges::Operations(ops)) => {
                for op in ops {
                    if let lsp_types::DocumentChangeOperation::Edit(d) = op {
                        if let Some(p) = loom_lsp::uri::to_path(d.text_document.uri.as_str()) {
                            per_file.entry(p).or_default().extend(d.edits.into_iter().map(|e| match e { lsp_types::OneOf::Left(t) => t, lsp_types::OneOf::Right(a) => a.text_edit }));
                        }
                    }
                }
            }
            None => {}
        }
        if per_file.is_empty() {
            return Err(anyhow!("服务器没有返回任何编辑"));
        }
        let mut changed = 0;
        for (path, mut edits) in per_file {
            let open = self.docs.get(&path).map(|d| d.editor.clone());
            let text = match &open {
                Some(ed) => ed.read(cx).value().to_string(),
                None => std::fs::read_to_string(&path).map_err(|e| anyhow!("读取 {} 失败:{e}", path.display()))?,
            };
            let lines = split_lines(&text);
            for e in &mut edits {
                fix_range(&mut e.range, &lines);
            }
            let new = apply_edits(&text, &Rope::from(text.as_str()), &edits)?;
            if new == text {
                continue;
            }
            match open {
                Some(ed) => ed.update(cx, |e, cx| e.replace_all(new, window, cx)),
                None => std::fs::write(&path, new).map_err(|e| anyhow!("写入 {} 失败:{e}", path.display()))?,
            }
            changed += 1;
        }
        Ok(changed)
    }

    /// 向服务器要当前位置 / 选区的代码操作,按种类前缀(`refactor` / `source`)过滤后弹出列表。
    pub(crate) fn show_code_actions(&mut self, kind: &'static str, title: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        let Some((path, sync, full, rope, _)) = self.lsp_context_pub(cx) else { return };
        let Some(doc) = self.docs.get(&path) else { return };
        let sel = doc.editor.read(cx).selected_range();
        cx.spawn_in(window, async move |this, cx| {
            let res: Result<Vec<ActionItem>> = cx
                .background_spawn(async move {
                    let lines = split_lines(&full);
                    let (a, b) = (to_lsp_pos(&rope, sel.start, &lines), to_lsp_pos(&rope, sel.end, &lines));
                    sync.sync(&full);
                    let rx = sync.client.request("textDocument/codeAction", json!({ "textDocument": { "uri": sync.uri }, "range": { "start": a, "end": b }, "context": { "diagnostics": [], "only": [kind] } }));
                    let v = rx.recv().await.map_err(|e| anyhow!("{e}"))?.map_err(|e| anyhow!(e))?;
                    let list: Vec<Value> = if v.is_null() { vec![] } else { serde_json::from_value(v)? };
                    Ok(list.into_iter().filter_map(|raw| Some(ActionItem { title: raw.get("title")?.as_str()?.to_string(), raw })).collect())
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| match res {
                Ok(items) if !items.is_empty() => this.set_overlay(Overlay::Actions { title, items }, cx),
                Ok(_) => this.show_cursor_message(format!("没有可用的{title}"), cx),
                Err(e) => Self::notify_ui(window, cx, Notification::error(format!("获取{title}失败:{e}"))),
            });
        })
        .detach();
    }

    /// 执行代码操作列表的第 `i` 项:缺少编辑时先向服务器 resolve;只有命令没有编辑的暂不支持。
    pub(crate) fn run_action_item(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Overlay::Actions { items, .. }) = &self.overlay else { return };
        let Some(raw) = items.get(i).map(|a| a.raw.clone()) else { return };
        let Some((_, sync, ..)) = self.lsp_context_pub(cx) else { return };
        self.close_overlay(window, cx);
        cx.spawn_in(window, async move |this, cx| {
            let res: Result<Option<lsp_types::WorkspaceEdit>> = cx
                .background_spawn(async move {
                    let mut item = raw;
                    if item.get("edit").is_none() && item.get("data").is_some() {
                        let rx = sync.client.request("codeAction/resolve", item.clone());
                        item = rx.recv().await.map_err(|e| anyhow!("{e}"))?.map_err(|e| anyhow!(e))?;
                    }
                    Ok(match item.get("edit") { Some(e) => Some(serde_json::from_value(e.clone())?), None => None })
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| match res {
                Ok(Some(edit)) => match this.apply_workspace_edit(edit, window, cx) {
                    Ok(_) => {}
                    Err(e) => Self::notify_ui(window, cx, Notification::error(format!("应用失败:{e}"))),
                },
                Ok(None) => Self::notify_ui(window, cx, Notification::info("这个操作需要执行服务器命令,暂不支持")),
                Err(e) => Self::notify_ui(window, cx, Notification::error(format!("执行失败:{e}"))),
            });
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 整词替换不误伤更长的词() {
        assert_eq!(replace_word("foo foobar bar_foo foo(foo)", "foo", "x"), "x foobar bar_foo x(x)");
        assert_eq!(replace_word("名称 名称表 名称", "名称", "name"), "name 名称表 name");
        assert_eq!(replace_word("abc", "", "x"), "abc");
        assert_eq!(replace_word("foofoo foo", "foo", "z"), "foofoo z");
    }
}
