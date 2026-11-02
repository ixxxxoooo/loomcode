//! 从磁盘读出「启动一次 claude」所需的上下文(claude 路径 + 额外环境变量),
//! 给拿不到工作台内存状态的后台运行(自动化)用,保证与交互会话注入的一致。
//!
//! @author ixxxxoooo

use crate::AppSettings;
use crate::providers::{ProviderStore, load_keys};
use std::path::Path;

/// 一次 claude 运行的上下文。
#[derive(Debug, Clone, Default)]
pub struct RunContext {
    /// 设置里自定义的 claude 路径(未设置为 `None`,按 PATH 查找)。
    pub claude_path: Option<String>,
    /// 额外环境变量:网络代理 / 超时等客户端偏好 + 当前服务商的模型映射。
    pub env: Vec<(String, String)>,
}

/// 读取运行上下文。服务商配置与密钥和 `settings.json` 放在同一目录。
/// 参数:`settings_path` `settings.json` 的路径。返回:运行上下文(文件缺失时取默认值)。
pub fn load_run_context(settings_path: &Path) -> RunContext {
    let settings = AppSettings::load(settings_path);
    let mut env = settings.session_env();
    let store = ProviderStore::load(&settings_path.with_file_name("providers.json"));
    env.extend(store.mapping_env(&load_keys(&settings_path.with_file_name("provider-keys.json"))));
    RunContext { claude_path: settings.claude_path, env }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 缺文件时取默认且与会话环境一致() {
        let dir = std::env::temp_dir().join(format!("loom-runctx-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        let ctx = load_run_context(&path);
        assert!(ctx.claude_path.is_none());
        assert_eq!(ctx.env, AppSettings::default().session_env());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
