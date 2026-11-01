//! 危险操作的确认横幅:删除 MCP / 钩子、条目移到废纸篓、卸载与安装插件。
//! 点操作先记下待确认项,页面顶部出现横幅,确认后才真正执行。
//!
//! @author ixxxxoooo

use crate::workbench::Workbench;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_config::items::Item;
use loom_config::plugins::PluginEntry;
use loom_config::{HookEntry, McpServer, Scope};

/// 待确认的操作。
#[derive(Clone)]
pub enum Confirm {
    DeleteMcp(McpServer),
    DeleteHook(HookEntry),
    TrashItem(Item),
    Uninstall(PluginEntry),
    /// 删除模型服务商(显示名)
    DeleteProvider(String),
    /// 安装插件(`名称@市场`)到某一层
    Install(String, Scope),
}

impl Confirm {
    /// 横幅文字。
    pub fn message(&self) -> String {
        match self {
            Self::DeleteMcp(s) => format!("删除 MCP 服务器「{}」?会从{}配置中移除,原文件先备份为 .loom-backup。", s.name, s.scope.label()),
            Self::DeleteHook(h) => format!("删除 {} 钩子?会从{}配置中移除。", h.event, h.scope.label()),
            Self::TrashItem(i) => format!("「{}」会被移到废纸篓,确定吗?", i.name),
            Self::DeleteProvider(n) => format!("删除服务商「{n}」?服务商配置与已保存的密钥都会被删除。"),
            Self::Uninstall(p) => format!("卸载插件 {}?(范围:{})", p.id, p.scope.label()),
            Self::Install(id, scope) => format!("安装 {id}({}范围)?插件可以在你的电脑上运行代码(命令、MCP 服务器、Hooks),请只安装信任来源的插件。", scope.label()),
        }
    }

    /// 确认按钮文字。
    fn label(&self) -> &'static str {
        match self {
            Self::Install(..) => "安装",
            Self::Uninstall(_) => "卸载",
            Self::TrashItem(_) => "移到废纸篓",
            _ => "删除",
        }
    }

    /// 是否破坏性操作(确认按钮用红色)。
    fn destructive(&self) -> bool {
        !matches!(self, Self::Install(..))
    }
}

impl Workbench {
    /// 记下待确认的操作。
    pub fn ask(&mut self, c: Confirm, cx: &mut Context<Self>) {
        self.forms_mut().confirm = Some(c);
        cx.notify();
    }

    /// 取消待确认的操作。
    pub fn cancel_confirm(&mut self, cx: &mut Context<Self>) {
        self.forms_mut().confirm = None;
        cx.notify();
    }

    /// 执行待确认的操作。
    pub fn accept_confirm(&mut self, cx: &mut Context<Self>) {
        let Some(c) = self.forms_mut().confirm.take() else { return };
        match c {
            Confirm::DeleteMcp(s) => self.delete_mcp(&s, cx),
            Confirm::DeleteHook(h) => self.delete_hook(&h, cx),
            Confirm::TrashItem(i) => self.trash_item(&i, cx),
            Confirm::DeleteProvider(_) => self.delete_provider(cx),
            Confirm::Uninstall(p) => self.plugin_action("uninstall", &p, cx),
            Confirm::Install(id, scope) => self.do_install(&id, scope, cx),
        }
        cx.notify();
    }
}

/// 确认横幅;没有待确认项时返回 `None`。
pub fn confirm_banner(wb: &Workbench, cx: &mut Context<Workbench>) -> Option<Div> {
    let c = wb.forms().confirm.clone()?;
    let t = cx.theme().clone();
    let tint = if c.destructive() { t.danger } else { t.warning };
    Some(v_flex().w_full().p_3().gap_2().rounded(px(8.)).bg(tint.opacity(0.1)).border_1().border_color(tint.opacity(0.4))
        .child(div().text_size(px(13.)).child(c.message()))
        .child(h_flex().gap_2().justify_end()
            .child(Button::new("confirm-cancel").small().ghost().label("取消").on_click(cx.listener(|this, _, _, cx| this.cancel_confirm(cx))))
            .child(Button::new("confirm-ok").small().when(c.destructive(), |b| b.danger()).when(!c.destructive(), |b| b.primary()).label(c.label()).on_click(cx.listener(|this, _, _, cx| this.accept_confirm(cx))))))
}
