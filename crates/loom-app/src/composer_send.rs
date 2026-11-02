//! 输入框右下角的两个按钮:圆形发送 / 停止按钮(等待时带旋转弧线动画),以及「优化提示词」按钮与它的逻辑。
//!
//! @author ixxxxoooo

use crate::composer::Composer;
use crate::ui::icon;
use gpui_kit::assets::IconName;
use gpui_kit::component::ActiveTheme;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::path::PathBuf;
use std::time::Duration;

/// 优化提示词需要的运行环境(真实模式由工作台注入)。
#[derive(Clone)]
pub struct RewriteCtx {
    /// 用户在设置里指定的 claude 路径
    pub claude_path: Option<String>,
    /// 当前项目目录
    pub cwd: PathBuf,
    /// 模型服务商等额外环境变量
    pub env: Vec<(String, String)>,
}

/// 按钮直径(像素)。
const BTN: f32 = 28.;

/// 圆形发送 / 停止按钮。
///
/// 状态:没内容 → 灰底灰箭头(不可点);有内容 → 主题色底白箭头;回合运行中 → 浅主题色底 + 中间方块 + 一圈旋转弧线,点击中断。
///
/// 参数:`running` 回合是否在运行;`can_send` 是否有可发送内容;`on_click` 点击回调。
pub fn send_button(running: bool, can_send: bool, on_click: impl Fn(&mut Window, &mut App) + 'static, cx: &App) -> AnyElement {
    let t = cx.theme();
    let base = div().id(if running { "stop" } else { "send" }).relative().size(px(BTN)).flex_none().flex().items_center().justify_center().rounded_full();
    if running {
        let c = t.primary;
        return base.bg(c.opacity(0.14)).cursor_pointer().hover(|d| d.bg(c.opacity(0.22)))
            .on_click(move |_, window, cx| on_click(window, cx))
            // 旋转的弧线:一圈 1.1 秒,匀速
            .child(
                div().absolute().size_full().flex().items_center().justify_center().child(
                    icon(IconName::LoaderCircle, BTN - 2., c).with_animation(
                        "send-ring",
                        Animation::new(Duration::from_millis(1100)).repeat_synced().with_max_fps(crate::ui::fps::SPIN),
                        |ic, delta| ic.transform(Transformation::rotate(percentage(delta))),
                    ),
                ),
            )
            // 中间的停止方块随呼吸轻微缩放
            .child(div().size(px(9.)).rounded(px(2.5)).bg(c).with_animation(
                "send-stop-pulse",
                Animation::new(Duration::from_millis(1600)).repeat_synced().with_max_fps(crate::ui::fps::BREATHE),
                |d, delta| d.opacity(0.65 + 0.35 * (delta * std::f32::consts::TAU).cos().abs()),
            ))
            .into_any_element();
    }
    if can_send {
        let (bg, fg) = (t.primary, t.primary_foreground);
        base.bg(bg).cursor_pointer().hover(|d| d.bg(bg.opacity(0.85)))
            .on_click(move |_, window, cx| on_click(window, cx))
            .child(icon(IconName::ArrowUp, 15., fg))
            .into_any_element()
    } else {
        base.bg(t.muted).child(icon(IconName::ArrowUp, 15., t.muted_foreground.opacity(0.6))).into_any_element()
    }
}

/// 「优化提示词」按钮:有文本时亮,没有则置灰;优化进行中图标闪烁。
///
/// 参数:`enabled` 输入框有文本;`busy` 正在优化;`on_click` 点击回调。
pub fn optimize_button(enabled: bool, busy: bool, on_click: impl Fn(&mut Window, &mut App) + 'static, cx: &App) -> AnyElement {
    let t = cx.theme();
    let fg = if enabled || busy { t.foreground } else { t.muted_foreground.opacity(0.45) };
    let ic = icon(IconName::WandSparkles, 16., fg);
    let base = div().id("optimize").size(px(BTN)).flex_none().flex().items_center().justify_center().rounded_full();
    if busy {
        return base.child(ic.with_animation("optimize-busy", Animation::new(Duration::from_millis(900)).repeat_synced().with_max_fps(crate::ui::fps::BREATHE), |i, delta| {
            i.opacity(0.35 + 0.65 * (0.5 - 0.5 * (delta * std::f32::consts::TAU).cos()))
        })).into_any_element();
    }
    let hover = t.list_hover;
    base.when(enabled, |d| d.cursor_pointer().hover(move |d| d.bg(hover)).on_click(move |_, window, cx| on_click(window, cx)))
        .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(if enabled { "优化提示词" } else { "输入内容后可优化提示词" }).build(window, cx))
        .child(ic)
        .into_any_element()
}

/// 演示模式的本地改写:不调用模型,只在草稿后补一段固定的工作约定,方便验证界面。
fn demo_rewrite(text: &str) -> String {
    format!("{}\n\n要求:\n1. 先说明你的理解与改动计划,再动手;\n2. 完成后列出修改的文件与验证方式。", text.trim())
}

impl Composer {
    /// 注入优化提示词所需的运行环境(真实模式)。
    pub fn set_rewrite_ctx(&mut self, ctx: RewriteCtx) {
        self.rewrite = Some(ctx);
    }

    /// 优化输入框里的提示词:后台改写,完成后替换输入框内容(期间用户改过输入则丢弃结果)。
    pub(crate) fn optimize(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.input.read(cx).value().trim().to_string();
        if text.is_empty() || self.optimizing {
            return;
        }
        self.optimizing = true;
        self.optimize_error = None;
        cx.notify();
        let (demo, ctx) = (self.demo, self.rewrite.clone());
        let original = text.clone();
        cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    if demo {
                        std::thread::sleep(Duration::from_millis(1400));
                        return Ok(demo_rewrite(&text));
                    }
                    let ctx = ctx.ok_or_else(|| "当前会话不支持优化提示词".to_string())?;
                    let claude = loom_claude::find_claude(ctx.claude_path.as_deref()).ok_or_else(|| "找不到 claude 可执行文件".to_string())?;
                    loom_claude::rewrite::optimize_prompt(&claude, &ctx.cwd, &ctx.env, &text)
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.optimizing = false;
                match result {
                    Ok(better) => {
                        // 优化期间用户又改了输入:不覆盖
                        if this.input.read(cx).value().trim() == original {
                            this.input.update(cx, |i, cx| i.set_value(better, window, cx));
                        }
                    }
                    Err(e) => this.optimize_error = Some(e),
                }
                cx.notify();
            });
        })
        .detach();
    }
}
