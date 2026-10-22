//! 内置浏览器的内核:地址规范化、抓取、正文提取与历史记录。
//!
//! GPUI 没有 WebView,按「Zed 的方式」取舍:UCode 内置的是一个**阅读模式浏览器**——
//! 抓取页面后提取标题、正文结构与链接,用原生控件渲染;需要完整网页时一键交给系统浏览器。
//! 这样不引入第二套渲染引擎,也不执行任何网页脚本。

mod extract;
mod fetch;
mod history;

pub use extract::{Link, Page, PageBlock, extract};
pub use fetch::{BrowserError, Fetched, fetch, load, normalize_input};
pub use history::History;
