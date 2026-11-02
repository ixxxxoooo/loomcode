//! Mermaid 排版内核(与界面无关):把 Mermaid 源码解析并排版成由矩形 / 多边形 / 文字组成的 `Scene`,
//! 界面只负责按主题给 `Role` 上色并绘制。目前支持流程图(graph / flowchart)、时序图、饼图、状态图;
//! 其余图表类型返回 `RenderError::Unsupported`,由界面回退成源码卡片。
//!
//! @author ixxxxoooo

mod flow;
mod flow_draw;
mod flow_layout;
mod flow_parse;
mod geom;
mod pie;
mod seq;
mod seq_draw;
mod seq_parse;
mod state;
mod text;

pub use text::text_w;

/// 图元的语义配色,由界面映射到当前主题。
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Role {
    /// 图的底色(用于给连线标签垫底)
    Surface,
    /// 节点填充
    Node,
    /// 节点 / 分组描边
    Border,
    /// 连线
    Line,
    /// 正文
    Text,
    /// 次要文字
    Muted,
    /// 便签 / 注释底色
    Note,
    /// 分组(子图、循环框)的淡底色
    Group,
    /// 调色板里的第 n 种强调色(饼图扇区等)
    Accent(u8),
}

/// 颜色:语义角色,或源码里指定的 RGB。
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Paint {
    Role(Role),
    Rgb(u32),
}

impl From<Role> for Paint {
    fn from(r: Role) -> Self {
        Paint::Role(r)
    }
}

/// 文字对齐锚点。
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Anchor {
    Start,
    Middle,
    End,
}

/// 一个图元。
#[derive(Clone, PartialEq, Debug)]
pub enum Item {
    /// 圆角矩形;`r` 为圆角半径
    Rect { x: f32, y: f32, w: f32, h: f32, r: f32, fill: Option<Paint>, stroke: Option<Paint>, width: f32 },
    /// 折线 / 多边形
    Poly { pts: Vec<(f32, f32)>, closed: bool, fill: Option<Paint>, stroke: Option<Paint>, width: f32 },
    /// 单行文字;`y` 为行的垂直中心
    Text { x: f32, y: f32, text: String, size: f32, bold: bool, anchor: Anchor, paint: Paint },
}

/// 排版结果。
#[derive(Clone, PartialEq, Debug)]
pub struct Scene {
    pub width: f32,
    pub height: f32,
    pub items: Vec<Item>,
}

/// 渲染失败的原因。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RenderError {
    #[error("暂不支持 {0} 图表")]
    Unsupported(String),
    #[error("语法错误:{0}")]
    Syntax(String),
    #[error("图太大(超过 {0} 个节点)")]
    TooLarge(usize),
    #[error("内容为空")]
    Empty,
}

/// 单张图的节点数上限。
pub const MAX_NODES: usize = 300;

impl Scene {
    /// 按图元的范围收紧画布:整体平移使最小坐标为 `margin`,宽高随之更新。
    pub fn fit(mut self, margin: f32) -> Self {
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        let mut grow = |a: f32, b: f32, c: f32, d: f32| {
            x0 = x0.min(a);
            y0 = y0.min(b);
            x1 = x1.max(c);
            y1 = y1.max(d);
        };
        for it in &self.items {
            match it {
                Item::Rect { x, y, w, h, .. } => grow(*x, *y, x + w, y + h),
                Item::Poly { pts, .. } => pts.iter().for_each(|p| grow(p.0, p.1, p.0, p.1)),
                Item::Text { x, y, text, size, anchor, .. } => {
                    let w = text_w(text, *size);
                    let left = match anchor {
                        Anchor::Start => *x,
                        Anchor::Middle => x - w / 2.,
                        Anchor::End => x - w,
                    };
                    grow(left, y - size, left + w, y + size);
                }
            }
        }
        if x0 > x1 {
            self.width = margin * 2.;
            self.height = margin * 2.;
            return self;
        }
        let (dx, dy) = (margin - x0, margin - y0);
        for it in &mut self.items {
            match it {
                Item::Rect { x, y, .. } | Item::Text { x, y, .. } => {
                    *x += dx;
                    *y += dy;
                }
                Item::Poly { pts, .. } => pts.iter_mut().for_each(|p| {
                    p.0 += dx;
                    p.1 += dy;
                }),
            }
        }
        self.width = x1 - x0 + margin * 2.;
        self.height = y1 - y0 + margin * 2.;
        self
    }
}

/// 图表类型(取自首行关键字)。
fn diagram_kind(src: &str) -> Option<String> {
    let mut lines = src.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with("%%"));
    let mut first = lines.next()?;
    // 跳过 YAML 前置元数据
    if first == "---" {
        first = lines.by_ref().skip_while(|l| *l != "---").nth(1)?;
    }
    Some(first.split(|c: char| c.is_whitespace() || c == ';').next().unwrap_or("").to_string())
}

/// 把 Mermaid 源码排版成图元场景。
///
/// 参数:`src` Mermaid 源码。
/// 返回:排版后的场景;不支持的图表类型或语法错误返回对应的 `RenderError`。
pub fn render(src: &str) -> Result<Scene, RenderError> {
    let kind = diagram_kind(src).ok_or(RenderError::Empty)?;
    let scene = match kind.to_ascii_lowercase().as_str() {
        "graph" | "flowchart" | "flowchart-elk" => flow::render(src)?,
        "sequencediagram" => seq::render(src)?,
        "pie" => pie::render(src)?,
        "statediagram" | "statediagram-v2" => state::render(src)?,
        _ => return Err(RenderError::Unsupported(kind)),
    };
    Ok(scene.fit(16.))
}

/// 源码对应的图表类型名(给界面做标题用);无法识别时为 `None`。
pub fn kind_title(src: &str) -> Option<&'static str> {
    Some(match diagram_kind(src)?.to_ascii_lowercase().as_str() {
        "graph" | "flowchart" | "flowchart-elk" => "流程图",
        "sequencediagram" => "时序图",
        "pie" => "饼图",
        "statediagram" | "statediagram-v2" => "状态图",
        _ => return None,
    })
}

#[cfg(test)]
mod tests;
