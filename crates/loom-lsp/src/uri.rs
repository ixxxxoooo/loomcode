//! 文件路径与 `file://` URI 互转(百分号编码,不依赖外部 crate)。

use std::path::{Path, PathBuf};

fn unreserved(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~' | b'/')
}

/// 路径 → `file:///…`。
pub fn from_path(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut s = String::from("file://");
    for &b in path.as_os_str().as_bytes() {
        if unreserved(b) {
            s.push(b as char);
        } else {
            s.push_str(&format!("%{b:02X}"));
        }
    }
    s
}

/// `file:///…` → 路径;不是 file 协议返回 `None`。
pub fn to_path(uri: &str) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    let rest = uri.strip_prefix("file://")?;
    // 去掉可能的 authority(`file://localhost/…`)
    let rest = if rest.starts_with('/') { rest } else { &rest[rest.find('/')?..] };
    let b = rest.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 3 <= b.len() {
            if let Some(v) = rest.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    Some(PathBuf::from(std::ffi::OsString::from_vec(out)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 往返含空格与中文() {
        let p = Path::new("/tmp/我的 项目/a#b.rs");
        let u = from_path(p);
        assert!(u.starts_with("file:///tmp/%E6%88%91"));
        assert!(u.contains("%20") && u.contains("%23"));
        assert_eq!(to_path(&u).unwrap(), p);
    }

    #[test]
    fn 非_file_协议() {
        assert_eq!(to_path("https://x.com/a"), None);
        assert_eq!(to_path("file:///a/b").unwrap(), Path::new("/a/b"));
    }
}
