//! 读取图片文件头得到像素尺寸(PNG / JPEG / GIF / WebP),不解码整张图。
//!
//! @author ixxxxoooo

use std::io::Read;
use std::path::Path;

/// 读取图片的像素宽高;格式不支持或文件损坏返回 None。
///
/// 参数:`path` 图片路径。
pub fn read(path: &Path) -> Option<(u32, u32)> {
    let mut buf = Vec::new();
    // JPEG 的尺寸块可能在较后面的位置,读前 256KB 足够绝大多数文件
    std::fs::File::open(path).ok()?.take(256 * 1024).read_to_end(&mut buf).ok()?;
    parse(&buf)
}

/// 从文件头字节解析尺寸。
pub fn parse(b: &[u8]) -> Option<(u32, u32)> {
    let be32 = |i: usize| b.get(i..i + 4).map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]));
    let le16 = |i: usize| b.get(i..i + 2).map(|s| u32::from(u16::from_le_bytes([s[0], s[1]])));
    if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some((be32(16)?, be32(20)?));
    }
    if b.starts_with(b"GIF8") {
        return Some((le16(6)?, le16(8)?));
    }
    if b.len() > 30 && &b[0..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        return match &b[12..16] {
            b"VP8 " => Some((le16(26)? & 0x3fff, le16(28)? & 0x3fff)),
            b"VP8L" => {
                let v = u32::from_le_bytes([b[21], b[22], b[23], b[24]]);
                Some(((v & 0x3fff) + 1, ((v >> 14) & 0x3fff) + 1))
            }
            b"VP8X" => {
                let w = u32::from_le_bytes([b[24], b[25], b[26], 0]) + 1;
                let h = u32::from_le_bytes([b[27], b[28], b[29], 0]) + 1;
                Some((w, h))
            }
            _ => None,
        };
    }
    if b.starts_with(&[0xff, 0xd8]) {
        let mut i = 2;
        while i + 9 < b.len() {
            if b[i] != 0xff {
                i += 1;
                continue;
            }
            let m = b[i + 1];
            // SOF0..SOF15(除 DHT 0xC4、JPG 0xC8、DAC 0xCC)带尺寸
            if (0xc0..=0xcf).contains(&m) && ![0xc4, 0xc8, 0xcc].contains(&m) {
                let h = u32::from(u16::from_be_bytes([b[i + 5], b[i + 6]]));
                let w = u32::from(u16::from_be_bytes([b[i + 7], b[i + 8]]));
                return Some((w, h));
            }
            if m == 0xd8 || m == 0x01 || (0xd0..=0xd7).contains(&m) || m == 0xff {
                i += if m == 0xff { 1 } else { 2 };
                continue;
            }
            i += 2 + usize::from(u16::from_be_bytes([b[i + 2], b[i + 3]]));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 解析_png_与_gif() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend_from_slice(&640u32.to_be_bytes());
        png.extend_from_slice(&480u32.to_be_bytes());
        assert_eq!(parse(&png), Some((640, 480)));
        let mut gif = b"GIF89a".to_vec();
        gif.extend_from_slice(&[0x20, 0x01, 0x10, 0x00]);
        assert_eq!(parse(&gif), Some((288, 16)));
    }

    #[test]
    fn 解析_jpeg_跳过其它段() {
        // SOI + APP0(长度 4,2 字节内容)+ SOF0(高 100,宽 200)
        let b = [0xff, 0xd8, 0xff, 0xe0, 0x00, 0x04, 0x01, 0x02, 0xff, 0xc0, 0x00, 0x0b, 0x08, 0x00, 0x64, 0x00, 0xc8, 0x01];
        assert_eq!(parse(&b), Some((200, 100)));
        assert_eq!(parse(b"not an image at all, just text"), None);
    }
}
