//! 后台任务按类别分组:输入框上方的任务坞与右侧「任务」标签共用。
//!
//! 运行中的任务按类型分组(子代理、后台命令、监听、工作流、其他),已结束的统一放进「已结束」。
//!
//! @author ixxxxoooo

use crate::task::{BackgroundTask, TaskKind, TaskStatus};

/// 任务分组。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskGroup {
    Agent,
    Shell,
    Monitor,
    Workflow,
    Other,
    /// 已结束(完成、失败、停止)的任务
    Finished,
}

impl TaskGroup {
    /// 分组标题。
    pub fn label(self) -> &'static str {
        match self {
            Self::Agent => "子代理",
            Self::Shell => "后台命令",
            Self::Monitor => "监听",
            Self::Workflow => "工作流",
            Self::Other => "其他任务",
            Self::Finished => "已结束",
        }
    }

    /// 对应的任务类型;「已结束」没有。
    pub fn kind(self) -> Option<TaskKind> {
        match self {
            Self::Agent => Some(TaskKind::Agent),
            Self::Shell => Some(TaskKind::Shell),
            Self::Monitor => Some(TaskKind::Monitor),
            Self::Workflow => Some(TaskKind::Workflow),
            Self::Other => Some(TaskKind::Other),
            Self::Finished => None,
        }
    }
}

/// 一个分组及其任务。
#[derive(Debug, Clone, PartialEq)]
pub struct TaskSection<'a> {
    pub group: TaskGroup,
    pub tasks: Vec<&'a BackgroundTask>,
}

/// 分组显示顺序(「已结束」永远在最后)。
const ORDER: [TaskGroup; 6] = [TaskGroup::Agent, TaskGroup::Shell, TaskGroup::Monitor, TaskGroup::Workflow, TaskGroup::Other, TaskGroup::Finished];

/// 把任务分组。
///
/// 参数:`tasks` 全部任务(环境任务、子 Agent 私有任务会被过滤)。
/// 返回:非空的分组,按固定顺序;组内运行中的在前、新开始的在前(已结束组按结束时间倒序)。
pub fn group_tasks(tasks: &[BackgroundTask]) -> Vec<TaskSection<'_>> {
    let mut out = Vec::new();
    for group in ORDER {
        let mut list: Vec<&BackgroundTask> = tasks
            .iter()
            .filter(|t| t.visible())
            .filter(|t| match group.kind() {
                Some(k) => t.status == TaskStatus::Running && t.kind == k,
                None => t.status.is_finished(),
            })
            .collect();
        if list.is_empty() {
            continue;
        }
        list.sort_by_key(|t| std::cmp::Reverse(t.ended_ms.unwrap_or(t.started_ms)));
        out.push(TaskSection { group, tasks: list });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(id: &str, kind: TaskKind, status: TaskStatus, started: u64) -> BackgroundTask {
        let mut t = BackgroundTask::new(id, kind, id);
        t.status = status;
        t.started_ms = started;
        t
    }

    #[test]
    fn 按类别分组且已结束放最后() {
        let tasks = vec![
            task("done", TaskKind::Shell, TaskStatus::Completed, 1),
            task("sh", TaskKind::Shell, TaskStatus::Running, 2),
            task("ag", TaskKind::Agent, TaskStatus::Running, 3),
            task("mo", TaskKind::Monitor, TaskStatus::Running, 4),
            task("wf", TaskKind::Workflow, TaskStatus::Running, 5),
            task("ot", TaskKind::Other, TaskStatus::Running, 6),
            task("bad", TaskKind::Agent, TaskStatus::Failed, 7),
        ];
        let groups: Vec<TaskGroup> = group_tasks(&tasks).iter().map(|s| s.group).collect();
        assert_eq!(groups, vec![TaskGroup::Agent, TaskGroup::Shell, TaskGroup::Monitor, TaskGroup::Workflow, TaskGroup::Other, TaskGroup::Finished]);
    }

    #[test]
    fn 空分组不出现且过滤不可见任务() {
        let mut ambient = task("amb", TaskKind::Shell, TaskStatus::Running, 1);
        ambient.ambient = true;
        let mut private = task("own", TaskKind::Agent, TaskStatus::Running, 2);
        private.owned_by_subagent = true;
        let tasks = vec![ambient, private, task("sh", TaskKind::Shell, TaskStatus::Running, 3)];
        let groups = group_tasks(&tasks);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].group, TaskGroup::Shell);
        assert_eq!(groups[0].tasks.len(), 1);
        assert!(group_tasks(&[]).is_empty());
    }

    #[test]
    fn 组内新的在前_已结束按结束时间倒序() {
        let mut a = task("a", TaskKind::Shell, TaskStatus::Completed, 1);
        a.ended_ms = Some(100);
        let mut b = task("b", TaskKind::Shell, TaskStatus::Stopped, 50);
        b.ended_ms = Some(60);
        let tasks = vec![task("old", TaskKind::Shell, TaskStatus::Running, 10), task("new", TaskKind::Shell, TaskStatus::Running, 20), a, b];
        let g = group_tasks(&tasks);
        let ids: Vec<&str> = g[0].tasks.iter().map(|t| t.task_id.as_str()).collect();
        assert_eq!(ids, vec!["new", "old"]);
        let fin: Vec<&str> = g[1].tasks.iter().map(|t| t.task_id.as_str()).collect();
        assert_eq!(fin, vec!["a", "b"]);
    }

    #[test]
    fn 前台运行中的子代理也在子代理组() {
        let mut fg = task("fg", TaskKind::Agent, TaskStatus::Running, 1);
        fg.backgrounded = false;
        let tasks = vec![fg];
        assert_eq!(group_tasks(&tasks)[0].group, TaskGroup::Agent);
    }
}
