//! 项目信任页:管理每个项目是否已在 Claude Code 里「信任」(`~/.claude.json` 的 `hasTrustDialogAccepted`)。
//! 未信任的项目,Claude Code 首次打开时会先询问是否允许项目里的 Hooks、MCP 与环境变量。
//!
//! @author ixxxxoooo

use super::widgets::{banner, error_banner, group, item, note, page_header, Tone};
use crate::ui::{MONO, caption};
use crate::workbench::Workbench;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme, v_flex};
use gpui_kit::*;

/// 渲染项目信任页。
pub fn render(wb: &Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    let t = cx.theme().clone();
    let list = wb.forms().ops.trust.clone();
    let untrusted = list.iter().filter(|(_, _, ok)| !ok).count();
    let rows: Vec<AnyElement> = if list.is_empty() {
        vec![item(cx).h(px(44.)).child(caption("还没有项目;在侧边栏添加项目后,这里会列出它们", cx)).into_any_element()]
    } else {
        list.into_iter().enumerate().map(|(i, (name, path, ok))| {
            let p = path.clone();
            item(cx).min_h(px(48.)).gap_3().py_2()
                .child(v_flex().flex_1().min_w_0().gap_0p5()
                    .child(div().text_size(px(13.)).text_color(t.foreground).child(name))
                    .child(div().truncate().font_family(MONO).text_size(px(11.)).text_color(t.muted_foreground).child(path)))
                .child(div().text_size(px(11.5)).text_color(if ok { t.success } else { t.muted_foreground }).child(if ok { "已信任" } else { "未信任" }))
                .child(Switch::new(("trust", i)).checked(ok).on_click(cx.listener(move |this, c: &bool, _, cx| this.set_project_trust(&p, *c, cx))))
                .into_any_element()
        }).collect()
    };
    v_flex().w_full()
        .child(page_header("项目信任", "决定 Claude Code 是否直接加载项目里的 Hooks、MCP 服务器与环境变量", div(), cx))
        .children(error_banner(wb, cx))
        .children((untrusted > 0).then(|| banner(Tone::Info, &format!("有 {untrusted} 个项目未信任:首次在其中运行 claude 时会先弹出信任确认"), cx)))
        .child(group("项目", rows, cx))
        .child(note("只改 ~/.claude.json 里对应项目的 hasTrustDialogAccepted,其它字段原样保留;写入前会备份", cx))
        .into_any_element()
}
