//! 文件树的拖放(参考 OhMyAgent):把文件拖到输入框 = 引用 `@路径`;拖到文件夹 / 空白处 = 移动到那里;
//! 从访达拖进来 = 复制。
//!
//! @author ixxxxoooo

use crate::right_panel::RightPanel;
use gpui_kit::component::{ActiveTheme, h_flex};
use gpui_kit::*;
use std::path::{Path, PathBuf};

/// 正在拖动的文件树条目:跟随鼠标的浮标,同时携带路径。
#[derive(Clone)]
pub struct DraggedPath {
    pub path: PathBuf,
    /// 相对项目根的路径(拖进输入框时写成 `@相对路径`)
    pub rel: String,
}

impl Render for DraggedPath {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme();
        let name = self.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        h_flex().gap_1p5().items_center().px_3().py_1().rounded(px(6.)).border_1().border_color(t.border).bg(t.background).text_size(px(12.5)).shadow_md().child(crate::file_icons::file_icon_img(&name, self.path.is_dir(), false, 14.)).child(name)
    }
}

/// 递归复制文件或目录到 `dest`(目标不存在)。
pub fn copy_recursive(src: &Path, dest: &Path) -> std::io::Result<()> {
    if src.is_dir() {
        std::fs::create_dir_all(dest)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            copy_recursive(&entry.path(), &dest.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(src, dest).map(|_| ())
    }
}

/// 复制到同一目录时的目标名:`名称 副本.扩展名`,已存在就 `名称 副本 2.扩展名`…(对标 VS Code / Finder 的「创建副本」)。
pub fn unique_copy_dest(src: &Path, dir: &Path) -> PathBuf {
    let name = src.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let plain = dir.join(&name);
    if !plain.exists() {
        return plain;
    }
    let (stem, ext) = match src.extension() {
        Some(e) if !src.is_dir() => (src.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(), format!(".{}", e.to_string_lossy())),
        _ => (name.clone(), String::new()),
    };
    let mut n = 1;
    loop {
        let label = if n == 1 { "副本".to_string() } else { format!("副本 {n}") };
        let cand = dir.join(format!("{stem} {label}{ext}"));
        if !cand.exists() {
            return cand;
        }
        n += 1;
    }
}

/// 规划一次放置:`copy` 为真是复制(外部拖入),否则是移动。返回目标路径或不能执行的原因。
///
/// 参数:`src` 被拖的路径;`dir` 落点文件夹。
pub fn plan_drop(src: &Path, dir: &Path) -> Result<PathBuf, String> {
    let name = src.file_name().ok_or("无效的路径")?;
    let dest = dir.join(name);
    if dest == src {
        return Err(String::new()); // 原地放回,静默忽略
    }
    if dir.starts_with(src) {
        return Err("不能把文件夹移到它自身或子文件夹里".into());
    }
    if dest.exists() {
        return Err(format!("目标文件夹里已有同名项目「{}」", name.to_string_lossy()));
    }
    Ok(dest)
}

impl RightPanel {
    /// 把文件树剪贴板里的条目粘贴到 `dir`:复制遇到同名自动起「副本」名,剪切是移动(成功后清空剪贴板)。
    pub(crate) fn paste_files(&mut self, dir: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let Some((sources, cut)) = self.file_clip.clone() else { return };
        if cut {
            self.drop_into(&sources, dir, false, window, cx);
            self.file_clip = None;
            return;
        }
        let mut failures = Vec::new();
        for src in &sources {
            if dir.starts_with(src) {
                failures.push(format!("不能把文件夹复制到它自身或子文件夹里:{}", src.display()));
                continue;
            }
            let dest = unique_copy_dest(src, dir);
            if let Err(e) = copy_recursive(src, &dest) {
                failures.push(format!("{}:{e}", src.display()));
            }
        }
        if !failures.is_empty() {
            crate::notice::error(window, cx, failures.join("\n"));
        }
        self.files.expand_dirs(&[dir.to_string_lossy().to_string()]);
        self.reload_files(cx);
        cx.notify();
    }

    /// 把一批路径放进 `dir`;`copy` 为真复制,否则移动。成功后刷新文件树。
    pub(crate) fn drop_into(&mut self, sources: &[PathBuf], dir: &Path, copy: bool, window: &mut Window, cx: &mut Context<Self>) {
        let mut failures = Vec::new();
        for src in sources {
            match plan_drop(src, dir) {
                Ok(dest) => {
                    let res = if copy { copy_recursive(src, &dest) } else { std::fs::rename(src, &dest) };
                    if let Err(e) = res {
                        failures.push(format!("{}:{e}", src.display()));
                    }
                }
                Err(e) if e.is_empty() => {}
                Err(e) => failures.push(e),
            }
        }
        if !failures.is_empty() {
            crate::notice::error(window, cx, failures.join("\n"));
        }
        self.files.expand_dirs(&[dir.to_string_lossy().to_string()]);
        self.reload_files(cx);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 副本命名() {
        let dir = std::env::temp_dir().join(format!("loom-copy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("d")).unwrap();
        std::fs::write(dir.join("a.txt"), "x").unwrap();
        assert_eq!(unique_copy_dest(&dir.join("a.txt"), &dir.join("d")), dir.join("d/a.txt"));
        assert_eq!(unique_copy_dest(&dir.join("a.txt"), &dir), dir.join("a 副本.txt"));
        std::fs::write(dir.join("a 副本.txt"), "x").unwrap();
        assert_eq!(unique_copy_dest(&dir.join("a.txt"), &dir), dir.join("a 副本 2.txt"));
        assert_eq!(unique_copy_dest(&dir.join("d"), &dir), dir.join("d 副本"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[::core::prelude::v1::test]
    fn 放置规划() {
        let dir = std::env::temp_dir().join(format!("loom-dnd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("a/b")).unwrap();
        std::fs::create_dir_all(dir.join("c")).unwrap();
        std::fs::write(dir.join("a/f.txt"), "x").unwrap();
        std::fs::write(dir.join("c/f.txt"), "y").unwrap();
        // 放进自己的子目录:拒绝
        assert!(plan_drop(&dir.join("a"), &dir.join("a/b")).unwrap_err().contains("自身"));
        // 同名冲突:拒绝
        assert!(plan_drop(&dir.join("a/f.txt"), &dir.join("c")).unwrap_err().contains("同名"));
        // 原地放回:静默
        assert_eq!(plan_drop(&dir.join("a/f.txt"), &dir.join("a")).unwrap_err(), "");
        // 正常
        assert_eq!(plan_drop(&dir.join("a/f.txt"), &dir.join("a/b")).unwrap(), dir.join("a/b/f.txt"));
        // 递归复制
        copy_recursive(&dir.join("a"), &dir.join("c/a2")).unwrap();
        assert!(dir.join("c/a2/b").is_dir() && dir.join("c/a2/f.txt").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
