//! Agent 窗口三栏(侧栏 | 对话 | 右面板)的拖动调宽、宽度保存和窄窗口自动收起。
//!
//! @author ixxxxoooo

use crate::layout_rules::{clamp_left, clamp_right, collapsed, is_narrow, RIGHT_HIDE_BELOW, RIGHT_SHOW_ABOVE};
use crate::workbench::{Page, Workbench};
use gpui_kit::*;
use std::time::Duration;

/// 拖动停下多久后把宽度写进设置(拖动过程中每一帧都会回调,不能每次都落盘)。
const SAVE_DELAY: Duration = Duration::from_millis(400);

impl Workbench {
    /// 拖动回调:记录新的栏宽并安排保存。
    ///
    /// 参数:`sizes` 三个面板的当前宽度(左、中、右)。不可见的面板宽度不可信,会被忽略。
    pub fn on_split_resized(&mut self, sizes: &[Pixels], cx: &mut Context<Self>) {
        let [left, _, right] = sizes else { return };
        let (left, right) = (f32::from(*left), f32::from(*right));
        if self.left_visible() && left > 1. {
            self.left_w = clamp_left(left);
        }
        if self.right_visible() && right > 1. {
            self.right_w = clamp_right(right);
        }
        self.schedule_width_save(cx);
    }

    /// 左侧栏当前是否显示。
    pub fn left_visible(&self) -> bool {
        self.left_open && self.page != Page::Settings
    }

    /// 右侧面板当前是否显示。
    pub fn right_visible(&self) -> bool {
        self.right_open && self.page == Page::Chat
    }

    /// 防抖保存:一串拖动只在停下 [`SAVE_DELAY`] 后写一次文件。
    fn schedule_width_save(&mut self, cx: &mut Context<Self>) {
        if self.width_save_pending {
            return;
        }
        self.width_save_pending = true;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SAVE_DELAY).await;
            let _ = this.update(cx, |this, _| {
                this.width_save_pending = false;
                this.settings.left_width = this.left_w;
                this.settings.right_width = this.right_w;
                if let Err(e) = this.settings.save(&this.settings_path) {
                    eprintln!("保存栏宽失败:{e}");
                }
            });
        })
        .detach();
    }

    /// 窗口变窄时自动收起侧栏,变宽后恢复到收起前的状态。在渲染前调用。
    ///
    /// 参数:`width` 窗口宽度(像素)。
    pub fn apply_responsive(&mut self, width: Pixels) {
        let narrow = is_narrow(self.layout_narrow, f32::from(width));
        if narrow == self.layout_narrow {
            return;
        }
        self.layout_narrow = narrow;
        if narrow {
            // 记下收起前的开关状态,然后真正收起;之后用户在窄窗口里手动点开侧栏也有效
            self.left_before_narrow = self.left_open;
            self.left_open = false;
        } else {
            self.left_open = self.left_before_narrow;
        }
    }

    /// 窗口变窄时自动收起右侧面板(比侧栏先收),变宽后恢复。
    ///
    /// 参数:`width` 窗口宽度(像素)。
    pub fn apply_responsive_right(&mut self, width: Pixels) {
        let narrow = collapsed(self.right_narrow, f32::from(width), RIGHT_HIDE_BELOW, RIGHT_SHOW_ABOVE);
        if narrow == self.right_narrow {
            return;
        }
        self.right_narrow = narrow;
        if narrow {
            self.right_before_narrow = self.right_open;
            self.right_open = false;
        } else {
            self.right_open = self.right_before_narrow;
        }
    }
}
