//! 提问向导的「其他」自由输入与键盘操作:输入框与焦点句柄按提问 id 惰性创建(需要 window),
//! 答案 = 已选项 + 「其他」里的文字;键盘:A–Z 选择、⏎ 下一题 / 提交、Esc 跳过、← → 翻页。
//!
//! @author ixxxxoooo

use crate::model::SessionModel;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;
use loom_core::*;

/// 找出这个会话里最早一个还没回答的提问。
fn pending_question(model: &Entity<SessionModel>, cx: &App) -> Option<QuestionRequest> {
    model.read(cx).state.items.iter().find_map(|i| match i {
        TimelineItem::Question(q) if q.answers.is_none() => Some(q.clone()),
        _ => None,
    })
}

/// 为待回答的提问创建「其他」输入框(每题一个)与焦点句柄;已创建过就什么也不做。
/// 在渲染对话区之前调用(创建输入框需要 window)。
///
/// 参数:`model` 会话模型。
pub fn ensure(model: &Entity<SessionModel>, window: &mut Window, cx: &mut App) {
    let Some(q) = pending_question(model, cx) else { return };
    if model.read(cx).wizard_inputs.contains_key(&q.id) {
        return;
    }
    let mut inputs = Vec::new();
    for step in 0..q.questions.len() {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("其他…"));
        let (weak, id) = (model.downgrade(), q.id.clone());
        cx.subscribe(&input, move |_, ev: &InputEvent, cx| {
            let Some(model) = weak.upgrade() else { return };
            match ev {
                InputEvent::Change => on_change(&model, &id, step, cx),
                InputEvent::PressEnter { .. } => advance(&model, &id, cx),
                _ => {}
            }
        })
        .detach();
        inputs.push(input);
    }
    let focus = cx.focus_handle();
    focus.focus(window, cx);
    model.update(cx, |m, _| {
        m.wizard_inputs.insert(q.id.clone(), inputs);
        m.wizard_focus = Some(focus);
    });
}

/// 「其他」里的文字(去掉首尾空白);没有输入框时为空。
pub fn custom(model: &Entity<SessionModel>, id: &str, step: usize, cx: &App) -> String {
    model.read(cx).wizard_inputs.get(id).and_then(|v| v.get(step)).map(|i| i.read(cx).value().trim().to_string()).unwrap_or_default()
}

/// 单选题里输入了「其他」就取消已选项(两者互斥);多选题两者并存。
fn on_change(model: &Entity<SessionModel>, id: &str, step: usize, cx: &mut App) {
    let multi = pending_question(model, cx).and_then(|q| q.questions.get(step).map(|x| x.multi_select)).unwrap_or(false);
    let typed = !custom(model, id, step, cx).is_empty();
    model.update(cx, |m, cx| {
        if typed && !multi && let Some(slot) = m.wizard_draft.get_mut(id).and_then(|d| d.get_mut(step)) {
            slot.clear();
        }
        cx.notify();
    });
}

/// 某题是否已作答:选了项,或「其他」里有字。
pub fn answered(model: &Entity<SessionModel>, id: &str, step: usize, cx: &App) -> bool {
    model.read(cx).wizard_draft.get(id).and_then(|d| d.get(step)).is_some_and(|s| !s.is_empty()) || !custom(model, id, step, cx).is_empty()
}

/// 汇总全部答案:每题 = 已选项 + 「其他」文字。
fn collect(model: &Entity<SessionModel>, id: &str, n: usize, cx: &App) -> Vec<Vec<String>> {
    (0..n)
        .map(|i| {
            let mut a = model.read(cx).wizard_draft.get(id).and_then(|d| d.get(i)).cloned().unwrap_or_default();
            let c = custom(model, id, i, cx);
            if !c.is_empty() {
                a.push(c);
            }
            a
        })
        .collect()
}

/// 选一个选项:单选替换并清空「其他」,选完自动进入下一题(最后一题不自动提交);多选切换。
pub fn pick(model: &Entity<SessionModel>, id: &str, step: usize, label: &str, window: &mut Window, cx: &mut App) {
    let Some(q) = pending_question(model, cx) else { return };
    let (n, multi) = (q.questions.len(), q.questions.get(step).is_some_and(|x| x.multi_select));
    if !multi && let Some(input) = model.read(cx).wizard_inputs.get(id).and_then(|v| v.get(step)).cloned() {
        input.update(cx, |i, cx| i.set_value("", window, cx));
    }
    model.update(cx, |m, cx| {
        m.wizard_pick(id, step, n, label, multi, cx);
        if !multi && step + 1 < n {
            m.wizard_go(id, step + 1, cx);
        }
    });
}

/// 前进:非最后一题进入下一题(要求本题已作答),最后一题提交(要求全部已作答)。
pub fn advance(model: &Entity<SessionModel>, id: &str, cx: &mut App) {
    let Some(q) = pending_question(model, cx) else { return };
    let n = q.questions.len();
    let step = model.read(cx).wizard_step.get(id).copied().unwrap_or(0).min(n.saturating_sub(1));
    if step + 1 >= n {
        if (0..n).all(|i| answered(model, id, i, cx)) {
            let answers = collect(model, id, n, cx);
            model.update(cx, |m, cx| m.answer_question(id, answers, cx));
        }
    } else if answered(model, id, step, cx) {
        model.update(cx, |m, cx| m.wizard_go(id, step + 1, cx));
    }
}

/// 跳过:以空答案结束这次提问。
pub fn skip(model: &Entity<SessionModel>, id: &str, cx: &mut App) {
    let Some(q) = pending_question(model, cx) else { return };
    let n = q.questions.len();
    model.update(cx, |m, cx| m.answer_question(id, vec![Vec::new(); n], cx));
}

/// 翻到指定题(越界时夹在范围内)。
pub fn go(model: &Entity<SessionModel>, id: &str, target: isize, cx: &mut App) {
    let Some(q) = pending_question(model, cx) else { return };
    let step = target.clamp(0, q.questions.len() as isize - 1) as usize;
    model.update(cx, |m, cx| m.wizard_go(id, step, cx));
}

/// 键盘:字母选择(超出选项数的下一个字母聚焦「其他」)、⏎、Esc、← →。焦点在输入框里时只处理 Esc(回到卡片)。
///
/// 参数:`key` 按键名;`shift_only_or_none` 没有命令 / 控制类修饰键。返回:是否已处理。
pub fn on_key(model: &Entity<SessionModel>, key: &str, plain: bool, window: &mut Window, cx: &mut App) -> bool {
    let Some(q) = pending_question(model, cx) else { return false };
    let id = q.id.clone();
    let n = q.questions.len();
    let step = model.read(cx).wizard_step.get(&id).copied().unwrap_or(0).min(n.saturating_sub(1));
    let input = model.read(cx).wizard_inputs.get(&id).and_then(|v| v.get(step)).cloned();
    let typing = input.as_ref().is_some_and(|i| i.focus_handle(cx).is_focused(window));
    if typing {
        if key == "escape" && let Some(f) = model.read(cx).wizard_focus.clone() {
            f.focus(window, cx);
            return true;
        }
        return false;
    }
    if !plain {
        return false;
    }
    match key {
        "enter" => advance(model, &id, cx),
        "escape" => skip(model, &id, cx),
        "left" => go(model, &id, step as isize - 1, cx),
        "right" => go(model, &id, step as isize + 1, cx),
        k if k.len() == 1 && k.as_bytes()[0].is_ascii_lowercase() => {
            let index = (k.as_bytes()[0] - b'a') as usize;
            let options = &q.questions[step].options;
            if let Some(opt) = options.get(index) {
                pick(model, &id, step, &opt.label.clone(), window, cx);
            } else if index == options.len() && let Some(i) = input {
                i.focus_handle(cx).focus(window, cx);
            } else {
                return false;
            }
        }
        _ => return false,
    }
    true
}
