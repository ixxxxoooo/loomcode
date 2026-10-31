//! 文件 / 文件夹图标:沿用 OhMyAgent 的 Material Icon Theme 数据(`assets/file_icons.json`)。
//! 先按文件名、再按扩展名查图标名,图标是「颜色 + 路径」规格,转成 SVG 后当图片画出(彩色)。
//!
//! @author ixxxxoooo

use gpui_kit::*;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

/// 映射表与图标规格。
struct Catalog {
    extensions: HashMap<String, String>,
    file_names: HashMap<String, String>,
    folders: HashMap<String, String>,
    folders_open: HashMap<String, String>,
    icons: HashMap<String, String>,
    default_file: String,
    default_folder: String,
    default_folder_open: String,
}

/// 读取 JSON 里的字符串映射表。
fn string_map(v: &Value, key: &str) -> HashMap<String, String> {
    v[key].as_object().map(|o| o.iter().filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string()))).collect()).unwrap_or_default()
}

/// 全局映射表(首次使用时解析,约 0.5 MB)。
fn catalog() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let v: Value = serde_json::from_str(include_str!("../assets/file_icons.json")).unwrap_or(Value::Null);
        let s = |k: &str| v[k].as_str().unwrap_or_default().to_string();
        Catalog {
            extensions: string_map(&v, "extensions"),
            file_names: string_map(&v, "fileNames"),
            folders: string_map(&v, "folders"),
            folders_open: string_map(&v, "foldersOpen"),
            icons: string_map(&v, "icons"),
            default_file: s("defaultFile"),
            default_folder: s("defaultFolder"),
            default_folder_open: s("defaultFolderOpen"),
        }
    })
}

/// 解析图标名:先完整文件名,再扩展名,最后默认图标。
///
/// 参数:`name` 文件(夹)名;`is_dir` 是否文件夹;`open` 文件夹是否展开。
pub fn icon_name(name: &str, is_dir: bool, open: bool) -> String {
    let c = catalog();
    let lower = name.to_lowercase();
    if is_dir {
        let table = if open { &c.folders_open } else { &c.folders };
        return table.get(&lower).cloned().unwrap_or_else(|| if open { c.default_folder_open.clone() } else { c.default_folder.clone() });
    }
    if let Some(hit) = c.file_names.get(name).or_else(|| c.file_names.get(&lower)) {
        return hit.clone();
    }
    let ext = if lower.starts_with('.') && !lower[1..].contains('.') {
        &lower[1..]
    } else {
        match lower.rfind('.') {
            Some(d) if d > 0 && d + 1 < lower.len() => &lower[d + 1..],
            _ => "",
        }
    };
    c.extensions.get(ext).cloned().unwrap_or_else(|| c.default_file.clone())
}

/// 把图标规格转成 SVG:首行为视口边长(可带 viewBox 原点),其后每行「颜色 不透明度 path」。
pub fn spec_to_svg(spec: &str) -> String {
    let mut lines = spec.lines().filter(|l| !l.trim().is_empty());
    let Some(head) = lines.next() else { return String::new() };
    let h: Vec<&str> = head.split_whitespace().collect();
    let size: f32 = h.first().and_then(|s| s.parse().ok()).unwrap_or(16.);
    let (ox, oy) = if h.len() >= 3 { (h[1], h[2]) } else { ("0", "0") };
    let mut body = String::new();
    for line in lines {
        let mut it = line.trim().splitn(3, ' ');
        let (Some(color), Some(opacity), Some(d)) = (it.next(), it.next(), it.next()) else { continue };
        let alpha = if opacity == "1" { String::new() } else { format!(" fill-opacity=\"{opacity}\"") };
        body.push_str(&format!("<path fill=\"{color}\"{alpha} d=\"{d}\"/>"));
    }
    format!("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"{ox} {oy} {size} {size}\">{body}</svg>")
}

/// 按图标名取(并缓存)图片;找不到规格时退回默认文件图标。
fn icon_image(icon: &str) -> Arc<Image> {
    static CACHE: OnceLock<Mutex<HashMap<String, Arc<Image>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
    guard
        .entry(icon.to_string())
        .or_insert_with(|| {
            let c = catalog();
            let spec = c.icons.get(icon).or_else(|| c.icons.get(&c.default_file)).cloned().unwrap_or_default();
            Arc::new(Image::from_bytes(ImageFormat::Svg, spec_to_svg(&spec).into_bytes()))
        })
        .clone()
}

/// 文件(夹)图标元素。
///
/// 参数:`name` 名称;`is_dir`、`open` 同 [`icon_name`];`size` 边长(像素)。
pub fn file_icon_img(name: &str, is_dir: bool, open: bool, size: f32) -> Img {
    img(icon_image(&icon_name(name, is_dir, open))).size(px(size)).flex_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 按文件名与扩展名解析() {
        assert_eq!(icon_name("main.rs", false, false), "rust");
        assert_eq!(icon_name("Cargo.toml", false, false), icon_name("cargo.toml", false, false));
        assert_eq!(icon_name("zzz.unknownext", false, false), catalog().default_file);
        assert_ne!(icon_name("src", true, false), "");
        assert!(catalog().icons.contains_key(&icon_name("README.md", false, false)));
    }

    #[::core::prelude::v1::test]
    fn 规格转_svg() {
        let svg = spec_to_svg("16\n#ff0000 1 M0 0h16v16H0z\n#00ff00 0.5 M1 1h2v2H1z\n");
        assert!(svg.starts_with("<svg") && svg.contains("viewBox=\"0 0 16 16\""));
        assert!(svg.contains("fill=\"#ff0000\" d=\"M0 0h16v16H0z\"") && svg.contains("fill-opacity=\"0.5\""));
    }
}
