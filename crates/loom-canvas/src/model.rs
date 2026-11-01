//! 画布文档模型与规模校验。

use serde::{Deserialize, Serialize};

/// 文档规模上限,防止模型一次生成过大的画布拖垮界面。
pub mod limits {
    /// 块总数上限(含分区内嵌套)
    pub const MAX_BLOCKS: usize = 200;
    /// 表格行数上限
    pub const MAX_TABLE_ROWS: usize = 2000;
    /// 表格列数上限
    pub const MAX_TABLE_COLUMNS: usize = 50;
    /// 图表数据点上限(所有系列合计)
    pub const MAX_CHART_POINTS: usize = 1000;
    /// 单段文本字符数上限
    pub const MAX_TEXT_LENGTH: usize = 100_000;
    /// 分区嵌套深度上限
    pub const MAX_DEPTH: usize = 3;
}

/// 色调。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Tone {
    #[default]
    Neutral,
    Info,
    Success,
    Warning,
    Danger,
}

/// 统计卡里的一项。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stat {
    pub label: String,
    pub value: String,
    /// 变化说明(如 `+12%`)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta: Option<String>,
    #[serde(default)]
    pub tone: Tone,
}

/// 图表种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChartKind {
    Bar,
    Line,
    Area,
    Pie,
    /// 蜡烛图(K 线),数据在 `Chart::candles`
    Candle,
}

/// 图表系列。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Series {
    pub name: String,
    pub values: Vec<f64>,
}

/// 一根蜡烛。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Candle {
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
}

fn yes() -> bool {
    true
}

/// 图表:`labels` 为横轴(饼图为扇区名、蜡烛图为日期),每个系列的 `values` 与之等长;
/// 蜡烛图用 `candles`(与 `labels` 等长)而不是 `series`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Chart {
    pub kind: ChartKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub labels: Vec<String>,
    #[serde(default)]
    pub series: Vec<Series>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candles: Vec<Candle>,
    /// 均线周期(仅蜡烛图),按收盘价计算
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub moving_averages: Vec<u32>,
    /// 红涨绿跌(默认 true,A 股习惯)
    #[serde(default = "yes")]
    pub up_is_red: bool,
}

impl Chart {
    /// 数据点总数。
    pub fn point_count(&self) -> usize {
        self.series.iter().map(|s| s.values.len()).sum::<usize>() + self.candles.len()
    }
}

/// 画布块。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Block {
    Heading { text: String, #[serde(default = "one")] level: u8 },
    Markdown { text: String },
    Stats { items: Vec<Stat> },
    Table {
        columns: Vec<String>,
        rows: Vec<Vec<String>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
    },
    Chart(ChartBlock),
    Callout {
        #[serde(default)]
        tone: Tone,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        text: String,
    },
    Code {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        language: Option<String>,
        text: String,
    },
    /// 图片(本机路径或 http(s) 地址)
    Image {
        source: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        caption: Option<String>,
    },
    Mermaid { text: String },
    Section {
        title: String,
        #[serde(default)]
        collapsed: bool,
        blocks: Vec<Block>,
    },
    Divider,
}

fn one() -> u8 {
    1
}

/// 图表块包装:让图表字段平铺在块上(与其它块的 JSON 形态一致)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChartBlock {
    #[serde(flatten)]
    pub chart: Chart,
}

impl Block {
    /// 块类型名(JSON 的 `type`)。
    pub fn type_name(&self) -> &'static str {
        match self {
            Block::Heading { .. } => "heading",
            Block::Markdown { .. } => "markdown",
            Block::Stats { .. } => "stats",
            Block::Table { .. } => "table",
            Block::Chart(_) => "chart",
            Block::Callout { .. } => "callout",
            Block::Code { .. } => "code",
            Block::Image { .. } => "image",
            Block::Mermaid { .. } => "mermaid",
            Block::Section { .. } => "section",
            Block::Divider => "divider",
        }
    }
}

/// 画布文档。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanvasDocument {
    pub id: String,
    pub title: String,
    pub blocks: Vec<Block>,
    #[serde(default)]
    pub project_path: String,
    /// 创建时间(Unix 秒)
    #[serde(default)]
    pub created_at: i64,
    #[serde(default)]
    pub updated_at: i64,
    /// 每次更新递增
    #[serde(default)]
    pub revision: u32,
}

/// 校验失败原因。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CanvasError {
    #[error("画布块过多(超过 {0} 个)")]
    TooManyBlocks(usize),
    #[error("表格过大:{0}")]
    TableTooLarge(String),
    #[error("图表数据点过多(超过 {0} 个)")]
    TooManyPoints(usize),
    #[error("文本过长(超过 {0} 字符)")]
    TextTooLong(usize),
    #[error("分区嵌套过深(超过 {0} 层)")]
    TooDeep(usize),
    #[error("图表数据不一致:{0}")]
    BadChart(String),
    #[error("{0}")]
    Io(String),
    #[error("文档格式错误:{0}")]
    Format(String),
}

impl CanvasDocument {
    /// 块总数(含分区内嵌套)。
    pub fn block_count(&self) -> usize {
        fn count(blocks: &[Block]) -> usize {
            blocks.iter().map(|b| 1 + if let Block::Section { blocks, .. } = b { count(blocks) } else { 0 }).sum()
        }
        count(&self.blocks)
    }

    /// 校验规模与图表一致性。
    pub fn validate(&self) -> Result<(), CanvasError> {
        if self.block_count() > limits::MAX_BLOCKS {
            return Err(CanvasError::TooManyBlocks(limits::MAX_BLOCKS));
        }
        fn check(blocks: &[Block], depth: usize) -> Result<(), CanvasError> {
            if depth > limits::MAX_DEPTH {
                return Err(CanvasError::TooDeep(limits::MAX_DEPTH));
            }
            let text_ok = |t: &str| {
                if t.chars().count() > limits::MAX_TEXT_LENGTH { Err(CanvasError::TextTooLong(limits::MAX_TEXT_LENGTH)) } else { Ok(()) }
            };
            for b in blocks {
                match b {
                    Block::Markdown { text } | Block::Mermaid { text } | Block::Code { text, .. } | Block::Callout { text, .. } => text_ok(text)?,
                    Block::Table { columns, rows, .. } => {
                        if columns.len() > limits::MAX_TABLE_COLUMNS {
                            return Err(CanvasError::TableTooLarge(format!("列数超过 {}", limits::MAX_TABLE_COLUMNS)));
                        }
                        if rows.len() > limits::MAX_TABLE_ROWS {
                            return Err(CanvasError::TableTooLarge(format!("行数超过 {}", limits::MAX_TABLE_ROWS)));
                        }
                    }
                    Block::Chart(ChartBlock { chart }) => {
                        if chart.point_count() > limits::MAX_CHART_POINTS {
                            return Err(CanvasError::TooManyPoints(limits::MAX_CHART_POINTS));
                        }
                        if chart.kind == ChartKind::Candle {
                            if chart.candles.len() != chart.labels.len() {
                                return Err(CanvasError::BadChart("蜡烛数量与标签数量不一致".into()));
                            }
                        } else if let Some(s) = chart.series.iter().find(|s| s.values.len() != chart.labels.len()) {
                            return Err(CanvasError::BadChart(format!("系列「{}」的数据长度与标签不一致", s.name)));
                        }
                    }
                    Block::Section { blocks, .. } => check(blocks, depth + 1)?,
                    _ => {}
                }
            }
            Ok(())
        }
        check(&self.blocks, 1)
    }

    /// 从 JSON 解析并校验。
    pub fn from_json(json: &str) -> Result<Self, CanvasError> {
        let doc: Self = serde_json::from_str(json).map_err(|e| CanvasError::Format(e.to_string()))?;
        doc.validate()?;
        Ok(doc)
    }

    /// 序列化为易读的 JSON(源码视图使用)。
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }
}

/// 计算简单移动平均;前 `period - 1` 项为 `None`。
pub fn moving_average(closes: &[f64], period: usize) -> Vec<Option<f64>> {
    closes
        .iter()
        .enumerate()
        .map(|(i, _)| {
            if period == 0 || i + 1 < period {
                None
            } else {
                Some(closes[i + 1 - period..=i].iter().sum::<f64>() / period as f64)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(blocks: Vec<Block>) -> CanvasDocument {
        CanvasDocument { id: "d".into(), title: "t".into(), blocks, project_path: String::new(), created_at: 0, updated_at: 0, revision: 1 }
    }

    fn bar(values: Vec<f64>, labels: usize) -> Block {
        Block::Chart(ChartBlock {
            chart: Chart {
                kind: ChartKind::Bar,
                title: None,
                labels: (0..labels).map(|i| i.to_string()).collect(),
                series: vec![Series { name: "s".into(), values }],
                candles: vec![],
                moving_averages: vec![],
                up_is_red: true,
            },
        })
    }

    #[test]
    fn json_往返并平铺图表字段() {
        let d = doc(vec![
            Block::Heading { text: "标题".into(), level: 1 },
            bar(vec![1., 2.], 2),
            Block::Divider,
            Block::Callout { tone: Tone::Warning, title: None, text: "注意".into() },
        ]);
        let json = d.to_json();
        assert!(json.contains(r#""type": "chart""#));
        assert!(json.contains(r#""kind": "bar""#), "图表字段平铺在块上");
        assert_eq!(CanvasDocument::from_json(&json).unwrap(), d);
    }

    #[test]
    fn 缺省字段取默认值() {
        let d = CanvasDocument::from_json(r#"{"id":"a","title":"t","blocks":[{"type":"callout","text":"x"},{"type":"heading","text":"h"},{"type":"chart","kind":"candle","labels":["d"],"candles":[{"open":1,"high":2,"low":0,"close":1.5}]}]}"#).unwrap();
        assert!(matches!(&d.blocks[0], Block::Callout { tone: Tone::Neutral, .. }));
        assert!(matches!(&d.blocks[1], Block::Heading { level: 1, .. }));
        assert!(matches!(&d.blocks[2], Block::Chart(c) if c.chart.up_is_red));
    }

    #[test]
    fn 校验图表一致性与规模() {
        assert!(matches!(doc(vec![bar(vec![1.], 2)]).validate(), Err(CanvasError::BadChart(_))));
        assert!(matches!(doc(vec![bar(vec![0.; 1001], 1001)]).validate(), Err(CanvasError::TooManyPoints(_))));
        assert!(matches!(doc(vec![Block::Divider; 201]).validate(), Err(CanvasError::TooManyBlocks(_))));
        let deep = Block::Section { title: "1".into(), collapsed: false, blocks: vec![Block::Section { title: "2".into(), collapsed: false, blocks: vec![Block::Section { title: "3".into(), collapsed: false, blocks: vec![Block::Section { title: "4".into(), collapsed: false, blocks: vec![] }] }] }] };
        assert!(matches!(doc(vec![deep]).validate(), Err(CanvasError::TooDeep(_))));
        let wide = Block::Table { columns: vec!["c".into(); 51], rows: vec![], title: None };
        assert!(matches!(doc(vec![wide]).validate(), Err(CanvasError::TableTooLarge(_))));
    }

    #[test]
    fn 块计数包含嵌套() {
        let d = doc(vec![Block::Section { title: "s".into(), collapsed: false, blocks: vec![Block::Divider, Block::Divider] }, Block::Divider]);
        assert_eq!(d.block_count(), 4);
    }

    #[test]
    fn 移动平均() {
        let ma = moving_average(&[1., 2., 3., 4.], 2);
        assert_eq!(ma, vec![None, Some(1.5), Some(2.5), Some(3.5)]);
    }
}
