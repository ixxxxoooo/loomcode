//! 服务商配置与密钥的读写。密钥文件权限设为 0600,内容不写日志、不回显。
//!
//! @author ixxxxoooo

use super::ProviderStore;
use std::collections::HashMap;
use std::io::Result;
use std::path::{Path, PathBuf};

/// 原子写入文本,可选设置 0600 权限。
fn write_atomic(path: &Path, text: &str, private: bool) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, text)?;
    #[cfg(unix)]
    if private {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    let _ = private;
    std::fs::rename(&tmp, path)
}

impl ProviderStore {
    /// 读取;不存在或损坏时返回空配置。
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    /// 写入。
    pub fn save(&self, path: &Path) -> Result<()> {
        write_atomic(path, &serde_json::to_string_pretty(self).map_err(std::io::Error::other)?, false)
    }
}

/// 读取密钥表(服务商 id → API Key)。
pub fn load_keys(path: &Path) -> HashMap<String, String> {
    std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

/// 写入密钥表(0600)。
pub fn save_keys(path: &Path, keys: &HashMap<String, String>) -> Result<()> {
    write_atomic(path, &serde_json::to_string_pretty(keys).map_err(std::io::Error::other)?, true)
}
