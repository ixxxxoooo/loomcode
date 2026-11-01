//! Canvas 画布:标题 + 块列表的声明式文档。
//!
//! 块以 `type` 字段区分,JSON 形态稳定,既是 Agent 工具的入参,也是落盘格式。
//! 这里只放数据模型、规模校验与存储,渲染在界面层。

mod demo;
mod model;
mod store;

pub use demo::demo_document;
pub use model::*;
pub use store::{CanvasStore, CanvasSummary};
