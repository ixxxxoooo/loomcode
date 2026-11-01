//! 地址规范化与页面抓取。

use crate::extract::{Page, PageBlock, extract};
use std::io::Read;
use std::time::Duration;

/// 响应体大小上限。
const MAX_BODY: u64 = 2 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum BrowserError {
    #[error("地址无效:{0}")]
    BadUrl(String),
    #[error("请求失败:{0}")]
    Request(String),
    #[error("服务器返回 {0}")]
    Status(u16),
}

/// 抓取结果。
#[derive(Debug, Clone)]
pub struct Fetched {
    /// 跟随重定向后的最终地址
    pub url: String,
    pub content_type: String,
    pub body: String,
}

/// 把地址栏输入变成可访问的 URL:带协议的原样使用;像域名的补 `https://`;其余当作搜索词。
pub fn normalize_input(input: &str) -> String {
    let s = input.trim();
    if s.starts_with("http://") || s.starts_with("https://") || s.starts_with("file://") {
        return s.to_string();
    }
    let host = s.split(['/', '?', '#']).next().unwrap_or_default();
    let looks_like_host = !s.contains(char::is_whitespace)
        && (host == "localhost" || host.starts_with("localhost:") || (host.contains('.') && !host.ends_with('.')));
    if looks_like_host {
        let scheme = if host.starts_with("localhost") || host.starts_with("127.") { "http" } else { "https" };
        return format!("{scheme}://{s}");
    }
    let q: String = url::form_urlencoded::byte_serialize(s.as_bytes()).collect();
    format!("https://duckduckgo.com/html/?q={q}")
}

/// 抓取选项。
#[derive(Debug, Clone)]
pub struct FetchOptions {
    /// 请求总超时(秒)
    pub timeout_secs: u64,
    /// 自定义 User-Agent;为空使用默认
    pub user_agent: String,
}

impl Default for FetchOptions {
    fn default() -> Self {
        Self { timeout_secs: 15, user_agent: String::new() }
    }
}

const DEFAULT_UA: &str = "Mozilla/5.0 (Macintosh) LoomCode/0.1 ReaderBrowser";

/// 用默认选项抓取一个页面(阻塞)。
pub fn fetch(url: &str) -> Result<Fetched, BrowserError> {
    fetch_with(url, &FetchOptions::default())
}

/// 抓取一个页面(阻塞)。
///
/// 参数:`url` 地址;`opts` 超时与 User-Agent。返回:响应正文与最终地址。
pub fn fetch_with(url: &str, opts: &FetchOptions) -> Result<Fetched, BrowserError> {
    let parsed = url::Url::parse(url).map_err(|e| BrowserError::BadUrl(e.to_string()))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(BrowserError::BadUrl(format!("不支持的协议:{}", parsed.scheme())));
    }
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(opts.timeout_secs.clamp(1, 120)))
        .redirects(5)
        .user_agent(if opts.user_agent.trim().is_empty() { DEFAULT_UA } else { opts.user_agent.trim() })
        .build();
    let resp = match agent.get(url).call() {
        Ok(r) => r,
        Err(ureq::Error::Status(code, _)) => return Err(BrowserError::Status(code)),
        Err(e) => return Err(BrowserError::Request(e.to_string())),
    };
    let final_url = resp.get_url().to_string();
    let content_type = resp.content_type().to_string();
    let mut bytes = Vec::new();
    resp.into_reader().take(MAX_BODY).read_to_end(&mut bytes).map_err(|e| BrowserError::Request(e.to_string()))?;
    Ok(Fetched { url: final_url, content_type, body: String::from_utf8_lossy(&bytes).into_owned() })
}

/// 用默认选项抓取并转换成可渲染的页面。
pub fn load(url: &str) -> Result<Page, BrowserError> {
    load_with(url, &FetchOptions::default())
}

/// 抓取并转换成可渲染的页面。非 HTML 内容(JSON、纯文本)以等宽文本展示。
pub fn load_with(url: &str, opts: &FetchOptions) -> Result<Page, BrowserError> {
    let f = fetch_with(url, opts)?;
    if f.content_type.contains("html") || f.body.trim_start().starts_with("<!") || f.body.trim_start().starts_with("<html") {
        let mut page = extract(&f.body, &f.url);
        page.url = f.url;
        Ok(page)
    } else {
        Ok(Page { url: f.url.clone(), title: f.url, blocks: vec![PageBlock::Code(f.body)], links: Vec::new() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;

    #[test]
    fn 地址规范化() {
        assert_eq!(normalize_input("https://a.com/x"), "https://a.com/x");
        assert_eq!(normalize_input("example.com/docs"), "https://example.com/docs");
        assert_eq!(normalize_input("localhost:3000"), "http://localhost:3000");
        assert_eq!(normalize_input("rust 异步 教程"), "https://duckduckgo.com/html/?q=rust+%E5%BC%82%E6%AD%A5+%E6%95%99%E7%A8%8B");
        assert_eq!(normalize_input("hello"), "https://duckduckgo.com/html/?q=hello");
    }

    /// 起一个只服务一次请求的本地 HTTP 服务。
    fn serve_once(content_type: &'static str, body: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 2048];
                let _ = std::io::Read::read(&mut stream, &mut buf);
                let resp = format!("HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                let _ = stream.write_all(resp.as_bytes());
            }
        });
        format!("http://{addr}/")
    }

    #[test]
    fn 抓取并提取_html() {
        let url = serve_once("text/html; charset=utf-8", "<html><head><title>你好</title></head><body><h1>标题</h1><p>正文 <a href=\"/next\">下一页</a></p></body></html>");
        let page = load(&url).unwrap();
        assert_eq!(page.title, "你好");
        assert!(page.links.iter().any(|l| l.url.ends_with("/next") && l.text == "下一页"));
    }

    #[test]
    fn 非_html_按文本展示() {
        let url = serve_once("application/json", "{\"a\":1}");
        let page = load(&url).unwrap();
        assert!(matches!(&page.blocks[0], PageBlock::Code(c) if c.contains("\"a\"")));
    }

    #[test]
    fn 拒绝非_http_协议() {
        assert!(matches!(fetch("ftp://x.com"), Err(BrowserError::BadUrl(_))));
        assert!(matches!(fetch("不是地址"), Err(BrowserError::BadUrl(_))));
    }
}
