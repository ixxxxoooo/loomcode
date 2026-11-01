//! 隐私与遥测页:Claude Code 的数据上报开关(当前配置层 env)与本地诊断脱敏说明。
//!
//! @author ixxxxoooo

use super::widgets::{error_banner, group, note, page_header, row, rows, scope_header};
use crate::ui::pill;
use crate::workbench::Workbench;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme, v_flex};
use gpui_kit::*;

/// 隐私开关:(env 变量, 标题, 说明)。
const PRIVACY: &[(&str, &str, &str)] = &[
    ("DISABLE_TELEMETRY", "关闭使用统计", "DISABLE_TELEMETRY:不上报功能使用数据"),
    ("DISABLE_ERROR_REPORTING", "关闭错误报告", "DISABLE_ERROR_REPORTING:不上报崩溃与错误"),
    ("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "关闭非必要网络请求", "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC:同时关闭统计、错误报告、自动更新检查等"),
    ("DISABLE_AUTOUPDATER", "关闭 claude 自动更新", "DISABLE_AUTOUPDATER"),
];

/// 渲染隐私与遥测页。
pub fn render(wb: &Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    let t = cx.theme().clone();
    let scope = wb.effective_scope();
    let layer = wb.claude_cfg.layer(scope);
    let target = match wb.scope_settings_file(scope) {
        Some(p) => format!("写入 {} 的 env", p.display()),
        None => "先打开一个项目会话".into(),
    };
    v_flex().w_full()
        .child(page_header("隐私与遥测", "控制 Claude Code 向外发送哪些数据;LoomCode 自己不收集任何数据", div(), cx))
        .child(scope_header(wb, target, cx))
        .children(error_banner(wb, cx))
        .child(group("数据上报", PRIVACY.iter().map(|(key, title, desc)| {
            let key = *key;
            row(title, desc, Switch::new(SharedString::from(format!("sw-{key}"))).checked(layer.env(key).is_some_and(|v| v != "0" && v != "false"))
                .on_click(cx.listener(move |this, c: &bool, _, cx| this.set_env_flag(key, *c, cx))), cx).into_any_element()
        }).collect(), cx))
        .child(note("开关写入所选配置层的 env,对新会话生效", cx))
        .child(group("本地", rows![
            row("日志与诊断包", "家目录显示为 ~,只含配置概况与变量名,不含密钥和环境变量值", pill("始终脱敏", t.success, t.muted), cx),
            row("凭据", "LoomCode 不读取钥匙串;服务商 API Key 单独存放在 0600 权限的文件里", pill("本地保存", t.foreground, t.muted), cx),
        ], cx))
        .into_any_element()
}
