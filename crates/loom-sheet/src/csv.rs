//! CSV / TSV 解析:自动识别编码(BOM、UTF-8、GBK 等)与分隔符,支持引号、转义引号与引号内换行。

use crate::model::{Cell, CellKind, Sheet};
use crate::{MAX_COLUMNS, MAX_ROWS};

/// 把文件字节解码成文本:先看 BOM,再试 UTF-8,不是 UTF-8 就用编码探测(中文常见的 GBK 等)。
pub fn decode(bytes: &[u8]) -> String {
    if let Some((enc, bom)) = encoding_rs::Encoding::for_bom(bytes) {
        return enc.decode_without_bom_handling(&bytes[bom..]).0.into_owned();
    }
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_string();
    }
    let mut det = chardetng::EncodingDetector::new();
    det.feed(&bytes[..bytes.len().min(64 * 1024)], bytes.len() <= 64 * 1024);
    det.guess(None, true).decode_without_bom_handling(bytes).0.into_owned()
}

/// 猜分隔符:看前若干行里 `, \t ; |` 哪个出现最多(忽略引号内的),扩展名是 `.tsv` 时由调用方直接指定。
pub fn sniff_delimiter(text: &str) -> char {
    let mut best = (',', 0usize);
    for d in [',', '\t', ';', '|'] {
        let mut count = 0;
        let mut in_q = false;
        let mut lines = 0;
        for ch in text.chars() {
            match ch {
                '"' => in_q = !in_q,
                '\n' if !in_q => {
                    lines += 1;
                    if lines >= 20 {
                        break;
                    }
                }
                c if c == d && !in_q => count += 1,
                _ => {}
            }
        }
        if count > best.1 {
            best = (d, count);
        }
    }
    best.0
}

/// 解析成行列(字符串矩阵)。引号内的分隔符与换行保留,`""` 还原成 `"`;行尾 `\r\n` 与 `\n` 都认。
pub fn parse_rows(text: &str, delim: char) -> (Vec<Vec<String>>, bool) {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let (mut rows, mut row, mut field) = (Vec::new(), Vec::new(), String::new());
    let (mut in_q, mut truncated) = (false, false);
    let mut chars = text.chars().peekable();
    let finish_row = |row: &mut Vec<String>, field: &mut String, rows: &mut Vec<Vec<String>>, truncated: &mut bool| {
        row.push(std::mem::take(field));
        if row.len() > MAX_COLUMNS {
            row.truncate(MAX_COLUMNS);
            *truncated = true;
        }
        rows.push(std::mem::take(row));
    };
    while let Some(c) = chars.next() {
        if in_q {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    field.push('"');
                    chars.next();
                }
                '"' => in_q = false,
                _ => field.push(c),
            }
            continue;
        }
        match c {
            '"' if field.is_empty() => in_q = true,
            c if c == delim => row.push(std::mem::take(&mut field)),
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                finish_row(&mut row, &mut field, &mut rows, &mut truncated);
            }
            '\n' => finish_row(&mut row, &mut field, &mut rows, &mut truncated),
            _ => field.push(c),
        }
        if rows.len() >= MAX_ROWS {
            return (rows, true);
        }
    }
    if !field.is_empty() || !row.is_empty() {
        finish_row(&mut row, &mut field, &mut rows, &mut truncated);
    }
    (rows, truncated)
}

/// 读取并解析一个分隔文本文件为工作表。
///
/// 参数:`bytes` 文件内容;`name` 工作表名(文件名);`tsv` 为真时固定用制表符。
pub fn parse(bytes: &[u8], name: &str, tsv: bool) -> Sheet {
    let text = decode(bytes);
    let delim = if tsv { '\t' } else { sniff_delimiter(&text) };
    let (rows, truncated) = parse_rows(&text, delim);
    let rows = rows
        .into_iter()
        .map(|r| {
            let mut cells: Vec<Cell> = r.into_iter().map(|t| Cell { kind: if is_number(&t) { CellKind::Number } else { CellKind::Text }, text: t }).collect();
            while cells.last().is_some_and(|c| c.text.is_empty()) {
                cells.pop();
            }
            cells
        })
        .collect();
    Sheet::new(name, rows, truncated)
}

/// 文本是不是数字(右对齐用):忽略千分位、百分号、货币符号。
fn is_number(t: &str) -> bool {
    let c: String = t.chars().filter(|c| !",%¥$€£ ".contains(*c)).collect();
    !c.is_empty() && c.parse::<f64>().is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 引号逗号换行与转义() {
        let (rows, _) = parse_rows("a,\"b,c\",\"d\"\"e\"\n\"x\ny\",2\r\nlast", ',');
        assert_eq!(rows[0], vec!["a", "b,c", "d\"e"]);
        assert_eq!(rows[1], vec!["x\ny", "2"]);
        assert_eq!(rows[2], vec!["last"]);
    }

    #[test]
    fn 自动识别分隔符() {
        assert_eq!(sniff_delimiter("a;b;c\n1;2;3\n"), ';');
        assert_eq!(sniff_delimiter("a\tb\n1\t2\n"), '\t');
        assert_eq!(sniff_delimiter("\"a,b\"|c\n"), '|');
        assert_eq!(sniff_delimiter("单列\n"), ',');
    }

    #[test]
    fn 识别_gbk_与_bom() {
        let (gbk, _, _) = encoding_rs::GBK.encode("姓名,年龄\n张三,30\n");
        assert_eq!(decode(&gbk), "姓名,年龄\n张三,30\n");
        let mut bom = vec![0xEF, 0xBB, 0xBF];
        bom.extend("a,b".as_bytes());
        assert_eq!(decode(&bom), "a,b");
    }

    #[test]
    fn 解析成工作表_去掉行尾空单元格并标数字() {
        let s = parse("名称,数量,\n苹果,1,000\n".as_bytes(), "t.csv", false);
        assert_eq!(s.rows[0].len(), 2);
        assert_eq!(s.rows[1][0].kind, CellKind::Text);
        assert_eq!(s.column_count, 3);
        let s = parse("a\t1\n".as_bytes(), "t.tsv", true);
        assert_eq!(s.rows[0][1].kind, CellKind::Number);
    }

    #[test]
    fn 行数上限截断() {
        let big = "a\n".repeat(MAX_ROWS + 10);
        let (rows, truncated) = parse_rows(&big, ',');
        assert!(truncated && rows.len() == MAX_ROWS);
    }
}
