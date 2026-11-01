//! 记忆 / 命令 / 子 Agent / Skills 页的 Workbench 方法:扫描缓存、打开与保存 Markdown、新建、移到废纸篓,
//! 以及把自定义命令推给各会话的输入框补全。
//!
//! @author ixxxxoooo

use super::CustomizeTab;
use super::forms::DocEdit;
use crate::composer::Composer;
use crate::workbench::Workbench;
use gpui_kit::component::input::{EditorState, InputEvent};
use gpui_kit::*;
use loom_config::Scope;
use loom_config::items::{self, Item, ItemKind};
use std::path::{Path, PathBuf};

impl Workbench {
    /// 重新扫描三类扩展与记忆文件(表单尚未创建时跳过)。
    pub fn refresh_items(&mut self) {
        let home = self.claude_home.clone();
        let project = self.claude_cfg_root.clone();
        let Some(forms) = self.settings_forms.as_mut() else { return };
        for kind in ItemKind::ALL {
            forms.items.insert(kind, items::list_items(&home, project.as_deref().map(Path::new), kind));
        }
        forms.memory_files = items::memory_files(&home, project.as_deref().map(Path::new));
    }

    /// 把某个目录下可用的自定义命令 / Skills / 子 Agent 推给输入框补全。
    ///
    /// 参数:`composer` 输入框;`cwd` 会话工作目录。
    pub fn push_extensions(&self, composer: &Entity<Composer>, cwd: &str, cx: &mut Context<Self>) {
        let project = Some(Path::new(cwd));
        let commands = items::slash_suggestions(&self.claude_home, project);
        let agents = items::list_items(&self.claude_home, project, ItemKind::Agent).into_iter().map(|i| i.name).collect();
        composer.update(cx, |c, _| c.set_extensions(commands, agents));
    }

    /// 扩展有增删改后,刷新所有已打开会话的补全。
    pub fn push_extensions_all(&self, cx: &mut Context<Self>) {
        let targets: Vec<(Entity<Composer>, String)> =
            self.views.values().filter_map(|v| self.projects.get(v.project_ix).map(|p| (v.composer.clone(), self.work_root(&p.path)))).collect();
        for (composer, cwd) in targets {
            self.push_extensions(&composer, &cwd, cx);
        }
    }

    /// 在编辑器里打开一个 Markdown 文件(不存在时为空,保存时新建)。
    ///
    /// 参数:`path` 文件;`tab` 打开它的自定义标签。
    pub fn open_doc(&mut self, path: PathBuf, tab: CustomizeTab, window: &mut Window, cx: &mut Context<Self>) {
        if self.forms().doc.as_ref().is_some_and(|d| d.path == path && d.tab == tab) {
            return;
        }
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let editor = cx.new(|cx| EditorState::new(window, cx).language("markdown").default_value(text));
        // 编辑后标记为未保存
        cx.subscribe(&editor, |this, _, ev: &InputEvent, cx| {
            if matches!(ev, InputEvent::Change) {
                if let Some(d) = this.settings_forms.as_mut().and_then(|f| f.doc.as_mut()) {
                    d.saved = false;
                    cx.notify();
                }
            }
        })
        .detach();
        self.forms_mut().doc = Some(DocEdit { path, editor, saved: true, tab });
        cx.notify();
    }

    /// 关闭编辑器。
    pub fn close_doc(&mut self, cx: &mut Context<Self>) {
        self.forms_mut().doc = None;
        cx.notify();
    }

    /// 保存编辑器里的文件(必要时创建目录),然后刷新扫描结果与补全。
    pub fn save_doc(&mut self, cx: &mut Context<Self>) {
        let Some(doc) = self.forms().doc.as_ref() else { return };
        let (path, text) = (doc.path.clone(), doc.editor.read(cx).value().to_string());
        let result = path.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|_| std::fs::write(&path, text));
        let ok = result.is_ok();
        self.forms_mut().error = result.err().map(|e| format!("保存失败:{e}"));
        if ok {
            if let Some(d) = self.forms_mut().doc.as_mut() {
                d.saved = true;
            }
            self.refresh_items();
            self.push_extensions_all(cx);
        }
        cx.notify();
    }

    /// 展开 / 收起新建表单。
    pub fn toggle_item_form(&mut self, cx: &mut Context<Self>) {
        let f = &mut self.forms_mut().item;
        f.open = !f.open;
        self.forms_mut().error = None;
        cx.notify();
    }

    /// 按表单新建一条扩展,成功后在编辑器里打开它。
    ///
    /// 参数:`kind` 种类;`tab` 当前自定义标签。
    pub fn create_item_from_form(&mut self, kind: ItemKind, tab: CustomizeTab, window: &mut Window, cx: &mut Context<Self>) {
        let f = &self.forms().item;
        let (name, desc, scope) = (f.name.read(cx).value().trim().to_string(), f.description.read(cx).value().trim().to_string(), f.scope);
        let scope = if scope == Scope::Project && self.claude_cfg_root.is_none() { Scope::User } else { scope };
        match items::create_item(&self.claude_home, self.cfg_project(), scope, kind, &name, &desc) {
            Ok(path) => {
                let f = &mut self.forms_mut().item;
                f.open = false;
                let inputs = [f.name.clone(), f.description.clone()];
                for i in inputs {
                    i.update(cx, |i, cx| i.set_value(String::new(), window, cx));
                }
                self.forms_mut().error = None;
                self.refresh_items();
                self.push_extensions_all(cx);
                self.open_doc(path, tab, window, cx);
            }
            Err(e) => self.forms_mut().error = Some(format!("新建失败:{e}")),
        }
        cx.notify();
    }

    /// 把条目移到废纸篓。
    pub fn trash_item(&mut self, item: &Item, cx: &mut Context<Self>) {
        let trash = self.claude_home.join(".Trash");
        let result = items::trash_item(item, &trash);
        self.forms_mut().error = result.err().map(|e| format!("删除失败:{e}"));
        if self.forms().doc.as_ref().is_some_and(|d| d.path.starts_with(item.path.parent().unwrap_or(&item.path)) && (item.kind == ItemKind::Skill || d.path == item.path)) {
            self.forms_mut().doc = None;
        }
        self.refresh_items();
        self.push_extensions_all(cx);
        cx.notify();
    }
}
