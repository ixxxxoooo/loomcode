//! 右侧「任务」标签:本会话的全部后台任务(命令、监听、子 Agent、工作流),
//! 上面是列表(运行中的在前),下面是选中任务的详情与输出(可停止、复制、打开子 Agent 对话)。
//!
//! @author ixxxxoooo

use crate::right_panel::RightPanel;
use crate::task_ui::{kind_icon, meta_text, status_icon, task_color};
use crate::ui::{MONO, empty_state, icon, pill};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_core::*;
use std::io::{Read, Seek, SeekFrom};

/// 读取输出文件的尾部(最多 `OUTPUT_CAP` 字节);文件不存在或读失败返回 `None`。
pub fn read_tail(path: &str) -> Option<String> {
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let cap = OUTPUT_CAP as u64;
    f.seek(SeekFrom::Start(len.saturating_sub(cap))).ok()?;
    let mut buf = Vec::new();
    f.take(cap).read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

impl RightPanel {
    /// 打开「任务」标签,并选中某个任务。
    ///
    /// 参数:`id` 要查看的任务;`None` 只打开标签,保留原选中项。
    pub fn open_tasks(&mut self, id: Option<String>, cx: &mut Context<Self>) {
        if id.is_some() {
            self.task_sel = id;
        }
        self.open_tab(crate::workbench::RightTab::Tasks, cx);
    }

    /// 「任务」标签内容。
    pub(crate) fn tasks_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let Some(model) = self.model.clone() else {
            return empty_state(IconName::Activity, "没有会话", "打开一个会话后,这里显示它的后台任务", cx).into_any_element();
        };
        let mut tasks: Vec<BackgroundTask> = model.read(cx).state.tasks.iter().filter(|x| x.visible()).cloned().collect();
        if tasks.is_empty() {
            return empty_state(IconName::Activity, "还没有后台任务", "后台命令、监听和后台子 Agent 会列在这里,可以查看输出与停止", cx).into_any_element();
        }
        tasks.sort_by_key(|x| (!x.counts_as_running(), std::cmp::Reverse(x.started_ms)));
        let now = now_ms();
        let running = tasks.iter().filter(|x| x.counts_as_running()).count();
        let first_id = group_tasks(&tasks).first().and_then(|s| s.tasks.first()).map_or_else(|| tasks[0].task_id.clone(), |x| x.task_id.clone());
        let sel_id = self.task_sel.clone().filter(|id| tasks.iter().any(|x| &x.task_id == id)).unwrap_or(first_id);
        let sel = tasks.iter().find(|x| x.task_id == sel_id).cloned().unwrap_or_else(|| tasks[0].clone());

        let sections = group_tasks(&tasks);
        let mut list: Vec<AnyElement> = Vec::new();
        for sec in &sections {
            let run_n = sec.tasks.iter().filter(|x| x.counts_as_running()).count();
            list.push(
                h_flex().w_full().h(px(24.)).px_3().gap_1p5().items_center().text_size(px(11.)).text_color(t.muted_foreground)
                    .when_some(sec.group.kind(), |d, k| d.child(icon(kind_icon(k), 12., t.muted_foreground)))
                    .child(format!("{} · {}{}", sec.group.label(), sec.tasks.len(), if run_n > 0 && run_n < sec.tasks.len() { format!("({run_n} 运行中)") } else { String::new() }))
                    .into_any_element(),
            );
            for x in &sec.tasks {
                let on = x.task_id == sel_id;
                let run = x.counts_as_running();
                let id = x.task_id.clone();
                list.push(
                    h_flex().id(SharedString::from(format!("tk-{}", x.task_id))).w_full().h(px(32.)).px_3().gap_2().items_center().cursor_pointer()
                        .when(on, |d| d.bg(t.muted))
                        .hover(|d| d.bg(t.list_hover))
                        .on_click(cx.listener(move |this, _, _, cx| { this.task_sel = Some(id.clone()); cx.notify(); }))
                        .child(if run { Spinner::new().xsmall().into_any_element() } else { icon(status_icon(x.status), 13., task_color(x.status, cx)).into_any_element() })
                        .child(div().flex_1().min_w_0().truncate().text_size(px(13.)).text_color(if run { t.foreground } else { t.muted_foreground }).child(x.description.clone()))
                        .child(div().flex_none().text_size(px(11.)).text_color(t.muted_foreground.opacity(0.8)).child(format_elapsed(x.elapsed_ms(now))))
                        .into_any_element(),
                );
            }
        }

        // 详情
        let run = sel.counts_as_running();
        let stalled = run && sel.idle_ms(now) >= STALL_MS;
        let output = if !sel.output.is_empty() { sel.output.clone() } else { sel.output_file.as_deref().and_then(read_tail).unwrap_or_default() };
        let card_id = sel.tool_use_id.clone().filter(|_| sel.kind == TaskKind::Agent);
        let (m_stop, id_stop) = (model.clone(), sel.task_id.clone());
        let copy_text = output.clone();
        let detail = v_flex().flex_1().min_h_0().w_full().border_t_1().border_color(t.border)
            .child(
                v_flex().w_full().px_3().py_2().gap_1().border_b_1().border_color(t.border)
                    .child(
                        h_flex().w_full().gap_2().items_center()
                            .child(icon(kind_icon(sel.kind), 14., t.primary))
                            .child(div().flex_1().min_w_0().truncate().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(sel.description.clone()))
                            .child(pill(sel.status.label(), task_color(sel.status, cx), t.muted))
                            .when(run, |d| d.child(
                                Button::new("tk-stop").xsmall().ghost().icon(IconName::Square).label("停止")
                                    .on_click(move |_, _, cx| { m_stop.update(cx, |m, cx| m.stop_task(&id_stop, cx)); }),
                            ))
                            .when_some(card_id, |d, cid| d.child(
                                Button::new("tk-open-agent").xsmall().ghost().icon(IconName::Bot).label("查看对话")
                                    .on_click(cx.listener(move |this, _, _, cx| this.open_subagent(cid.clone(), cx))),
                            ))
                            .when(!copy_text.is_empty(), |d| d.child(
                                Button::new("tk-copy").xsmall().ghost().icon(IconName::Copy).tooltip("复制输出")
                                    .on_click(move |_, _, cx| { cx.write_to_clipboard(ClipboardItem::new_string(copy_text.clone())); }),
                            )),
                    )
                    .child(div().text_size(px(11.)).text_color(t.muted_foreground).child(format!("{} · {}{}", sel.kind.label(), meta_text(Some(sel.elapsed_ms(now)), sel.tool_uses, sel.tokens), if stalled { format!(" · 已 {} 无进展", format_elapsed(sel.idle_ms(now))) } else { String::new() })))
                    .when(!sel.summary.is_empty(), |d| d.child(div().text_size(px(12.)).text_color(t.foreground).child(sel.summary.clone())))
                    .when_some(sel.output_file.clone(), |d, f| d.child(div().text_size(px(11.)).font_family(MONO).text_color(t.muted_foreground).truncate().child(f))),
            )
            .child(
                v_flex().id("task-output").flex_1().min_h_0().overflow_y_scroll().p_3()
                    .child(if output.is_empty() {
                        div().text_size(px(12.)).text_color(t.muted_foreground).child(if run { "暂无输出" } else { "没有输出记录" }).into_any_element()
                    } else {
                        div().w_full().text_size(px(12.)).font_family(MONO).text_color(t.foreground).child(output).into_any_element()
                    }),
            );

        v_flex().size_full()
            .child(
                h_flex().w_full().h(px(28.)).px_3().gap_2().items_center().flex_none().text_size(px(11.)).text_color(t.muted_foreground)
                    .child(format!("{} 个任务", tasks.len()))
                    .child(div().text_color(if running > 0 { t.primary } else { t.muted_foreground }).child(format!("{running} 个运行中"))),
            )
            .child(v_flex().id("task-list").flex_none().max_h(px(200.)).overflow_y_scroll().children(list))
            .child(detail)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 读取文件尾部不超过上限() {
        let dir = std::env::temp_dir().join(format!("loom-tail-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("a.output");
        std::fs::write(&p, "x".repeat(OUTPUT_CAP + 100)).unwrap();
        assert_eq!(read_tail(p.to_str().unwrap()).unwrap().len(), OUTPUT_CAP);
        std::fs::write(&p, "短文本").unwrap();
        assert_eq!(read_tail(p.to_str().unwrap()).unwrap(), "短文本");
        assert!(read_tail("/不存在/x").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
