//! 窗口位置与大小记忆:主窗口与独立 IDE 窗口各存一份到 `~/.loomcode/window.json`。
//! 与 `state.json` 分开存放,避免被工作台整体回写覆盖。
//!
//! @author ixxxxoooo

use gpui_kit::*;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// 防抖代数:每次窗口变化 +1,只有最后一次的延时任务才会写盘。
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// 各窗口最近一次的边界(还没写盘的也在内),退出时 `flush` 一并落盘,免得变动后马上退出丢掉。
static LATEST: Mutex<Option<HashMap<&'static str, WindowBounds>>> = Mutex::new(None);

/// 记忆文件路径。
fn file_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".loomcode").join("window.json"))
}

/// 读取整个记忆文件;不存在或损坏时返回空对象。
fn load_all() -> Value {
    file_path().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_else(|| json!({}))
}

/// 恢复窗口边界。
///
/// 参数:`key` 窗口标识(main / ide);`default` 默认宽高;`cx` 应用上下文。
/// 返回:有记忆且仍落在某个显示器内时用记忆值(含最大化状态),否则默认居中。
pub fn restore(key: &str, default: (f32, f32), cx: &App) -> WindowBounds {
    let fallback = WindowBounds::Windowed(Bounds::centered(None, size(px(default.0), px(default.1)), cx));
    let all = load_all();
    let Some(v) = all.get(key) else { return fallback };
    let num = |k: &str| v.get(k).and_then(Value::as_f64).map(|n| n as f32);
    let (Some(x), Some(y), Some(w), Some(h)) = (num("x"), num("y"), num("w"), num("h")) else { return fallback };
    if w < 640. || h < 480. {
        return fallback;
    }
    let bounds = Bounds::new(point(px(x), px(y)), size(px(w), px(h)));
    // 显示器拔掉后位置可能在屏幕外:必须与某个显示器有足够重叠才采用
    let visible = cx.displays().iter().any(|d| {
        let db = d.bounds();
        let (ix, iy) = (bounds.right().min(db.right()) - bounds.left().max(db.left()), bounds.bottom().min(db.bottom()) - bounds.top().max(db.top()));
        ix > px(120.) && iy > px(80.)
    });
    if !visible {
        return fallback;
    }
    if v.get("maximized").and_then(Value::as_bool).unwrap_or(false) { WindowBounds::Maximized(bounds) } else { WindowBounds::Windowed(bounds) }
}

/// 监听窗口位置 / 大小变化,停止变动 0.6 秒后写入记忆。
///
/// 参数:`key` 窗口标识;`window` / `cx` 视图的窗口与上下文。
pub fn track<T: 'static>(key: &'static str, window: &mut Window, cx: &mut Context<T>) {
    cx.observe_window_bounds(window, move |_, window, cx| {
        let wb = window.window_bounds();
        if let Ok(mut m) = LATEST.lock() {
            m.get_or_insert_with(HashMap::new).insert(key, wb);
        }
        let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
        cx.spawn(async move |_, cx| {
            cx.background_executor().timer(Duration::from_millis(600)).await;
            if GENERATION.load(Ordering::SeqCst) == generation {
                save(key, wb);
            }
        })
        .detach();
    })
    .detach();
}

/// 把还没写盘的窗口边界立刻写掉;应用退出时调用(防抖里的写入可能还没来得及执行)。
pub fn flush() {
    let pending: Vec<(&'static str, WindowBounds)> = LATEST.lock().ok().and_then(|mut m| m.take()).map(|m| m.into_iter().collect()).unwrap_or_default();
    for (key, wb) in pending {
        save(key, wb);
    }
}

/// 写入某个窗口的边界(先写临时文件再改名)。
fn save(key: &str, wb: WindowBounds) {
    let Some(path) = file_path() else { return };
    let b = wb.get_bounds();
    let maximized = matches!(wb, WindowBounds::Maximized(_));
    let mut all = load_all();
    all[key] = json!({ "x": f32::from(b.origin.x), "y": f32::from(b.origin.y), "w": f32::from(b.size.width), "h": f32::from(b.size.height), "maximized": maximized });
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, serde_json::to_string_pretty(&all).unwrap_or_default()).is_ok() {
        let _ = std::fs::rename(tmp, path);
    }
}
