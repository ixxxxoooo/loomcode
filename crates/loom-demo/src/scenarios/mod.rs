//! 演示场景注册表。

mod background;
mod errors;
mod interact;
mod perf;
mod styles;
mod subagents;
mod tools;

use crate::script::Script;

/// 场景的运行方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// 自动跑完
    Auto,
    /// 需要用户点击(权限、提问、计划)
    Interactive,
    /// 停住不动,需要取消
    Frozen,
    /// 以非正常方式收尾
    Abnormal,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Auto => "自动",
            Kind::Interactive => "交互(需点击)",
            Kind::Frozen => "冻结(需取消)",
            Kind::Abnormal => "非正常收尾",
        }
    }
}

/// 一个演示场景。
#[derive(Clone)]
pub struct Scenario {
    pub id: &'static str,
    pub title: &'static str,
    pub group: &'static str,
    pub kind: Kind,
    /// 通过菜单触发时显示的用户提示词
    pub prompt: &'static str,
    pub build: fn() -> Script,
}

/// 全部场景(按分组顺序)。
pub fn all() -> Vec<Scenario> {
    let mut v = Vec::new();
    v.extend(tools::scenarios());
    v.extend(interact::scenarios());
    v.extend(styles::scenarios());
    v.extend(subagents::scenarios());
    v.extend(background::scenarios());
    v.extend(errors::scenarios());
    v.extend(perf::scenarios());
    v
}

/// 按 id 查找。
pub fn find(id: &str) -> Option<Scenario> {
    all().into_iter().find(|s| s.id == id)
}

/// 从用户输入解析场景:`/demo-<id>` 或文本里的 `#demo-<id>`。
pub fn resolve(text: &str) -> Option<Scenario> {
    let key = text
        .split_whitespace()
        .find_map(|w| w.strip_prefix("/demo-").or_else(|| w.strip_prefix("#demo-")))?;
    find(key.trim_matches(|c: char| !c.is_alphanumeric() && c != '-'))
}

/// 分组名列表(去重,保持首次出现顺序)。
pub fn groups() -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for s in all() {
        if !out.contains(&s.group) {
            out.push(s.group);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 场景_id_唯一且可解析() {
        let list = all();
        let mut ids: Vec<_> = list.iter().map(|s| s.id).collect();
        let n = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), n, "场景 id 重复");
        assert!(resolve("/demo-showcase-tour 看看").is_some());
        assert!(resolve("请运行 #demo-ask。").is_some());
        assert!(resolve("普通文本").is_none());
    }

    #[test]
    fn 每个场景都能构建出非空脚本() {
        for s in all() {
            let script = (s.build)();
            assert!(!script.steps.is_empty(), "{} 脚本为空", s.id);
        }
    }
}
