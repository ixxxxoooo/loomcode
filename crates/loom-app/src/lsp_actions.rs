//! 编辑器右键菜单里的语言服务器动作(对标 VS Code):转到类型定义、格式化文档。
//! 菜单项本身在 `ide_menu.rs`;这里是动作的实现(向服务器发请求,结果落回编辑器)。
//!
//! @author ixxxxoooo

use crate::ide::IdeView;
use crate::lsp::{DocSync, definition_links, fix_pos, fix_range, split_lines, to_lsp_pos};
use anyhow::{Result, anyhow};
use gpui_kit::component::RopeExt as _;
use gpui_kit::component::Rope;
use gpui_kit::*;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;

/// 把一组 LSP 文本编辑(范围已换成字符列)应用到原文,返回新文本。编辑按位置从后往前套,互不影响。
///
/// 参数:`full` 原文;`rope` 同一份文本的 Rope(换算偏移用);`edits` 服务器给的编辑。返回:新文本;
/// 范围越界或互相重叠的编辑返回错误。
pub fn apply_edits(full: &str, rope: &Rope, edits: &[lsp_types::TextEdit]) -> Result<String> {
    let mut spans: Vec<(usize, usize, &str)> = edits
        .iter()
        .map(|e| (rope.position_to_offset(&e.range.start), rope.position_to_offset(&e.range.end), e.new_text.as_str()))
        .collect();
    spans.sort_by_key(|s| (s.0, s.1));
    if spans.windows(2).any(|w| w[0].1 > w[1].0) {
        return Err(anyhow!("服务器返回的编辑互相重叠"));
    }
    let mut out = full.to_string();
    for (start, end, text) in spans.into_iter().rev() {
        if start > end || end > out.len() || !out.is_char_boundary(start) || !out.is_char_boundary(end) {
            return Err(anyhow!("服务器返回的编辑范围越界"));
        }
        out.replace_range(start..end, text);
    }
    Ok(out)
}

impl IdeView {
    /// 当前标签的语言服务器句柄、全文、Rope 与光标偏移;没有服务器返回 `None`。
    pub(crate) fn lsp_context_pub(&self, cx: &App) -> Option<(PathBuf, Arc<DocSync>, String, Rope, usize)> {
        self.lsp_context(cx)
    }

    /// 同 `lsp_context`。
    fn lsp_context(&self, cx: &App) -> Option<(PathBuf, Arc<DocSync>, String, Rope, usize)> {
        let path = self.tabs.active()?.to_path_buf();
        let doc = self.docs.get(&path)?;
        let sync = doc.lsp.clone()?;
        let ed = doc.editor.read(cx);
        Some((path, sync, ed.value().to_string(), ed.text().clone(), ed.selected_range().start))
    }

    /// 当前文档的语言服务器是否支持某项能力(右键菜单决定灰不灰)。
    pub(crate) fn lsp_supports(&self, capability: &str, cx: &App) -> bool {
        self.lsp_context(cx).is_some_and(|(_, sync, ..)| sync.client.supports(capability))
    }

    /// 转到光标处符号的类型定义(右键菜单)。
    pub(crate) fn go_to_type_definition(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.goto_request("textDocument/typeDefinition", "类型定义", window, cx);
    }

    /// 转到光标处符号的实现(右键菜单)。
    pub(crate) fn go_to_implementation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.goto_request("textDocument/implementation", "实现", window, cx);
    }

    /// 向语言服务器发一个「转到…」请求(类型定义 / 实现),取第一个结果跳转;`what` 用在提示里。
    fn goto_request(&mut self, method: &'static str, what: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        let Some((_, sync, full, rope, offset)) = self.lsp_context(cx) else { return };
        cx.spawn_in(window, async move |this, cx| {
            let res: Result<Option<(PathBuf, u32, u32)>> = cx
                .background_spawn(async move {
                    let lines = split_lines(&full);
                    let pos = to_lsp_pos(&rope, offset, &lines);
                    sync.sync(&full);
                    let rx = sync.client.request(method, json!({ "textDocument": { "uri": sync.uri }, "position": pos }));
                    let v = rx.recv().await.map_err(|e| anyhow!("{e}"))?.map_err(|e| anyhow!(e))?;
                    if v.is_null() {
                        return Ok(None);
                    }
                    let links = definition_links(serde_json::from_value(v)?);
                    let Some(mut link) = links.into_iter().next() else { return Ok(None) };
                    let same = link.target_uri.as_str() == sync.uri;
                    let path = loom_lsp::uri::to_path(link.target_uri.as_str()).ok_or_else(|| anyhow!("目标不是本地文件"))?;
                    let other = (!same).then(|| std::fs::read_to_string(&path).ok()).flatten();
                    let target_lines = if same { split_lines(&full) } else { other.as_deref().map(split_lines).unwrap_or_default() };
                    fix_range(&mut link.target_selection_range, &target_lines);
                    let start = link.target_selection_range.start;
                    Ok(Some((path, start.line, start.character)))
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| match res {
                Ok(Some((path, line, col))) => this.open_at(path, line, col, window, cx),
                Ok(None) => this.show_cursor_message(format!("未找到{what}"), cx),
                Err(e) => crate::notice::error(window, cx, format!("转到{what}失败:{e}")),
            });
        })
        .detach();
    }

    /// 用语言服务器格式化整个文档(右键菜单 / ⇧⌥F)。
    pub(crate) fn format_document(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((path, sync, full, rope, _)) = self.lsp_context(cx) else { return };
        let tab = crate::prefs(cx).editor.tab_size;
        cx.spawn_in(window, async move |this, cx| {
            let (full2, rope2) = (full.clone(), rope.clone());
            let res: Result<Option<String>> = cx
                .background_spawn(async move {
                    sync.sync(&full2);
                    let rx = sync.client.request("textDocument/formatting", json!({ "textDocument": { "uri": sync.uri }, "options": { "tabSize": tab, "insertSpaces": true } }));
                    let v = rx.recv().await.map_err(|e| anyhow!("{e}"))?.map_err(|e| anyhow!(e))?;
                    if v.is_null() {
                        return Ok(None);
                    }
                    let mut edits: Vec<lsp_types::TextEdit> = serde_json::from_value(v)?;
                    let lines = split_lines(&full2);
                    for e in &mut edits {
                        fix_pos(&mut e.range.start, &lines);
                        fix_pos(&mut e.range.end, &lines);
                    }
                    Ok(Some(apply_edits(&full2, &rope2, &edits)?))
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                match res {
                    // 格式化期间文档又被改过就放弃,免得覆盖用户的输入
                    Ok(Some(new)) if this.docs.get(&path).is_some_and(|d| d.editor.read(cx).value().as_ref() == full.as_str()) => {
                        if new != full {
                            if let Some(doc) = this.docs.get(&path) {
                                doc.editor.update(cx, |e, cx| e.replace_all(new, window, cx));
                            }
                        } else {
                            crate::notice::info(window, cx, "文档已经是格式化好的");
                        }
                    }
                    Ok(Some(_)) => crate::notice::warning(window, cx, "格式化期间文档被修改,已放弃本次结果"),
                    Ok(None) => crate::notice::info(window, cx, "语言服务器没有返回格式化结果"),
                    Err(e) => crate::notice::error(window, cx, format!("格式化失败:{e}")),
                }
            });
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(sl: u32, sc: u32, el: u32, ec: u32, text: &str) -> lsp_types::TextEdit {
        lsp_types::TextEdit { range: lsp_types::Range::new(lsp_types::Position::new(sl, sc), lsp_types::Position::new(el, ec)), new_text: text.into() }
    }

    #[::core::prelude::v1::test]
    fn 多处编辑从后往前套() {
        let src = "fn  a( ){}\nfn b(){}\n";
        let rope = Rope::from(src);
        let out = apply_edits(src, &rope, &[edit(0, 2, 0, 4, " "), edit(1, 4, 1, 5, "() ")]).unwrap();
        assert_eq!(out, "fn a( ){}\nfn b() ){}\n");
    }

    #[::core::prelude::v1::test]
    fn 重叠与越界的编辑被拒绝() {
        let src = "abcdef\n";
        let rope = Rope::from(src);
        assert!(apply_edits(src, &rope, &[edit(0, 0, 0, 4, "x"), edit(0, 2, 0, 5, "y")]).is_err());
    }
}
