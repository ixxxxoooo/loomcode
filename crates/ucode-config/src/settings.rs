//! UCode 自身偏好。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use ucode_core::{Effort, PermissionMode};

/// 主题选择。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ThemeChoice {
    #[default]
    PaperLight,
    PaperDark,
    /// 跟随系统外观
    System,
}

/// 偏好设置。所有字段都有默认值,旧文件缺字段时不会报错。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub theme: ThemeChoice,
    pub default_model: String,
    pub default_mode: PermissionMode,
    pub default_effort: Effort,
    /// 思考内容默认展开
    pub expand_thinking: bool,
    /// 右侧面板默认打开
    pub right_open: bool,
    /// 融合模式:开启后 Agent 与 IDE 在同一窗口原地切换;关闭时 IDE 是独立窗口
    pub fused_ide: bool,
    /// 手动指定 `claude` 可执行文件路径;空则自动查找
    pub claude_path: Option<String>,
    /// 最近使用的项目目录
    pub recent_projects: Vec<String>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: ThemeChoice::default(),
            default_model: "default".into(),
            default_mode: PermissionMode::Default,
            default_effort: Effort::Medium,
            expand_thinking: false,
            right_open: true,
            fused_ide: false,
            claude_path: None,
            recent_projects: Vec::new(),
        }
    }
}

impl AppSettings {
    /// 默认存放位置 `~/.ucode/settings.json`。
    pub fn default_path() -> Option<PathBuf> {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".ucode").join("settings.json"))
    }

    /// 读取;文件不存在或损坏时返回默认值。
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    /// 写入(自动创建目录);先写临时文件再改名,避免写一半崩溃。
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self).map_err(std::io::Error::other)?)?;
        std::fs::rename(tmp, path)
    }

    /// 记录最近项目(去重,最多 12 个,最新在前)。
    pub fn touch_project(&mut self, path: &str) {
        self.recent_projects.retain(|p| p != path);
        self.recent_projects.insert(0, path.to_string());
        self.recent_projects.truncate(12);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 缺字段使用默认值() {
        let s: AppSettings = serde_json::from_str(r#"{"expand_thinking":true}"#).unwrap();
        assert!(s.expand_thinking);
        assert_eq!(s.default_model, "default");
        assert!(s.right_open);
    }

    #[test]
    fn 保存后可读回() {
        let dir = std::env::temp_dir().join(format!("ucode-cfg-{}", std::process::id()));
        let path = dir.join("settings.json");
        let mut s = AppSettings::default();
        s.theme = ThemeChoice::PaperDark;
        s.touch_project("/a");
        s.touch_project("/b");
        s.touch_project("/a");
        s.save(&path).unwrap();
        let back = AppSettings::load(&path);
        assert_eq!(back, s);
        assert_eq!(back.recent_projects, vec!["/a", "/b"]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn 损坏文件回退默认() {
        let dir = std::env::temp_dir().join(format!("ucode-cfg-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        std::fs::write(&path, "{bad").unwrap();
        assert_eq!(AppSettings::load(&path), AppSettings::default());
        let _ = std::fs::remove_dir_all(dir);
    }
}
