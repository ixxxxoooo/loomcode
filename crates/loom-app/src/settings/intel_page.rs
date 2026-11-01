//! 代码智能页:目前只有「忽略规则」(文件搜索、快速打开、IDE 全局搜索共用)。
//! 诊断、跳转定义、补全依赖语言服务器,暂未支持。
//!
//! @author ixxxxoooo

use super::widgets::{banner, block_row, group, page_header, rows, Tone};
use crate::workbench::Workbench;
use gpui_kit::component::input::Input;
use gpui_kit::component::{Sizable, v_flex};
use gpui_kit::*;

/// 渲染代码智能页。
pub fn render(wb: &Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    v_flex().w_full()
        .child(page_header("代码智能", "文件检索的范围", div(), cx))
        .child(banner(Tone::Info, "诊断、跳转定义与补全需要语言服务器(LSP),暂未支持", cx))
        .child(group("忽略规则", rows![
            block_row("忽略的文件与目录", "逗号分隔的名称;支持开头或结尾的 * 通配,例如 *.log、build*。命中的条目不出现在快速打开与全局搜索里(以点开头的文件始终隐藏)",
                Input::new(wb.field("ignore")).small(), cx),
        ], cx))
        .into_any_element()
}
