//! 极简 HTML 正文提取:不执行脚本,只保留标题、段落、列表、代码与链接。

use url::Url;

/// 页面块。
#[derive(Debug, Clone, PartialEq)]
pub enum PageBlock {
    Heading { level: u8, text: String },
    Paragraph(String),
    ListItem(String),
    Quote(String),
    Code(String),
}

/// 页面中的链接。
#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    pub text: String,
    pub url: String,
}

/// 提取后的页面。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Page {
    pub url: String,
    pub title: String,
    pub blocks: Vec<PageBlock>,
    pub links: Vec<Link>,
}

/// 最多保留的链接数。
const MAX_LINKS: usize = 200;

#[derive(Clone, Copy, PartialEq)]
enum Ctx {
    Para,
    Heading(u8),
    Item,
    Quote,
    Pre,
}

/// 从 HTML 提取页面;`base` 用于把相对链接补全为绝对地址。
pub fn extract(html: &str, base: &str) -> Page {
    let base_url = Url::parse(base).ok();
    let mut page = Page { url: base.into(), ..Default::default() };
    let mut text = String::new();
    let mut ctx = Ctx::Para;
    let mut skip: Option<String> = None;
    let mut in_title = false;
    let mut title = String::new();
    // 正在收集文字的链接
    let mut link: Option<(String, String)> = None;

    let flush = |text: &mut String, ctx: Ctx, page: &mut Page| {
        let raw = std::mem::take(text);
        let t = if ctx == Ctx::Pre { raw.trim_matches('\n').to_string() } else { collapse(&raw) };
        if t.trim().is_empty() {
            return;
        }
        page.blocks.push(match ctx {
            Ctx::Para => PageBlock::Paragraph(t),
            Ctx::Heading(level) => PageBlock::Heading { level, text: t },
            Ctx::Item => PageBlock::ListItem(t),
            Ctx::Quote => PageBlock::Quote(t),
            Ctx::Pre => PageBlock::Code(t),
        });
    };

    let bytes = html.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'<' {
            // 注释
            if html[i..].starts_with("<!--") {
                i = html[i..].find("-->").map(|p| i + p + 3).unwrap_or(bytes.len());
                continue;
            }
            let Some(end) = html[i..].find('>').map(|p| i + p) else { break };
            let inner = &html[i + 1..end];
            i = end + 1;
            let closing = inner.starts_with('/');
            let name: String = inner.trim_start_matches('/').chars().take_while(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();

            if let Some(s) = &skip {
                if closing && *s == name {
                    skip = None;
                }
                continue;
            }
            match (closing, name.as_str()) {
                (false, "script" | "style" | "noscript" | "svg" | "template" | "iframe") => skip = Some(name),
                (false, "title") => in_title = true,
                (true, "title") => in_title = false,
                (false, "br") => text.push('\n'),
                (false, "a") => {
                    let href = attr(inner, "href").and_then(|h| resolve(&base_url, &h));
                    link = href.map(|h| (h, String::new()));
                }
                (true, "a") => {
                    if let Some((url, label)) = link.take() {
                        let label = collapse(&label);
                        if !label.is_empty() && page.links.len() < MAX_LINKS && !page.links.iter().any(|l| l.url == url) {
                            page.links.push(Link { text: label, url });
                        }
                    }
                }
                (false, "h1" | "h2" | "h3" | "h4" | "h5" | "h6") => {
                    flush(&mut text, ctx, &mut page);
                    ctx = Ctx::Heading(name.as_bytes()[1] - b'0');
                }
                (false, "li") => {
                    flush(&mut text, ctx, &mut page);
                    ctx = Ctx::Item;
                }
                (false, "pre") => {
                    flush(&mut text, ctx, &mut page);
                    ctx = Ctx::Pre;
                }
                (false, "blockquote") => {
                    flush(&mut text, ctx, &mut page);
                    ctx = Ctx::Quote;
                }
                (true, "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "li" | "pre" | "blockquote") => {
                    flush(&mut text, ctx, &mut page);
                    ctx = Ctx::Para;
                }
                (_, "p" | "div" | "section" | "article" | "tr" | "ul" | "ol" | "table" | "header" | "footer" | "nav" | "main" | "aside" | "form") => {
                    flush(&mut text, ctx, &mut page);
                }
                _ => {}
            }
        } else {
            let end = html[i..].find('<').map(|p| i + p).unwrap_or(bytes.len());
            if skip.is_none() {
                let chunk = decode_entities(&html[i..end]);
                if in_title {
                    title.push_str(&chunk);
                } else {
                    if let Some((_, label)) = link.as_mut() {
                        label.push_str(&chunk);
                    }
                    text.push_str(&chunk);
                }
            }
            i = end;
        }
    }
    flush(&mut text, ctx, &mut page);
    page.title = collapse(&title);
    if page.title.is_empty() {
        page.title = base.to_string();
    }
    page
}

/// 把连续空白折叠成一个空格。
fn collapse(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 读取标签属性值(支持单双引号)。
fn attr(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(p) = lower[from..].find(name) {
        let start = from + p;
        let before_ok = start == 0 || lower.as_bytes()[start - 1].is_ascii_whitespace();
        let rest = tag[start + name.len()..].trim_start();
        if before_ok && let Some(v) = rest.strip_prefix('=') {
            let v = v.trim_start();
            let quote = v.chars().next()?;
            return if quote == '"' || quote == '\'' {
                let body = &v[1..];
                body.find(quote).map(|e| decode_entities(&body[..e]))
            } else {
                Some(decode_entities(v.split_whitespace().next().unwrap_or_default().trim_end_matches('/')))
            };
        }
        from = start + name.len();
    }
    None
}

fn resolve(base: &Option<Url>, href: &str) -> Option<String> {
    let href = href.trim();
    if href.is_empty() || href.starts_with('#') || href.starts_with("javascript:") || href.starts_with("mailto:") {
        return None;
    }
    let url = match base {
        Some(b) => b.join(href).ok()?,
        None => Url::parse(href).ok()?,
    };
    matches!(url.scheme(), "http" | "https").then(|| url.to_string())
}

/// 解码常见 HTML 实体。
fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(p) = rest.find('&') {
        out.push_str(&rest[..p]);
        rest = &rest[p..];
        match rest.find(';').filter(|&e| e <= 10) {
            Some(e) => {
                let ent = &rest[1..e];
                let decoded = match ent {
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "quot" => Some('"'),
                    "apos" => Some('\''),
                    "nbsp" => Some(' '),
                    _ => ent
                        .strip_prefix("#x")
                        .or_else(|| ent.strip_prefix("#X"))
                        .and_then(|h| u32::from_str_radix(h, 16).ok())
                        .or_else(|| ent.strip_prefix('#').and_then(|d| d.parse().ok()))
                        .and_then(char::from_u32),
                };
                match decoded {
                    Some(c) => {
                        out.push(c);
                        rest = &rest[e + 1..];
                    }
                    None => {
                        out.push('&');
                        rest = &rest[1..];
                    }
                }
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const HTML: &str = r##"<!doctype html><html><head><title> 示例 &amp; 页面 </title><style>p{color:red}</style><script>var x = "<p>不应出现</p>";</script></head>
<body><!-- 注释 --><h1>大标题</h1><p>第一段 <b>加粗</b> 与 <a href="/docs/a">文档&nbsp;A</a>。</p>
<ul><li>条目一</li><li>条目二</li></ul><pre>fn main() {
    println!("hi");
}</pre><blockquote>引用&#20013;文</blockquote>
<a href="#top">锚点</a><a href="javascript:void(0)">脚本</a><a href="https://other.com/x?a=1&amp;b=2">外链</a><a href="/docs/a">重复</a></body></html>"##;

    #[test]
    fn 提取标题正文与结构() {
        let p = extract(HTML, "https://site.com/index.html");
        assert_eq!(p.title, "示例 & 页面");
        assert_eq!(p.blocks[0], PageBlock::Heading { level: 1, text: "大标题".into() });
        assert_eq!(p.blocks[1], PageBlock::Paragraph("第一段 加粗 与 文档 A。".into()));
        assert_eq!(p.blocks[2], PageBlock::ListItem("条目一".into()));
        assert_eq!(p.blocks[3], PageBlock::ListItem("条目二".into()));
        assert!(matches!(&p.blocks[4], PageBlock::Code(c) if c.contains("    println!(\"hi\");")), "pre 保留缩进");
        assert_eq!(p.blocks[5], PageBlock::Quote("引用中文".into()));
        assert!(!format!("{:?}", p.blocks).contains("不应出现"), "脚本内容被丢弃");
        assert!(!format!("{:?}", p.blocks).contains("color"), "样式被丢弃");
    }

    #[test]
    fn 链接补全去重并过滤() {
        let p = extract(HTML, "https://site.com/index.html");
        let urls: Vec<_> = p.links.iter().map(|l| l.url.as_str()).collect();
        assert_eq!(urls, ["https://site.com/docs/a", "https://other.com/x?a=1&b=2"], "锚点与脚本链接被忽略,重复链接只留一个");
        assert_eq!(p.links[0].text, "文档 A");
    }

    #[test]
    fn 实体解码() {
        assert_eq!(decode_entities("a &lt; b &amp;&amp; c &#x4E2D; &#20013; &bogus; &"), "a < b && c 中 中 &bogus; &");
    }

    #[test]
    fn 无标题时回退为地址() {
        let p = extract("<p>x</p>", "https://a.com/");
        assert_eq!(p.title, "https://a.com/");
    }
}
