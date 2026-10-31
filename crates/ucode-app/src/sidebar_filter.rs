//! 侧栏过滤:按关键字筛选会话与项目。纯函数,不依赖界面。
//!
//! @author ixxxxoooo

use ucode_core::{Project, SessionMeta};

/// 忽略大小写、忽略首尾空白的包含匹配;关键字为空时匹配一切。
///
/// 参数:`query` 关键字;`text` 被匹配的文本。
pub fn matches(query: &str, text: &str) -> bool {
    let q = query.trim().to_lowercase();
    q.is_empty() || text.to_lowercase().contains(&q)
}

/// 一个项目在过滤后要显示的会话。
///
/// 规则:项目名命中时显示它的全部会话,否则只显示标题命中的会话;已归档会话按
/// `show_archived` 取舍,但当前打开的会话(`active`)始终保留。
///
/// 返回:要显示的会话;项目整体不该出现时返回 `None`(关键字为空时项目永远出现)。
pub fn visible_sessions<'a>(query: &str, project: &'a Project, show_archived: bool, active: Option<&str>) -> Option<Vec<&'a SessionMeta>> {
    let project_hit = matches(query, &project.name);
    let sessions: Vec<&SessionMeta> = project
        .sessions
        .iter()
        .filter(|s| show_archived || !s.archived || active == Some(s.id.as_str()))
        .filter(|s| project_hit || matches(query, &s.title))
        .collect();
    (query.trim().is_empty() || project_hit || !sessions.is_empty()).then_some(sessions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ucode_core::SessionBadge;

    fn meta(id: &str, title: &str, archived: bool) -> SessionMeta {
        SessionMeta { id: id.into(), title: title.into(), updated: String::new(), pinned: false, archived, badge: SessionBadge::Idle }
    }

    fn project() -> Project {
        Project {
            name: "Demo".into(),
            path: "/demo".into(),
            branch: None,
            sessions: vec![meta("1", "修复登录 Bug", false), meta("2", "写文档", false), meta("3", "旧任务", true)],
            expanded: true,
        }
    }

    #[test]
    fn 匹配忽略大小写和空白() {
        assert!(matches("  BUG ", "修复登录 bug"));
        assert!(matches("", "任何"));
        assert!(!matches("xyz", "修复登录"));
    }

    #[test]
    fn 空关键字显示未归档会话() {
        let p = project();
        let ids: Vec<_> = visible_sessions("", &p, false, None).unwrap().iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["1", "2"]);
    }

    #[test]
    fn 按标题过滤且无命中隐藏项目() {
        let p = project();
        let hit = visible_sessions("文档", &p, false, None).unwrap();
        assert_eq!(hit.len(), 1);
        assert!(visible_sessions("不存在", &p, false, None).is_none());
    }

    #[test]
    fn 项目名命中显示全部会话() {
        let p = project();
        assert_eq!(visible_sessions("demo", &p, false, None).unwrap().len(), 2);
    }

    #[test]
    fn 归档会话按开关显示_当前会话始终保留() {
        let p = project();
        assert_eq!(visible_sessions("", &p, true, None).unwrap().len(), 3);
        assert_eq!(visible_sessions("", &p, false, Some("3")).unwrap().len(), 3);
    }
}
