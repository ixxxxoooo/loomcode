//! Claude Code 的 Markdown 扩展:斜杠命令(`.claude/commands`)、子 Agent(`.claude/agents`)、
//! Skills(`.claude/skills/<名>/SKILL.md`),以及记忆文件(CLAUDE.md、规则)。
//!
//! 只管用户层(`~/.claude`)与项目层(`<项目>/.claude`);插件带来的条目不在这里。
//!
//! @author ixxxxoooo

mod frontmatter;
mod memory;
mod store;

#[cfg(test)]
mod tests;

pub use frontmatter::split_frontmatter;
pub use memory::{MemoryFile, memory_files};
pub use store::{create_item, list_items, slash_suggestions, trash_item};

use crate::claude::Scope;
use std::path::{Path, PathBuf};

/// 扩展种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ItemKind {
    Command,
    Agent,
    Skill,
}

impl ItemKind {
    pub const ALL: [ItemKind; 3] = [ItemKind::Command, ItemKind::Agent, ItemKind::Skill];

    /// `.claude` 下的目录名。
    pub fn dir(self) -> &'static str {
        match self {
            ItemKind::Command => "commands",
            ItemKind::Agent => "agents",
            ItemKind::Skill => "skills",
        }
    }

    /// 界面名称。
    pub fn label(self) -> &'static str {
        match self {
            ItemKind::Command => "斜杠命令",
            ItemKind::Agent => "子 Agent",
            ItemKind::Skill => "Skills",
        }
    }

    /// 某一层的根目录。
    ///
    /// 参数:`home` 家目录;`project` 当前项目;`scope` 只支持用户层 / 项目层。返回:目录,项目层但没有项目时为 `None`。
    pub fn root(self, home: &Path, project: Option<&Path>, scope: Scope) -> Option<PathBuf> {
        let base = match scope {
            Scope::User => home.to_path_buf(),
            Scope::Project | Scope::Local => project?.to_path_buf(),
        };
        Some(base.join(".claude").join(self.dir()))
    }
}

/// 一条扩展。
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub kind: ItemKind,
    /// 调用名:命令为相对路径(子目录用 `:` 连接),Agent 取 frontmatter `name`,Skill 取目录名
    pub name: String,
    pub description: String,
    pub scope: Scope,
    /// 要编辑的 Markdown 文件
    pub path: PathBuf,
    /// Skill 为 `user-invocable: false` 时不能用斜杠手动调用
    pub user_invocable: bool,
}
