//! 权限模式、思考强度与模型描述。

use serde::{Deserialize, Serialize};

/// Claude Code 的六种权限模式,对应 `--permission-mode` 的取值。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum PermissionMode {
    /// 默认:每次危险操作都询问
    #[default]
    Default,
    /// 自动接受文件编辑
    AcceptEdits,
    /// 计划模式:只读,先出计划
    Plan,
    /// 自动模式:由分类器决定
    Auto,
    /// 不再询问(未预先允许的直接拒绝)
    DontAsk,
    /// 绕过全部权限检查
    BypassPermissions,
}

impl PermissionMode {
    /// 全部模式,按界面循环切换的顺序排列。
    pub const ALL: [PermissionMode; 6] = [
        Self::Default,
        Self::AcceptEdits,
        Self::Plan,
        Self::Auto,
        Self::DontAsk,
        Self::BypassPermissions,
    ];

    /// 传给 `claude --permission-mode` 的参数值。
    pub fn cli_value(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::AcceptEdits => "acceptEdits",
            Self::Plan => "plan",
            Self::Auto => "auto",
            Self::DontAsk => "dontAsk",
            Self::BypassPermissions => "bypassPermissions",
        }
    }

    /// 从 CLI 取值解析,未知值返回 `None`。
    pub fn from_cli(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.cli_value() == value)
    }

    /// 界面显示名。
    pub fn label(self) -> &'static str {
        match self {
            Self::Default => "默认",
            Self::AcceptEdits => "接受编辑",
            Self::Plan => "计划",
            Self::Auto => "自动",
            Self::DontAsk => "不再询问",
            Self::BypassPermissions => "绕过权限",
        }
    }

    /// 循环到下一个模式(Shift+Tab)。
    pub fn next(self) -> Self {
        let idx = Self::ALL.iter().position(|m| *m == self).unwrap_or(0);
        Self::ALL[(idx + 1) % Self::ALL.len()]
    }
}

/// 思考强度,对应 `--effort`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum Effort {
    Low,
    #[default]
    Medium,
    High,
    XHigh,
    Max,
}

impl Effort {
    pub const ALL: [Effort; 5] = [Self::Low, Self::Medium, Self::High, Self::XHigh, Self::Max];

    /// 传给 `claude --effort` 的参数值。
    pub fn cli_value(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::XHigh => "xhigh",
            Self::Max => "max",
        }
    }

    /// 界面显示名。
    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "低",
            Self::Medium => "中",
            Self::High => "高",
            Self::XHigh => "超高",
            Self::Max => "最大",
        }
    }
}

/// 可选模型。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelInfo {
    /// 传给 `--model` 的 id 或别名
    pub id: String,
    /// 界面显示名
    pub name: String,
    /// 一句话说明
    pub description: String,
}

impl ModelInfo {
    pub fn new(id: &str, name: &str, description: &str) -> Self {
        Self { id: id.into(), name: name.into(), description: description.into() }
    }
}

/// 内置的模型别名列表(真实模式下由 `claude` 初始化响应覆盖)。
pub fn default_models() -> Vec<ModelInfo> {
    vec![
        ModelInfo::new("default", "默认", "由 Claude Code 选择"),
        ModelInfo::new("opus", "Opus", "最强推理,适合复杂任务"),
        ModelInfo::new("sonnet", "Sonnet", "速度与能力均衡"),
        ModelInfo::new("haiku", "Haiku", "最快,适合简单任务"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 模式循环回到起点() {
        let mut m = PermissionMode::Default;
        for _ in 0..PermissionMode::ALL.len() {
            m = m.next();
        }
        assert_eq!(m, PermissionMode::Default);
    }

    #[test]
    fn 模式与_cli_值互转() {
        for m in PermissionMode::ALL {
            assert_eq!(PermissionMode::from_cli(m.cli_value()), Some(m));
        }
        assert_eq!(PermissionMode::from_cli("nope"), None);
    }
}
