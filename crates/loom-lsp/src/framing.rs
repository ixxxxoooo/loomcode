//! LSP 的消息帧:`Content-Length: N\r\n\r\n` + N 字节 JSON。

use serde_json::Value;
use std::io::{self, BufRead, Write};

/// 把一条 JSON 消息编码成带头部的字节。
pub fn encode(msg: &Value) -> Vec<u8> {
    let body = msg.to_string();
    let mut out = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
    out.extend_from_slice(body.as_bytes());
    out
}

/// 写出一条消息并刷新。
pub fn write_message(w: &mut impl Write, msg: &Value) -> io::Result<()> {
    w.write_all(&encode(msg))?;
    w.flush()
}

/// 读取一条消息;流结束返回 `Ok(None)`。头部里除 `Content-Length` 外的字段(如 `Content-Type`)忽略。
pub fn read_message(r: &mut impl BufRead) -> io::Result<Option<Value>> {
    let mut len: Option<usize> = None;
    let mut seen_header = false;
    loop {
        let mut line = String::new();
        if r.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            if len.is_some() {
                break;
            }
            if seen_header {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "缺少 Content-Length"));
            }
            // 头部前的多余空行:跳过
            continue;
        }
        seen_header = true;
        if let Some((k, v)) = line.split_once(':') {
            if k.trim().eq_ignore_ascii_case("content-length") {
                len = v.trim().parse().ok();
            }
        }
    }
    let len = len.unwrap_or(0);
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    serde_json::from_slice(&buf).map(Some).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Cursor;

    #[test]
    fn 编码解码往返() {
        let msg = json!({"jsonrpc":"2.0","id":1,"method":"x","params":{"s":"中文"}});
        let bytes = encode(&msg);
        // 长度按字节计,中文占 3 字节
        assert!(String::from_utf8_lossy(&bytes).starts_with(&format!("Content-Length: {}", msg.to_string().len())));
        let mut r = Cursor::new(bytes);
        assert_eq!(read_message(&mut r).unwrap(), Some(msg));
        assert_eq!(read_message(&mut r).unwrap(), None);
    }

    #[test]
    fn 连续两条与多余头部() {
        let a = json!({"a":1});
        let b = json!({"b":2});
        let mut bytes = b"Content-Type: application/vscode-jsonrpc; charset=utf-8\r\n".to_vec();
        bytes.extend(encode(&a));
        bytes.extend(encode(&b));
        let mut r = Cursor::new(bytes);
        assert_eq!(read_message(&mut r).unwrap(), Some(a));
        assert_eq!(read_message(&mut r).unwrap(), Some(b));
    }

    #[test]
    fn 缺少长度报错() {
        let mut r = Cursor::new(b"X: 1\r\n\r\n{}".to_vec());
        assert!(read_message(&mut r).is_err());
    }
}
