//! 命令面板 / 中央搜索(⌘K):命令、会话、项目文件、演示场景统一模糊搜索。

use crate::ui::{MONO, icon};
use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputEvent, InputState, MoveDown, MoveUp};
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::path::Path;
use ucode_config::ThemeChoice;

/// 选中某项后要执行的动作。
#[derive(Debug, Clone, PartialEq)]
pub enum PaletteAction {
    NewSession,
    ToggleLeft,
    ToggleRight,
    CycleMode,
    ToggleIde,
    OpenSettings(Option<&'static str>),
    OpenPage(&'static str),
    SetTheme(ThemeChoice),
    /// (项目下标, 会话 id)
    OpenSession(usize, String),
    OpenFile(String),
    /// 演示场景 id
    RunScenario(String),
}

/// 条目分组。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Command,
    Session,
    File,
    Scenario,
}

impl Group {
    fn label(self) -> &'static str {
        match self {
            Self::Command => "命令",
            Self::Session => "会话",
            Self::File => "文件",
            Self::Scenario => "演示场景",
        }
    }
}

/// 列表里的一项。
#[derive(Debug, Clone)]
pub struct PaletteItem {
    pub title: String,
    pub subtitle: String,
    pub group: Group,
    pub icon: IconName,
    pub action: PaletteAction,
}

/// 面板向工作台发出的事件。
pub enum PaletteEvent {
    Run(PaletteAction),
    Close,
}

impl EventEmitter<PaletteEvent> for Palette {}

/// 子序列模糊匹配:返回得分(越高越好),不匹配返回 `None`。
/// 用动态规划在所有对齐方式里取最优:连续命中、词首命中与更靠前的命中得分更高。
pub fn fuzzy_score(query: &str, text: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    let q: Vec<char> = query.to_lowercase().chars().filter(|c| !c.is_whitespace()).collect();
    let t: Vec<char> = text.to_lowercase().chars().collect();
    if q.is_empty() {
        return Some(0);
    }
    if q.len() > t.len() {
        return None;
    }
    const NONE: i32 = i32::MIN / 2;
    // 单个字符命中的基础分
    let base = |j: usize| -> i32 {
        let mut s = 10;
        if j == 0 || matches!(t[j - 1], '/' | '-' | '_' | ' ' | '.') {
            s += 6;
        }
        s - (j as i32).min(20) / 4
    };
    // prev[j]:前 i 个查询字符匹配完、且第 i 个落在 t[j] 的最佳得分
    let mut prev: Vec<i32> = (0..t.len()).map(|j| if t[j] == q[0] { base(j) } else { NONE }).collect();
    for &qc in &q[1..] {
        let mut cur = vec![NONE; t.len()];
        // best_before[j] = max(prev[k]) for k < j
        let mut best_before = NONE;
        for j in 0..t.len() {
            if j > 0 {
                best_before = best_before.max(prev[j - 1]);
            }
            if t[j] == qc {
                let mut cand = best_before;
                if j > 0 && prev[j - 1] > NONE {
                    cand = cand.max(prev[j - 1] + 8); // 紧挨上一个命中
                }
                if cand > NONE {
                    cur[j] = cand + base(j);
                }
            }
        }
        prev = cur;
    }
    let best = prev.into_iter().max().filter(|&s| s > NONE)?;
    Some(best - (t.len() as i32 / 8))
}

/// 在条目中按查询筛选并排序,最多返回 `limit` 项。
pub fn filter_items(items: &[PaletteItem], query: &str, limit: usize) -> Vec<PaletteItem> {
    if query.trim().is_empty() {
        // 无查询时只列命令与会话,避免淹没在文件里
        return items.iter().filter(|i| matches!(i.group, Group::Command | Group::Session)).take(limit).cloned().collect();
    }
    let mut scored: Vec<(i32, usize, &PaletteItem)> = items
        .iter()
        .enumerate()
        .filter_map(|(ix, i)| {
            let a = fuzzy_score(query, &i.title);
            let b = fuzzy_score(query, &i.subtitle).map(|s| s - 12);
            a.into_iter().chain(b).max().map(|s| (s, ix, i))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored.into_iter().take(limit).map(|(_, _, i)| i.clone()).collect()
}

/// 递归收集项目文件(相对路径),跳过隐藏目录与构建产物。
pub fn collect_files(root: &Path, max: usize) -> Vec<String> {
    fn go(root: &Path, dir: &Path, depth: usize, out: &mut Vec<String>, max: usize) {
        if depth > 5 || out.len() >= max {
            return;
        }
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        let mut entries: Vec<_> = rd.filter_map(|e| e.ok()).collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') && name != ".gitignore" || matches!(name.as_str(), "target" | "node_modules" | "DerivedData" | ".build") {
                continue;
            }
            let path = e.path();
            if path.is_dir() {
                go(root, &path, depth + 1, out, max);
            } else if let Ok(rel) = path.strip_prefix(root) {
                out.push(rel.to_string_lossy().to_string());
                if out.len() >= max {
                    return;
                }
            }
        }
    }
    let mut out = Vec::new();
    go(root, root, 0, &mut out, max);
    out
}

pub struct Palette {
    input: Entity<InputState>,
    items: Vec<PaletteItem>,
    shown: Vec<PaletteItem>,
    sel: usize,
    _sub: Subscription,
}

impl Palette {
    pub fn new(items: Vec<PaletteItem>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("搜索命令、会话、文件…"));
        let sub = cx.subscribe_in(&input, window, |this: &mut Self, _, ev: &InputEvent, _, cx| match ev {
            InputEvent::Change => this.refilter(cx),
            InputEvent::PressEnter { .. } => this.run_selected(cx),
            _ => {}
        });
        input.update(cx, |i, cx| i.focus(window, cx));
        let shown = filter_items(&items, "", 50);
        Self { input, items, shown, sel: 0, _sub: sub }
    }

    /// 预填查询(调试与外部调用)。
    pub fn set_query(&mut self, q: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |i, cx| i.set_value(q.to_string(), window, cx));
        self.refilter(cx);
    }

    fn refilter(&mut self, cx: &mut Context<Self>) {
        let q = self.input.read(cx).value().to_string();
        self.shown = filter_items(&self.items, &q, 50);
        self.sel = 0;
        cx.notify();
    }

    fn run_selected(&mut self, cx: &mut Context<Self>) {
        if let Some(item) = self.shown.get(self.sel) {
            cx.emit(PaletteEvent::Run(item.action.clone()));
        }
    }

    fn move_sel(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.shown.is_empty() {
            return;
        }
        let n = self.shown.len() as isize;
        self.sel = ((self.sel as isize + delta).rem_euclid(n)) as usize;
        cx.notify();
    }
}

impl Render for Palette {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let this = cx.entity();
        // 遮罩 + 居中偏上的面板
        div().absolute().inset_0().flex().justify_center().items_start().pt(px(88.))
            .bg(t.background.opacity(0.35))
            .child(div().absolute().inset_0().id("palette-backdrop").on_click(cx.listener(|_, _, _, cx| cx.emit(PaletteEvent::Close))))
            .child(
                v_flex().relative().w(px(620.)).max_h(px(480.)).rounded(px(14.)).border_1().border_color(t.border).bg(t.popover).shadow_lg().overflow_hidden()
                    .capture_action(cx.listener(|this, _: &MoveUp, _, cx| this.move_sel(-1, cx)))
                    .capture_action(cx.listener(|this, _: &MoveDown, _, cx| this.move_sel(1, cx)))
                    .capture_action(cx.listener(|_, _: &crate::workbench::Escape, _, cx| cx.emit(PaletteEvent::Close)))
                    .child(
                        h_flex().h(px(48.)).px_3().gap_2().items_center().border_b_1().border_color(t.border)
                            .child(icon(IconName::Search, 16., t.muted_foreground))
                            .child(div().flex_1().child(Input::new(&self.input).appearance(false).bordered(false))),
                    )
                    .child(
                        v_flex().id("palette-list").flex_1().min_h_0().overflow_y_scroll().p_1()
                            .when(self.shown.is_empty(), |d| d.child(div().p_4().text_size(px(13.)).text_color(t.muted_foreground).child("没有匹配的结果")))
                            .children(self.shown.iter().enumerate().map(|(i, item)| {
                                let this = this.clone();
                                let action = item.action.clone();
                                h_flex().id(SharedString::from(format!("pal-{i}"))).h(px(34.)).px_2().gap_2p5().items_center().rounded(px(8.)).cursor_pointer()
                                    .when(i == self.sel, |d| d.bg(t.list_active))
                                    .hover(|d| d.bg(t.list_hover))
                                    .on_click(move |_, _, cx| { let a = action.clone(); this.update(cx, |_, cx| cx.emit(PaletteEvent::Run(a))); })
                                    .child(icon(item.icon, 15., t.muted_foreground))
                                    .child(div().text_size(px(13.5)).flex_none().max_w(px(300.)).truncate().child(item.title.clone()))
                                    .child(div().flex_1().min_w_0().truncate().text_size(px(12.)).font_family(if item.group == Group::File { MONO } else { ".SystemUIFont" }).text_color(t.muted_foreground).child(item.subtitle.clone()))
                                    .child(div().text_size(px(11.)).text_color(t.muted_foreground).child(item.group.label()))
                            })),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(title: &str, group: Group) -> PaletteItem {
        PaletteItem { title: title.into(), subtitle: String::new(), group, icon: IconName::Search, action: PaletteAction::NewSession }
    }

    #[::core::prelude::v1::test]
    fn 子序列匹配() {
        assert!(fuzzy_score("rtr", "src/router.rs").is_some());
        assert!(fuzzy_score("xyz", "src/router.rs").is_none());
        assert_eq!(fuzzy_score("", "任何"), Some(0));
    }

    #[::core::prelude::v1::test]
    fn 连续与词首得分更高() {
        let a = fuzzy_score("router", "src/router.rs").unwrap();
        let b = fuzzy_score("router", "r_o_u_t_e_r_extra").unwrap();
        assert!(a > b);
    }

    #[::core::prelude::v1::test]
    fn 无查询只列命令与会话() {
        let items = vec![item("新会话", Group::Command), item("src/a.rs", Group::File), item("某会话", Group::Session)];
        let shown = filter_items(&items, "", 10);
        assert_eq!(shown.len(), 2);
    }

    #[::core::prelude::v1::test]
    fn 筛选并按得分排序() {
        let items = vec![item("设置", Group::Command), item("新会话", Group::Command), item("会话列表", Group::Command)];
        let shown = filter_items(&items, "会话", 10);
        assert_eq!(shown.len(), 2);
        // 前缀命中(词首)优先
        assert_eq!(shown[0].title, "会话列表");
    }
}
