//! 输入框里的文件引用:从文件树、编辑器标签或访达拖进来的文件 / 文件夹,显示成带文件图标的小胶囊(悬停出现 ×),
//! 发送时自动写成 `@相对路径` 提及放在消息前面。胶囊不占输入框里的文字,删掉胶囊就是取消引用。
//!
//! @author ixxxxoooo

use crate::composer::Composer;
use crate::file_icons::file_icon_img;
use crate::ui::icon;
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme, h_flex};
use gpui_kit::*;
use std::path::{Path, PathBuf};

/// 一个待发送的文件引用。
#[derive(Clone, Debug, PartialEq)]
pub struct FileRef {
    /// 绝对路径
    pub path: PathBuf,
    /// 写进消息里的路径:在项目里是相对项目根的路径,否则是绝对路径
    pub mention: String,
    pub is_dir: bool,
}

/// 算出写进消息的路径:在 `root` 之下取相对路径,否则用绝对路径。
///
/// 参数:`path` 文件绝对路径;`root` 项目根(没有就用绝对路径)。
pub fn mention_for(path: &Path, root: Option<&str>) -> String {
    root.and_then(|r| path.strip_prefix(r).ok()).map(|p| p.to_string_lossy().to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| path.to_string_lossy().to_string())
}

/// 把待发送的引用写成消息前缀:每个引用 `@路径 `。
pub fn prefix_for(refs: &[FileRef]) -> String {
    refs.iter().map(|r| format!("@{} ", r.mention)).collect()
}

impl Composer {
    /// 添加一个文件 / 文件夹引用(已有同一路径的不重复)。`mention` 为空时按当前项目根推算。
    pub(crate) fn add_file_ref(&mut self, path: PathBuf, mention: Option<String>, cx: &mut Context<Self>) {
        if self.file_refs.iter().any(|r| r.path == path) {
            return;
        }
        let mention = mention.unwrap_or_else(|| {
            let root = cx.try_global::<crate::WorkbenchRef>().and_then(|r| r.0.upgrade()).and_then(|wb| wb.read(cx).active_project_path());
            mention_for(&path, root.as_deref())
        });
        let is_dir = path.is_dir();
        self.file_refs.push(FileRef { path, mention, is_dir });
        cx.notify();
    }

    /// 文件引用胶囊行(没有引用返回 `None`)。
    pub(crate) fn file_chips(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.file_refs.is_empty() {
            return None;
        }
        let t = cx.theme().clone();
        let this = cx.entity().downgrade();
        let chips = self.file_refs.iter().enumerate().map(|(i, r)| {
            let name = r.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| r.mention.clone());
            let group = SharedString::from(format!("fref-g-{i}"));
            let this = this.clone();
            h_flex().id(("fref", i)).group(group.clone()).h(px(24.)).pl_1p5().pr_1().gap_1p5().items_center().rounded(px(6.)).border_1().border_color(t.border).bg(t.muted.opacity(0.6)).text_size(px(12.5))
                .tooltip({ let m = r.mention.clone(); move |window, cx| gpui_kit::component::tooltip::Tooltip::new(m.clone()).build(window, cx) })
                .child(file_icon_img(&name, r.is_dir, false, 14.))
                .child(div().max_w(px(220.)).truncate().child(name))
                .child(
                    div().id(("fref-x", i)).size(px(16.)).flex().items_center().justify_center().rounded(px(4.)).cursor_pointer().hover(|d| d.bg(t.border))
                        .invisible().group_hover(group, |d| d.visible())
                        .on_click(move |_, _, cx| { let _ = this.update(cx, |c, cx| { if i < c.file_refs.len() { c.file_refs.remove(i); } cx.notify(); }); })
                        .child(icon(IconName::X, 10., t.muted_foreground)),
                )
        });
        Some(h_flex().flex_wrap().gap_1p5().px_3().pt_3().children(chips).into_any_element())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 项目内取相对路径_项目外用绝对路径() {
        assert_eq!(mention_for(Path::new("/p/src/a.rs"), Some("/p")), "src/a.rs");
        assert_eq!(mention_for(Path::new("/other/a.rs"), Some("/p")), "/other/a.rs");
        assert_eq!(mention_for(Path::new("/p/a.rs"), None), "/p/a.rs");
        assert_eq!(mention_for(Path::new("/p"), Some("/p")), "/p");
    }

    #[::core::prelude::v1::test]
    fn 前缀按顺序写成提及() {
        let r = |m: &str| FileRef { path: PathBuf::from(m), mention: m.into(), is_dir: false };
        assert_eq!(prefix_for(&[r("a.rs"), r("src/b.rs")]), "@a.rs @src/b.rs ");
        assert_eq!(prefix_for(&[]), "");
    }
}
