//! UCode 的伪终端:`portable-pty` 起 shell,`alacritty_terminal` 解析转义序列。
//!
//! 与界面无关:界面只需要 `Terminal::snapshot()` 拿到「按样式合并的文本段」来绘制,
//! 用 `keys::encode_key` 把按键变成字节写进去。
//!
//! @author ixxxxoooo

pub mod keys;
pub mod snapshot;
mod terminal;

pub use keys::{Mods, encode_key};
pub use snapshot::{Run, Snapshot, Style, TermColor};
pub use terminal::{SpawnOptions, Terminal};
