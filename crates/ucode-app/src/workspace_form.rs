//! 工作区创建 / 编辑表单(放在对话框里):名称 + 文件夹列表。
//!
//! @author ixxxxoooo

use crate::ui::icon;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use ucode_core::WorkspaceDef;

/// 表单状态:正在编辑的工作区、名称输入框、校验错误。
pub struct WorkspaceForm {
    def: WorkspaceDef,
    name: Entity<InputState>,
    error: Option<String>,
}

impl WorkspaceForm {
    /// 以一个工作区定义(新建时是空的草稿)创建表单。
    pub fn new(def: WorkspaceDef, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let name = cx.new(|cx| InputState::new(window, cx).placeholder("工作区名称").default_value(def.name.clone()));
        Self { def, name, error: None }
    }

    /// 名称输入框(用于对话框打开时聚焦)。
    pub fn name_input(&self) -> &Entity<InputState> {
        &self.name
    }

    /// 读出当前表单内容。
    pub fn to_def(&self, cx: &App) -> WorkspaceDef {
        WorkspaceDef { name: self.name.read(cx).value().trim().to_string(), ..self.def.clone() }
    }

    /// 校验:名称非空、至少一个且不重复的文件夹、文件夹都存在。出错时把原因显示在表单里。
    ///
    /// 返回:合法时返回工作区定义。
    pub fn validate(&mut self, cx: &mut Context<Self>) -> Option<WorkspaceDef> {
        let def = self.to_def(cx);
        let result = def.validate().and_then(|_| match def.folders.iter().find(|f| !std::path::Path::new(f).is_dir()) {
            Some(missing) => Err(format!("文件夹不存在:{missing}")),
            None => Ok(()),
        });
        self.error = result.clone().err();
        cx.notify();
        result.ok().map(|_| def)
    }

    /// 弹出目录选择(可多选),把选中的文件夹加进列表。
    fn pick_folders(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: true, prompt: Some("添加文件夹".into()) });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else { return };
            let _ = this.update(cx, |this, cx| {
                for p in paths {
                    this.def.add_folder(&p.to_string_lossy());
                }
                this.error = None;
                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for WorkspaceForm {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let rows = self.def.folders.iter().enumerate().map(|(ix, path)| {
            h_flex().h(px(32.)).px_2().gap_2().items_center().rounded(px(6.)).bg(t.muted)
                .child(icon(IconName::Folder, 14., t.muted_foreground))
                .child(div().flex_1().min_w_0().truncate().text_size(px(12.5)).child(path.clone()))
                .when(ix == 0, |d| d.child(div().text_size(px(11.)).text_color(t.primary).child("主目录")))
                .when(ix > 0, |d| d.child(Button::new(SharedString::from(format!("ws-primary-{ix}"))).xsmall().ghost().icon(IconName::ArrowUp).tooltip("设为主目录")
                    .on_click(cx.listener(move |this, _, _, cx| { this.def.make_primary(ix); cx.notify(); }))))
                .child(Button::new(SharedString::from(format!("ws-remove-{ix}"))).xsmall().ghost().icon(IconName::Trash).tooltip("移除")
                    .on_click(cx.listener(move |this, _, _, cx| { this.def.remove_folder(ix); cx.notify(); })))
        });
        v_flex().gap_3().py_2()
            .child(Input::new(&self.name))
            .child(div().text_size(px(12.)).text_color(t.muted_foreground).child("文件夹(第一个是主目录,会话工作目录以它为准)"))
            .child(v_flex().gap_1().children(rows))
            .child(Button::new("ws-add-folder").small().icon(IconName::FolderPlus).label("添加文件夹…")
                .on_click(cx.listener(|this, _, window, cx| this.pick_folders(window, cx))))
            .when_some(self.error.clone(), |d, e| d.child(div().text_size(px(12.)).text_color(t.danger).child(e)))
    }
}
