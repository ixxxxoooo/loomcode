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
use std::time::Duration;

/// 「任务」标签里日志最多读多少字节(取文件末尾;比内存里的 `OUTPUT_CAP` 大得多,长日志才看得到近况)。
pub const LOG_CAP: usize = 96 * 1024;
/// 日志文件的轮询间隔:运行中的任务文件变长就刷新输出。
const POLL: Duration = Duration::from_millis(800);

/// 读到的日志尾部。
pub struct LogTail {
    /// 整理后的文本(已去掉回车覆盖的进度行,截断时从整行开始)
    pub text: String,
    /// 文件比读取上限大,前面的部分没显示
    pub truncated: bool,
    /// 文件总字节数
    pub total: u64,
}

/// 读取输出文件的尾部(最多 `cap` 字节);文件不存在或读失败返回 `None`。
///
/// 截断时丢掉不完整的第一行;每行只保留最后一个 `\r` 之后的内容(进度条反复覆盖同一行),`\r\n` 当普通换行。
pub fn read_log(path: &str, cap: usize) -> Option<LogTail> {
    let mut f = std::fs::File::open(path).ok()?;
    let total = f.metadata().ok()?.len();
    let truncated = total > cap as u64;
    f.seek(SeekFrom::Start(total.saturating_sub(cap as u64))).ok()?;
    let mut buf = Vec::new();
    f.take(cap as u64).read_to_end(&mut buf).ok()?;
    let mut text = String::from_utf8_lossy(&buf).into_owned();
    if truncated {
        if let Some(i) = text.find('\n') {
            text.drain(..=i);
        }
    }
    Some(LogTail { text: tidy_log(&text), truncated, total })
}

/// 整理终端输出:`\r\n` 归一,行内的回车覆盖只留最后一段。
fn tidy_log(text: &str) -> String {
    let text = text.replace("\r\n", "\n");
    if !text.contains('\r') {
        return text;
    }
    text.split('\n').map(|l| l.rsplit('\r').find(|x| !x.is_empty()).unwrap_or("")).collect::<Vec<_>>().join("\n")
}

/// 文件大小的简短写法。
fn human_bytes(n: u64) -> String {
    match n {
        0..=1023 => format!("{n} B"),
        1024..=1_048_575 => format!("{:.0} KB", n as f64 / 1024.),
        _ => format!("{:.1} MB", n as f64 / 1_048_576.),
    }
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
        self.start_log_poll(cx);
    }

    /// 启动日志轮询(只启动一次):停在「任务」标签时,运行中任务的输出文件变长就重绘,实现实时查看。
    /// 后台命令的输出只写进文件、不会有事件通知界面,所以必须自己盯文件。
    fn start_log_poll(&mut self, cx: &mut Context<Self>) {
        if self.log_poll.is_some() {
            return;
        }
        self.log_poll = Some(cx.spawn(async move |this, cx| {
            let mut last: u64 = 0;
            loop {
                cx.background_executor().timer(POLL).await;
                let Ok(files) = this.update(cx, |this, cx| {
                    if this.tab != crate::workbench::RightTab::Tasks {
                        return Vec::new();
                    }
                    let Some(m) = &this.model else { return Vec::new() };
                    m.read(cx).state.tasks.iter().filter(|x| x.counts_as_running()).filter_map(|x| x.output_file.clone()).collect::<Vec<_>>()
                }) else {
                    break;
                };
                // 文件长度的总和当签名:有任何一个变长(或任务集合变了)就重绘
                let sig = files.iter().map(|f| std::fs::metadata(f).map_or(0, |m| m.len()) + 1).sum::<u64>();
                if sig != last {
                    last = sig;
                    if this.update(cx, |_, cx| cx.notify()).is_err() {
                        break;
                    }
                }
            }
        }));
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
        // 输出:演示后端把输出放在内存里;真实后端只给输出文件,读它的末尾
        let log = if !sel.output.is_empty() { None } else { sel.output_file.as_deref().and_then(|f| read_log(f, LOG_CAP)) };
        let (output, truncated, total) = match log {
            Some(l) => (l.text, l.truncated, l.total),
            None => (sel.output.clone(), false, sel.output.len() as u64),
        };
        // 贴底跟随:切换任务时、或上一帧已经停在底部,就跟着新输出往下走;用户往上翻看时不打扰
        let switched = self.task_scroll_for.borrow().as_deref() != Some(sel_id.as_str());
        if switched {
            *self.task_scroll_for.borrow_mut() = Some(sel_id.clone());
        }
        let (off, max) = (f32::from(self.task_scroll.offset().y), f32::from(self.task_scroll.max_offset().y));
        if switched || max <= 0. || off <= -max + 24. {
            self.task_scroll.scroll_to_bottom();
        }
        let parsed = crate::ansi::parse(&output);
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
                    .when_some(sel.output_file.clone(), |d, f| {
                        let copy = f.clone();
                        d.child(
                            div().id("task-log-path").text_size(px(11.)).font_family(MONO).text_color(t.muted_foreground).truncate().cursor_pointer().hover(|d| d.text_color(t.foreground))
                                .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new("点击复制日志文件路径").build(window, cx))
                                .on_click(move |_, _, cx| { cx.write_to_clipboard(ClipboardItem::new_string(copy.clone())); })
                                .child(f),
                        )
                    }),
            )
            .child(
                v_flex().id("task-output").flex_1().min_h_0().overflow_y_scroll().track_scroll(&self.task_scroll).p_3().gap_1()
                    .when(truncated, |d| d.child(
                        div().text_size(px(11.)).text_color(t.muted_foreground).child(format!("日志共 {},只显示最后 {};完整内容在上方文件里", human_bytes(total), human_bytes(LOG_CAP as u64))),
                    ))
                    .child(if output.is_empty() {
                        div().text_size(px(12.)).text_color(t.muted_foreground).child(if run { "暂无输出" } else { "没有输出记录" }).into_any_element()
                    } else {
                        // 可拖选 / ⌘C 的等宽文本,带 ANSI 颜色;元素 id 按任务固定,日志变长时选区不丢
                        div().w_full().text_size(px(12.)).font_family(MONO).text_color(t.foreground)
                            .child(gpui_base::SelectableText::new(SharedString::from(format!("task-log-{}", sel.task_id)), parsed.text).highlights(parsed.runs))
                            .into_any_element()
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
        let path = p.to_str().unwrap();
        // 超限:只留末尾,且丢掉不完整的第一行
        std::fs::write(&p, format!("{}\n第二行\n第三行\n", "x".repeat(200))).unwrap();
        let l = read_log(path, 30).unwrap();
        assert!(l.truncated);
        assert_eq!(l.text, "第二行\n第三行\n");
        assert!(l.total > 200);
        // 未超限:原样
        std::fs::write(&p, "短文本").unwrap();
        let l = read_log(path, LOG_CAP).unwrap();
        assert!(!l.truncated);
        assert_eq!(l.text, "短文本");
        assert!(read_log("/不存在/x", LOG_CAP).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[::core::prelude::v1::test]
    fn 整理回车覆盖的进度行() {
        assert_eq!(tidy_log("a\r\nb\n"), "a\nb\n");
        assert_eq!(tidy_log("10%\r50%\r100%\n完成"), "100%\n完成");
        assert_eq!(tidy_log("普通\n文本"), "普通\n文本");
    }
}
