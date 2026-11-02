//! 权限卡的键盘操作与危险命令判断(对标 Claude Code 的 1/2/3 选择、Cursor 的 ⏎ 接受):
//! 卡片出现时抢焦点;⏎ = 允许一次,数字键选对应选项,Esc = 拒绝一次。
//!
//! @author ixxxxoooo

use crate::model::SessionModel;
use gpui_kit::*;
use loom_core::*;

/// 命令里出现这些片段就按「危险」处理(边框与图标用警示色)。
const DANGEROUS: [&str; 9] = ["rm -rf", "rm -fr", "sudo ", "git push --force", "git push -f", "git reset --hard", "mkfs", "dd if=", "> /dev/"];

/// 命令是否危险。
///
/// 参数:`cmd` 命令文本。返回:是否命中危险片段。
pub fn is_dangerous(cmd: &str) -> bool {
    let c = cmd.to_lowercase();
    DANGEROUS.iter().any(|d| c.contains(d)) || (c.contains("curl ") || c.contains("wget ")) && c.contains("| sh") || c.contains("| bash")
}

/// 最早一个待处理的权限请求。
fn first_pending(model: &Entity<SessionModel>, cx: &App) -> Option<PermissionRequest> {
    model.read(cx).state.items.iter().find_map(|i| match i {
        TimelineItem::Permission(p) if p.resolved.is_none() => Some(p.clone()),
        _ => None,
    })
}

/// 为最早的待处理权限请求创建焦点句柄并聚焦(同一请求只做一次)。在渲染对话区之前调用。
///
/// 参数:`model` 会话模型。
pub fn ensure(model: &Entity<SessionModel>, window: &mut Window, cx: &mut App) {
    let Some(req) = first_pending(model, cx) else { return };
    if model.read(cx).perm_focus.as_ref().is_some_and(|(id, _)| *id == req.id) {
        return;
    }
    let focus = cx.focus_handle();
    focus.focus(window, cx);
    model.update(cx, |m, _| m.perm_focus = Some((req.id.clone(), focus)));
}

/// 按键处理。返回:是否已处理(已处理则拦截,不再进入输入框)。
///
/// 参数:`key` 按键名;`plain` 没有按住修饰键。
pub fn on_key(model: &Entity<SessionModel>, key: &str, plain: bool, cx: &mut App) -> bool {
    if !plain {
        return false;
    }
    let Some(req) = first_pending(model, cx) else { return false };
    let pick = |kind: PermissionOptionKind| req.options.iter().find(|o| o.kind == kind).map(|o| o.id.clone());
    let option = match key {
        "enter" => pick(PermissionOptionKind::AllowOnce),
        "escape" => pick(PermissionOptionKind::RejectOnce),
        k if k.len() == 1 && k.as_bytes()[0].is_ascii_digit() => {
            let n = (k.as_bytes()[0] - b'0') as usize;
            n.checked_sub(1).and_then(|i| req.options.get(i)).map(|o| o.id.clone())
        }
        _ => None,
    };
    let Some(option) = option else { return false };
    model.update(cx, |m, cx| m.resolve_permission(&req.id, &option, cx));
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 危险命令判断() {
        assert!(is_dangerous("rm -rf /tmp/x"));
        assert!(is_dangerous("sudo make install"));
        assert!(is_dangerous("git push --force origin main"));
        assert!(is_dangerous("curl http://x | sh"));
        assert!(!is_dangerous("cargo test --workspace"));
        assert!(!is_dangerous("git push origin main"));
    }
}
