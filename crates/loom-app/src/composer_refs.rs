//! 输入框正文里的文件引用:从文件树、编辑器标签或访达拖进来的文件 / 文件夹,作为「原子行内标记」插在光标处,
//! 渲染成带文件图标的小胶囊,和文字一起编辑(光标可以移过它、退格整颗删除)。标记在文本里的内容是 `@路径`,
//! 所以发送、历史、`@` 提及解析都和手敲 `@路径` 完全一样。
//!
//! @author ixxxxoooo

use crate::composer::Composer;
use crate::file_icons::file_icon_img;
use gpui_kit::component::input::{InlineToken, InlineTokenContext};
use gpui_kit::component::{ActiveTheme, h_flex};
use gpui_kit::*;
use std::path::{Path, PathBuf};

/// 算出写进消息的路径:在 `root` 之下取相对路径,否则用绝对路径。
///
/// 参数:`path` 文件绝对路径;`root` 项目根(没有就用绝对路径)。
pub fn mention_for(path: &Path, root: Option<&str>) -> String {
    root.and_then(|r| path.strip_prefix(r).ok()).map(|p| p.to_string_lossy().to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| path.to_string_lossy().to_string())
}

/// 一个文件引用标记:文本是 `@路径`(含空白的路径会被拒绝,因为提及按空白切分),显示用文件名。
///
/// 参数:`path` 绝对路径(标记 id);`mention` 写进消息的路径。返回:`None` 表示路径不能作为提及。
pub fn file_token(path: &Path, mention: &str) -> Option<InlineToken> {
    if mention.is_empty() || mention.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return None;
    }
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| mention.to_string());
    Some(InlineToken::new(path.to_string_lossy().to_string(), format!("@{mention}")).with_label(name))
}

/// 标记的渲染:文件类型图标 + 文件名的圆角小胶囊(选中时描主题色边)。
pub fn token_view(ctx: &InlineTokenContext, _: &mut Window, cx: &mut App) -> AnyElement {
    let t = cx.theme();
    let (path, name) = (PathBuf::from(ctx.token().id().to_string()), ctx.token().label().to_string());
    let is_dir = path.is_dir();
    h_flex().h(px(22.)).px_1p5().gap_1().items_center().rounded(px(5.)).border_1().border_color(if ctx.is_selected() { t.primary } else { t.border }).bg(t.muted.opacity(0.7)).text_size(px(13.))
        .child(file_icon_img(&name, is_dir, false, 14.))
        .child(div().max_w(px(240.)).truncate().child(name))
        .into_any_element()
}

impl Composer {
    /// 把一个文件 / 文件夹作为引用标记插在光标处(替换选区),后面补一个空格并聚焦输入框。
    /// `mention` 为空时按当前项目根推算路径。
    pub(crate) fn insert_file_token(&mut self, path: PathBuf, mention: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let mention = mention.unwrap_or_else(|| {
            let root = cx.try_global::<crate::WorkbenchRef>().and_then(|r| r.0.upgrade()).and_then(|wb| wb.read(cx).active_project_path());
            mention_for(&path, root.as_deref())
        });
        let Some(token) = file_token(&path, &mention) else {
            // 带空格的路径做不成标记:退回成纯文字 `@路径`
            let cur = self.input.read(cx).value().to_string();
            let sep = if cur.is_empty() || cur.ends_with(' ') || cur.ends_with('\n') { "" } else { " " };
            self.set_text(&format!("{cur}{sep}@{mention} "), window, cx);
            return;
        };
        self.input.update(cx, |i, cx| {
            let _ = i.replace_with_token(token, window, cx);
            // 标记后补一个空格,接着打字不会粘在胶囊上
            i.insert(" ", window, cx);
            i.focus(window, cx);
        });
        cx.notify();
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
    fn 标记文本是_at_路径_显示名是文件名() {
        let t = file_token(Path::new("/p/src/a.rs"), "src/a.rs").unwrap();
        assert_eq!(t.text().as_ref(), "@src/a.rs");
        assert_eq!(t.label().as_ref(), "a.rs");
        assert_eq!(t.id().as_ref(), "/p/src/a.rs");
    }

    #[::core::prelude::v1::test]
    fn 带空白的路径不能做标记() {
        assert!(file_token(Path::new("/p/a b.rs"), "a b.rs").is_none());
        assert!(file_token(Path::new("/p/a.rs"), "").is_none());
    }
}
