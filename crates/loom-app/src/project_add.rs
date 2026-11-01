//! 添加项目:最近项目、打开文件夹、新建文件夹、拖入文件夹。
//!
//! @author ixxxxoooo

use crate::workbench::Workbench;
use gpui_kit::assets::IconName;
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::*;
use std::path::{Path, PathBuf};
use loom_core::project_name_from_path;

/// 添加菜单里最多列出的最近项目数。
const MAX_RECENT: usize = 6;

impl Workbench {
    /// 弹出系统「新建」面板,创建一个新文件夹并作为项目打开。
    pub fn new_folder_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let base = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
        let rx = cx.prompt_for_new_path(&base, Some("新项目"));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(path))) = rx.await else { return };
            // 面板可能返回带扩展名的路径,必须有名字才创建
            if path.file_name().is_none() {
                return;
            }
            if let Err(e) = std::fs::create_dir_all(&path) {
                eprintln!("创建项目文件夹失败:{e}");
                return;
            }
            let _ = this.update_in(cx, |this, window, cx| this.register_project(path.to_string_lossy().to_string(), window, cx));
        })
        .detach();
    }

    /// 处理拖入侧栏的路径:只接受文件夹,逐个加入项目。
    ///
    /// 参数:`paths` 拖入的路径(文件会被忽略)。
    pub fn drop_projects(&mut self, paths: &[PathBuf], window: &mut Window, cx: &mut Context<Self>) {
        for p in paths.iter().filter(|p| p.is_dir()) {
            self.register_project(p.to_string_lossy().to_string(), window, cx);
        }
    }
}

/// 构造「添加」下拉菜单:最近项目 + 打开文件夹 + 新建文件夹。
///
/// 参数:`wb` 工作台弱引用;`recent` 最近使用的项目目录(调用方已过滤掉不存在的)。
pub fn build_menu(wb: WeakEntity<Workbench>, recent: Vec<String>) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
    move |mut menu, _, _| {
        if !recent.is_empty() {
            menu = menu.item(PopupMenuItem::label("最近项目"));
            for path in recent.iter().take(MAX_RECENT) {
                let (w, p) = (wb.clone(), path.clone());
                let item = PopupMenuItem::new(project_name_from_path(path)).icon(IconName::Folder).on_click(move |_, window, cx| {
                    let _ = w.update(cx, |this, cx| this.register_project(p.clone(), window, cx));
                });
                menu = menu.item(item);
            }
            menu = menu.separator();
        }
        let w = wb.clone();
        let open = PopupMenuItem::new("打开文件夹…").icon(IconName::FolderOpen).on_click(move |_, window, cx| {
            let _ = w.update(cx, |this, cx| this.add_project(window, cx));
        });
        let w = wb.clone();
        let create = PopupMenuItem::new("新建文件夹…").icon(IconName::FolderPlus).on_click(move |_, window, cx| {
            let _ = w.update(cx, |this, cx| this.new_folder_project(window, cx));
        });
        let w = wb.clone();
        let workspace = PopupMenuItem::new("添加工作区…").icon(IconName::Folders).on_click(move |_, window, cx| {
            let _ = w.update(cx, |this, cx| this.new_workspace_dialog(window, cx));
        });
        menu.item(open).item(create).separator().item(workspace)
    }
}

/// 从设置的最近项目里挑出仍存在的目录。会读磁盘,调用量很小(最多 12 项)。
pub fn existing_recent(recent: &[String]) -> Vec<String> {
    recent.iter().filter(|p| Path::new(p).is_dir()).cloned().collect()
}
