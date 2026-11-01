//! 工具结果里的图片缓存:把 base64 图片解码落盘,界面按路径渲染。
//!
//! 文件名取内容哈希,同一张图(含历史回放)只写一次。
//!
//! @author ixxxxoooo

use base64::Engine;
use serde_json::Value;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

/// 缓存目录:系统临时目录下的 `loomcode-images`。
fn cache_dir() -> PathBuf {
    std::env::temp_dir().join("loomcode-images")
}

/// 按 MIME 取扩展名。参数:`media_type` 如 `image/png`。返回值:扩展名。
fn ext_of(media_type: &str) -> &'static str {
    match media_type {
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        _ => "png",
    }
}

/// 把一个 `{"type":"image","source":{...}}` 内容块存成本地文件。
/// 参数:`block` 图片内容块。返回值:本地路径;不是 base64 或解码/写入失败时为 `None`。
pub fn store(block: &Value) -> Option<String> {
    let source = block.get("source")?;
    if source.get("type").and_then(Value::as_str) != Some("base64") {
        return None;
    }
    let data = source.get("data").and_then(Value::as_str)?;
    let media_type = source.get("media_type").and_then(Value::as_str).unwrap_or("image/png");
    let mut hasher = DefaultHasher::new();
    data.hash(&mut hasher);
    let path = cache_dir().join(format!("{:016x}.{}", hasher.finish(), ext_of(media_type)));
    if !path.exists() {
        let bytes = base64::engine::general_purpose::STANDARD.decode(data).ok()?;
        std::fs::create_dir_all(cache_dir()).ok()?;
        // 先写临时文件再改名,避免界面读到写了一半的图
        let tmp = path.with_extension("part");
        std::fs::write(&tmp, bytes).ok()?;
        std::fs::rename(&tmp, &path).ok()?;
    }
    Some(path.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn base64图片落盘且重复调用路径相同() {
        let data = base64::engine::general_purpose::STANDARD.encode(b"\x89PNG-test-bytes");
        let block = json!({"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": data}});
        let a = store(&block).expect("应落盘");
        let b = store(&block).expect("应落盘");
        assert_eq!(a, b);
        assert!(a.ends_with(".png"));
        assert_eq!(std::fs::read(&a).unwrap(), b"\x89PNG-test-bytes");
    }

    #[test]
    fn 非base64来源返回空() {
        assert!(store(&json!({"type": "image", "source": {"type": "url", "url": "https://x"}})).is_none());
    }
}
