//! LSP 位置换算。LSP 的列默认按 UTF-16 码元计,而编辑器内部的 `Position.character` 按字符(Unicode 标量)计,
//! 行里有非 ASCII 字符(中文、emoji)时两者不同,必须换算,否则悬停 / 跳转会偏位。

/// 字符列 → UTF-16 列。`col` 超出行尾时按行尾算。
pub fn chars_to_utf16(line: &str, col: usize) -> usize {
    line.chars().take(col).map(char::len_utf16).sum()
}

/// UTF-16 列 → 字符列。落在代理对中间时向前取整;超出行尾按行尾算。
pub fn utf16_to_chars(line: &str, col16: usize) -> usize {
    let mut acc = 0;
    for (n, c) in line.chars().enumerate() {
        if acc >= col16 {
            return n;
        }
        acc += c.len_utf16();
        if acc > col16 {
            return n;
        }
    }
    line.chars().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_不变() {
        assert_eq!(chars_to_utf16("hello", 3), 3);
        assert_eq!(utf16_to_chars("hello", 3), 3);
    }

    #[test]
    fn 中文一个码元_emoji_两个() {
        let l = "a中😀b";
        assert_eq!(chars_to_utf16(l, 3), 4); // a(1)+中(1)+😀(2)
        assert_eq!(utf16_to_chars(l, 4), 3);
        assert_eq!(utf16_to_chars(l, 3), 2); // 落在 emoji 中间 → 取其起点
        assert_eq!(utf16_to_chars(l, 99), 4);
        assert_eq!(chars_to_utf16(l, 99), 5);
    }
}
