//! 演示画布:覆盖全部块类型,与演示场景 `canvas` 对应。

use crate::model::*;

/// 演示画布的固定 id。
pub const DEMO_ID: &str = "canvas-q3";

fn s(v: &str) -> String {
    v.into()
}

fn series(name: &str, values: &[f64]) -> Series {
    Series { name: s(name), values: values.to_vec() }
}

fn labels(l: &[&str]) -> Vec<String> {
    l.iter().map(|x| s(x)).collect()
}

fn chart(kind: ChartKind, title: &str, l: &[&str], series: Vec<Series>) -> Block {
    Block::Chart(ChartBlock {
        chart: Chart { kind, title: Some(s(title)), labels: labels(l), series, candles: vec![], moving_averages: vec![], up_is_red: true },
    })
}

/// 构造演示画布;`now` 为 Unix 秒。
pub fn demo_document(project_path: &str, now: i64) -> CanvasDocument {
    let candles = [(10., 12., 9.5, 11.5), (11.5, 13., 11., 12.8), (12.8, 13.2, 11.9, 12.1), (12.1, 12.6, 11.2, 11.4), (11.4, 13.5, 11.3, 13.2), (13.2, 14.1, 12.9, 13.9)];
    let blocks = vec![
        Block::Heading { text: s("Atlas 第三季度质量周报"), level: 1 },
        Block::Markdown { text: s("本季度重点是**路由性能**与**指标覆盖**。下方数据由 Agent 汇总,可在源码视图查看 JSON。") },
        Block::Stats {
            items: vec![
                Stat { label: s("合并 PR"), value: s("42"), delta: Some(s("+8 较上季")), tone: Tone::Success },
                Stat { label: s("平均响应"), value: s("118 ms"), delta: Some(s("-23 ms")), tone: Tone::Success },
                Stat { label: s("未关闭缺陷"), value: s("7"), delta: Some(s("+2")), tone: Tone::Warning },
                Stat { label: s("测试覆盖率"), value: s("86%"), delta: None, tone: Tone::Info },
            ],
        },
        chart(ChartKind::Bar, "每月合并 PR", &["7 月", "8 月", "9 月"], vec![series("功能", &[9., 14., 11.]), series("修复", &[6., 5., 9.])]),
        chart(ChartKind::Line, "响应时间(ms)", &["第 1 周", "第 2 周", "第 3 周", "第 4 周"], vec![series("P50", &[141., 130., 124., 118.]), series("P95", &[320., 290., 260., 244.])]),
        chart(ChartKind::Area, "活跃用户(千)", &["周一", "周二", "周三", "周四", "周五"], vec![series("DAU", &[12., 15., 14., 18., 21.])]),
        chart(ChartKind::Pie, "缺陷来源", &["路由", "缓存", "解析", "其他"], vec![series("数量", &[9., 5., 3., 2.])]),
        Block::Chart(ChartBlock {
            chart: Chart {
                kind: ChartKind::Candle,
                title: Some(s("日 K 线")),
                labels: labels(&["D1", "D2", "D3", "D4", "D5", "D6"]),
                series: vec![],
                candles: candles.iter().map(|&(open, high, low, close)| Candle { open, high, low, close }).collect(),
                moving_averages: vec![3],
                up_is_red: true,
            },
        }),
        Block::Table {
            title: Some(s("模块健康度")),
            columns: labels(&["模块", "负责人", "覆盖率", "状态"]),
            rows: [["AtlasCore", "ken", "92", "稳定"], ["Router", "mei", "88", "稳定"], ["Metrics", "ken", "74", "进行中"], ["Cache", "lin", "81", "评审中"], ["CLI", "mei", "69", "进行中"]]
                .iter()
                .map(|r| r.iter().map(|c| s(c)).collect())
                .collect(),
        },
        Block::Callout { tone: Tone::Warning, title: Some(s("风险")), text: s("`Metrics` 的覆盖率低于 80% 的目标,发布前需要补齐。") },
        Block::Section {
            title: s("附录:实现细节"),
            collapsed: true,
            blocks: vec![
                Block::Code { language: Some(s("rust")), text: s("router.register(\"/metrics\", |req| {\n    metrics().count(req.path());\n});") },
                Block::Mermaid { text: s("graph TD\n  A[请求] --> B{路由匹配}\n  B -->|命中| C[处理器]\n  B -->|未命中| D[404]") },
            ],
        },
        Block::Divider,
        Block::Image { source: format!("{project_path}/assets/logo.png"), caption: Some(s("Atlas 标志")) },
    ];
    CanvasDocument { id: DEMO_ID.into(), title: s("Atlas 第三季度质量周报"), blocks, project_path: project_path.into(), created_at: now - 3600, updated_at: now, revision: 2 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 演示画布通过校验并覆盖全部块类型() {
        let d = demo_document("/w", 100);
        d.validate().unwrap();
        let mut kinds = std::collections::BTreeSet::new();
        fn walk(b: &[Block], k: &mut std::collections::BTreeSet<&'static str>) {
            for x in b {
                k.insert(x.type_name());
                if let Block::Section { blocks, .. } = x {
                    walk(blocks, k);
                }
            }
        }
        walk(&d.blocks, &mut kinds);
        assert_eq!(kinds.len(), 11, "{kinds:?}");
    }
}
