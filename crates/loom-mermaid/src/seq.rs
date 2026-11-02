//! 时序图入口。

use crate::{RenderError, Scene, seq_draw, seq_parse};

pub fn render(src: &str) -> Result<Scene, RenderError> {
    let seq = seq_parse::parse(src)?;
    if seq.parts.len() > crate::MAX_NODES {
        return Err(RenderError::TooLarge(crate::MAX_NODES));
    }
    Ok(seq_draw::draw(&seq))
}
