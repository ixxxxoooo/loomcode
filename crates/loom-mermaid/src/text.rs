//! 文字度量与标签清洗。

/// 单个字符的宽度(以字号为单位的估算)。
fn char_em(c: char) -> f32 {
    match c {
        ' ' => 0.3,
        'i' | 'l' | 'j' | 'I' | '.' | ',' | ':' | ';' | '|' | '\'' | '!' | '`' => 0.3,
        'f' | 't' | 'r' | '(' | ')' | '[' | ']' | '{' | '}' | '-' | '/' | '\\' => 0.4,
        'm' | 'w' | 'M' | 'W' | '@' | '%' => 0.85,
        'A'..='Z' => 0.68,
        c if c.is_ascii() => 0.56,
        // 中日韩与全角符号按一个字宽
        _ => 1.0,
    }
}

/// 估算一行文字的宽度(像素)。
pub fn text_w(s: &str, size: f32) -> f32 {
    s.chars().map(char_em).sum::<f32>() * size
}

/// 把标签按换行(`\n`、`<br>`、`<br/>`)拆成多行,并清洗引号与简单的 HTML 标记。
pub fn split_lines(s: &str) -> Vec<String> {
    let s = s.trim().trim_matches('"').trim();
    let mut t = s.replace("\\n", "\n");
    for br in ["<br/>", "<br />", "<br>", "<BR/>", "<BR>"] {
        t = t.replace(br, "\n");
    }
    let lines: Vec<String> = t.split('\n').map(|l| strip_tags(l).trim().to_string()).collect();
    if lines.iter().all(String::is_empty) { vec![String::new()] } else { lines }
}

/// 去掉 `<b>` 之类的标记并还原常见实体。
fn strip_tags(s: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' if s.contains('>') => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if in_tag => {}
            _ => out.push(c),
        }
    }
    out.replace("&quot;", "\"").replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&").replace("#35;", "#").replace("#quot;", "\"")
}

/// 多行文字的最大宽度。
pub fn lines_w(lines: &[String], size: f32) -> f32 {
    lines.iter().map(|l| text_w(l, size)).fold(0., f32::max)
}

/// 解析 `#rgb` / `#rrggbb` / 常见颜色名。
pub fn parse_color(s: &str) -> Option<u32> {
    let s = s.trim();
    if let Some(h) = s.strip_prefix('#') {
        return match h.len() {
            3 => {
                let v = u32::from_str_radix(h, 16).ok()?;
                let (r, g, b) = ((v >> 8) & 0xf, (v >> 4) & 0xf, v & 0xf);
                Some((r * 17) << 16 | (g * 17) << 8 | b * 17)
            }
            6 => u32::from_str_radix(h, 16).ok(),
            _ => None,
        };
    }
    Some(match s.to_ascii_lowercase().as_str() {
        "white" => 0xffffff,
        "black" => 0x000000,
        "red" => 0xe5484d,
        "green" => 0x30a46c,
        "blue" => 0x3e63dd,
        "yellow" => 0xf5d90a,
        "orange" => 0xf76b15,
        "gray" | "grey" => 0x8b8d98,
        "pink" => 0xd6409f,
        "purple" => 0x8e4ec6,
        _ => return None,
    })
}

/// 在给定底色上更易读的文字色(黑或白)。
pub fn contrast(rgb: u32) -> u32 {
    let (r, g, b) = ((rgb >> 16) & 0xff, (rgb >> 8) & 0xff, rgb & 0xff);
    if (r * 299 + g * 587 + b * 114) / 1000 > 140 { 0x1a1a1a } else { 0xffffff }
}
