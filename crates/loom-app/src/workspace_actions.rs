//! 多根工作区:新建 / 编辑对话框、保存、工作目录映射。
//!
//! @author ixxxxoooo

use crate::ui::dialog_footer;
use crate::workbench::Workbench;
use crate::workspace_form::WorkspaceForm;
use gpui_kit::component::WindowExt as _;
use gpui_kit::*;
use std::cell::Cell;
use std::path::PathBuf;
use loom_core::WorkspaceDef;

impl Workbench {
    /// 工作区专属目录的父目录(`~/.loomcode/workspaces`,演示模式在沙盒里)。
    fn workspaces_root(&self) -> PathBuf {
        self.settings_path.with_file_name("workspaces")
    }

    /// 项目(会话 cwd)对应的「真实工作根」:普通项目就是自己,工作区取第一个文件夹。
    /// 文件树、CLAUDE.md、权限规则等都应以它为准,因为工作区专属目录本身是空的。
    pub fn work_root(&self, project_path: &str) -> String {
        self.ui_state.workspace_by_dir(project_path).and_then(|w| w.folders.first().cloned()).unwrap_or_else(|| project_path.to_string())
    }

    /// 项目的全部「真实工作根」:工作区是它的所有文件夹(第一个是主目录),普通项目只有自己。
    /// 资源管理器、源代码管理、文件监视与项目内搜索都覆盖这些目录(对标 VS Code 多根工作区)。
    pub fn work_roots(&self, project_path: &str) -> Vec<String> {
        match self.ui_state.workspace_by_dir(project_path) {
            Some(w) if !w.folders.is_empty() => w.folders.clone(),
            _ => vec![project_path.to_string()],
        }
    }

    /// 项目是工作区时返回工作区名称。
    pub fn workspace_name(&self, project_path: &str) -> Option<String> {
        self.ui_state.workspace_by_dir(project_path).map(|w| w.name.clone())
    }

    /// 当前项目的工作区名称(不是工作区为 `None`)。
    pub fn active_workspace_name(&self) -> Option<String> {
        self.current_project_ix().and_then(|ix| self.projects.get(ix)).and_then(|p| self.workspace_name(&p.path))
    }

    /// 当前项目的全部工作根(规则见 `work_roots`)。
    pub fn active_work_roots(&self) -> Option<Vec<String>> {
        self.current_project_ix().and_then(|ix| self.projects.get(ix)).map(|p| self.work_roots(&p.path))
    }

    /// 打开新建工作区对话框。
    pub fn new_workspace_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = uuid::Uuid::new_v4().to_string();
        let dir = self.workspaces_root().join(&id).to_string_lossy().to_string();
        let def = WorkspaceDef { id, name: "新工作区".into(), dir, folders: Vec::new() };
        self.open_workspace_dialog(def, true, window, cx);
    }

    /// 打开编辑对话框;`dir` 不是工作区时忽略。
    pub fn edit_workspace_dialog(&mut self, dir: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(def) = self.ui_state.workspace_by_dir(dir).cloned() {
            self.open_workspace_dialog(def, false, window, cx);
        }
    }

    fn open_workspace_dialog(&mut self, def: WorkspaceDef, is_new: bool, window: &mut Window, cx: &mut Context<Self>) {
        let form = cx.new(|cx| WorkspaceForm::new(def, window, cx));
        let wb = cx.entity().downgrade();
        let first = Cell::new(true);
        window.open_dialog(cx, move |dialog, window, cx| {
            if first.replace(false) {
                form.read(cx).name_input().focus_handle(cx).focus(window, cx);
            }
            let (form, wb) = (form.clone(), wb.clone());
            dialog
                .title(if is_new { "新建工作区" } else { "编辑工作区" })
                .width(px(460.))
                .child(form.clone())
                .footer(dialog_footer("保存"))
                .on_ok(move |_, window, cx| {
                    // 校验不过就留在对话框里,错误显示在表单中
                    let Some(def) = form.update(cx, |f, cx| f.validate(cx)) else { return false };
                    let _ = wb.update(cx, |this, cx| this.save_workspace(def, is_new, window, cx));
                    true
                })
        });
    }

    /// 保存工作区:创建专属目录、写入 `state.json`、让它出现在侧栏;新建时直接开一个会话。
    pub fn save_workspace(&mut self, def: WorkspaceDef, is_new: bool, window: &mut Window, cx: &mut Context<Self>) {
        if let Err(e) = std::fs::create_dir_all(&def.dir) {
            loom_diag::error!("创建工作区目录失败:{e}");
            return;
        }
        let dir = def.dir.clone();
        self.ui_state.unhide_project(&dir);
        self.ui_state.upsert_workspace(def);
        let owners = self.view_owners();
        self.ui_state.apply_workspaces(&mut self.projects);
        self.remap_views(&owners);
        self.persist_ui_state();
        // 编辑时文件夹可能增删:资源管理器 / 源代码管理随之更新
        self.sync_panel(cx);
        if is_new {
            if let Some(ix) = self.projects.iter().position(|p| p.path == dir) {
                self.new_session(ix, window, cx);
            }
        }
        cx.notify();
    }
}
