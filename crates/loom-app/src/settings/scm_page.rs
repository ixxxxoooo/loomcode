//! 源代码管理页:改动面板的 diff 上限,以及 LoomCode 创建的 Git worktree 位置与列表。
//!
//! @author ixxxxoooo

use super::widgets::{group, item, note, page_header, row, rows, stepper};
use crate::ui::{MONO, caption};
use crate::workbench::Workbench;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Sizable, v_flex};
use gpui_kit::*;

/// 渲染源代码管理页。
pub fn render(wb: &Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    let t = cx.theme().clone();
    let root = wb.worktrees_root();
    let shown = loom_config::diag::redact_home(&root.display().to_string());
    // worktree 目录结构:<根>/<项目名>/<分支目录>
    let mut found: Vec<(String, String)> = Vec::new();
    if let Ok(projects) = std::fs::read_dir(&root) {
        for p in projects.flatten().filter(|e| e.path().is_dir()) {
            if let Ok(branches) = std::fs::read_dir(p.path()) {
                for b in branches.flatten().filter(|e| e.path().is_dir()) {
                    found.push((format!("{} / {}", p.file_name().to_string_lossy(), b.file_name().to_string_lossy()), b.path().display().to_string()));
                }
            }
        }
    }
    found.sort();
    let list: Vec<AnyElement> = if found.is_empty() {
        rows![item(cx).h(px(44.)).child(caption("还没有由 LoomCode 创建的 worktree", cx))]
    } else {
        found.into_iter().enumerate().map(|(i, (name, path))| {
            let reveal = path.clone();
            item(cx).min_h(px(44.)).gap_3().py_1p5()
                .child(v_flex().flex_1().min_w_0()
                    .child(div().text_size(px(13.)).child(name))
                    .child(div().truncate().font_family(MONO).text_size(px(11.)).text_color(t.muted_foreground).child(loom_config::diag::redact_home(&path))))
                .child(Button::new(("wt-reveal", i)).xsmall().ghost().label("在 Finder 中显示")
                    .on_click(move |_, _, _| { let _ = std::process::Command::new("open").arg("-R").arg(&reveal).spawn(); }))
                .into_any_element()
        }).collect()
    };
    v_flex().w_full()
        .child(page_header("源代码管理", "改动面板与 Git worktree 的行为", div(), cx))
        .child(group("改动面板", rows![
            row("Diff 大小上限", "超过这个大小的文件在改动面板里不计算 diff,避免卡顿", stepper("scm-diff", wb.settings.scm.diff_max_kb as i64, (256, 8192, 256), " KB", cx,
                |this, v, cx| this.update_settings(cx, |s| s.scm.diff_max_kb = v as u32)), cx),
        ], cx))
        .child(group("Worktree", rows![
            row("存放位置", &shown, Button::new("wt-open").small().outline().label("打开目录").on_click({
                let root = root.clone();
                move |_, _, _| { let _ = std::fs::create_dir_all(&root); let _ = std::process::Command::new("open").arg(&root).spawn(); }
            }), cx),
        ], cx))
        .child(group("已创建的 worktree", list, cx))
        .child(note("在侧边栏项目菜单里创建与移除 worktree;这里只读列出目录", cx))
        .into_any_element()
}
