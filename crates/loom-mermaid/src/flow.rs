//! 流程图入口:解析 → 排版 → 绘制。

use crate::{RenderError, Scene, flow_draw, flow_parse};

pub fn render(src: &str) -> Result<Scene, RenderError> {
    let flow = flow_parse::parse(src)?;
    if flow.nodes.is_empty() {
        return Err(RenderError::Empty);
    }
    Ok(flow_draw::draw(&flow))
}
