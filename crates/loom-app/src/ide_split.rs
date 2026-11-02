//! 编辑器拆分(对标 VS Code「向右拆分 / 向下拆分」):主编辑器之外可再挂一个次编辑器组,
//! 两组各有自己的标签栏与当前文件,可调整分隔条。
//!
//! 设计:每个组就是一个 `IdeView`;主组(宿主嵌入的那个)持有次组 `Split`,渲染时左右 / 上下并排。
//! 同一文件在两组里各有一份编辑器状态,改动经 `sync_sibling` 同步文本、`sync_saved` 同步已保存状态。
//! 外部打开文件(文件树、搜索、快捷键)落在「最近点击的组」。次组的标签全部关闭后自动合并。
//!
//! @author ixxxxoooo

use crate::ide::{IdeEvent, IdeView};
use gpui_kit::component::{ActiveTheme, h_resizable, resizable_panel, v_resizable};
use gpui_kit::*;
use std::path::{Path, PathBuf};

/// 拆分状态:次编辑器组与方向。
pub(crate) struct Split {
    pub child: Entity<IdeView>,
    /// 为真则上下排列(向下拆分),否则左右排列
    pub down: bool,
    _sub: Subscription,
}

impl IdeView {
    /// 本组当前可拆分的文件:真实文件(临时记事本与差异标签没有可在另一组重新打开的路径)。
    fn splittable(&self) -> Option<PathBuf> {
        self.tabs.active().filter(|p| !crate::ide_untitled::is_untitled(p) && !crate::ide_diff::is_diff(p)).map(Path::to_path_buf)
    }

    /// 把当前标签拆分到另一组(没有次组就新建)。
    ///
    /// 参数:`down` 为真向下拆分,否则向右。在次组里调用时,文件会送到主组。
    pub fn split_active(&mut self, down: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.splittable() else { return };
        let from_second = self.parent.is_some();
        let target = self.parent.clone().unwrap_or_else(|| cx.weak_entity());
        // 推迟到本次事件之后:拆分要更新两个组,而当前组正在被更新
        window.defer(cx, move |window, cx| {
            let _ = target.update(cx, |p, cx| p.split_open(path, down, from_second, window, cx));
        });
    }

    /// 主组上执行:确保次组存在、方向正确,并把 `path` 在「另一组」打开。
    fn split_open(&mut self, path: PathBuf, down: bool, from_second: bool, window: &mut Window, cx: &mut Context<Self>) {
        let child = match &mut self.split {
            Some(sp) => {
                sp.down = down;
                sp.child.clone()
            }
            None => {
                let child = cx.new(|_| IdeView::new());
                let (me, root) = (cx.weak_entity(), self.root.clone());
                child.update(cx, |c, cx| {
                    c.root = root;
                    c.parent = Some(me);
                    c.start_watch(window, cx);
                });
                let sub = cx.subscribe(&child, |this, _, ev: &IdeEvent, cx| match ev {
                    IdeEvent::RevealInExplorer(p) => cx.emit(IdeEvent::RevealInExplorer(p.clone())),
                    IdeEvent::GroupEmpty => {
                        this.split = None;
                        this.active_second = false;
                        cx.notify();
                    }
                });
                self.split = Some(Split { child: child.clone(), down, _sub: sub });
                child
            }
        };
        if from_second {
            self.active_second = false;
            self.open_inner(path, false, true, window, cx);
        } else {
            self.active_second = true;
            child.update(cx, |c, cx| c.open_inner(path, false, true, window, cx));
        }
        cx.notify();
    }

    /// 合并拆分:关闭次组(有未保存修改会先确认,全部关完后自动合并)。
    pub fn close_split(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let target = self.parent.clone().unwrap_or_else(|| cx.weak_entity());
        window.defer(cx, move |window, cx| {
            let _ = target.update(cx, |p, cx| {
                if let Some(child) = p.split.as_ref().map(|s| s.child.clone()) {
                    child.update(cx, |c, cx| c.close_all(window, cx));
                }
            });
        });
    }

    /// 是否处于拆分状态(任一组调用均可)。
    pub fn is_split(&self) -> bool {
        self.split.is_some() || self.parent.is_some()
    }

    /// 外部操作(打开文件、保存、关闭…)应落在次组时返回它。
    pub(crate) fn focused_child(&self) -> Option<Entity<IdeView>> {
        self.split.as_ref().filter(|_| self.active_second).map(|s| s.child.clone())
    }

    /// 另一个组(主组看次组,次组看主组);没有拆分时为 `None`。
    fn sibling(&self, cx: &App) -> Option<Entity<IdeView>> {
        match (&self.split, &self.parent) {
            (Some(sp), _) => Some(sp.child.clone()),
            (None, Some(p)) => p.upgrade(),
            _ => {
                let _ = cx;
                None
            }
        }
    }

    /// 本组某文件的内容变了:把文本同步给另一组里同一文件的编辑器(文本相同则不动,避免来回触发)。
    pub(crate) fn sync_sibling(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let Some(sib) = self.sibling(cx) else { return };
        let Some(doc) = self.docs.get(path) else { return };
        let text = doc.editor.read(cx).value().to_string();
        let path = path.to_path_buf();
        window.defer(cx, move |window, cx| {
            sib.update(cx, |s, cx| {
                let Some(d) = s.docs.get(&path) else { return };
                if d.editor.read(cx).value().as_ref() == text.as_str() {
                    return;
                }
                d.editor.update(cx, |e, cx| e.set_value(text, window, cx));
            });
        });
    }

    /// 本组保存了某文件:另一组里文本一致的同一文件也标记为已保存。
    pub(crate) fn sync_saved(&mut self, path: &Path, cx: &mut Context<Self>) {
        let Some(sib) = self.sibling(cx) else { return };
        let Some(doc) = self.docs.get(path) else { return };
        let (text, mtime) = (doc.editor.read(cx).value().to_string(), doc.mtime);
        let path = path.to_path_buf();
        cx.spawn(async move |_, cx| {
            let _ = sib.update(cx, |s, cx| {
                if let Some(d) = s.docs.get_mut(&path).filter(|d| d.editor.read(cx).value().as_ref() == text.as_str()) {
                    d.saved = true;
                    d.stale = false;
                    d.mtime = mtime;
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 拆分状态下的整体渲染:两个组并排,中间是可拖动的分隔条;点哪个组,外部操作就落在哪个组。
    pub(crate) fn render_with_split(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some((child, down)) = self.split.as_ref().map(|s| (s.child.clone(), s.down)) else { return self.render_group(window, cx) };
        let first = self.render_group(window, cx);
        let a = div().size_full().capture_any_mouse_down(cx.listener(|this, _: &MouseDownEvent, _, _| this.active_second = false)).child(first);
        let me = cx.weak_entity();
        let b = div().size_full().capture_any_mouse_down(move |_, _, cx| {
            let _ = me.update(cx, |this, _| this.active_second = true);
        }).child(child);
        let _ = cx.theme();
        let group = if down { v_resizable("ide-split-v") } else { h_resizable("ide-split-h") };
        div().size_full().child(group.child(resizable_panel().child(a)).child(resizable_panel().child(b))).into_any_element()
    }
}
