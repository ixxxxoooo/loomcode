//! 扩展扫描、新建、删除与记忆文件清单的测试。
//!
//! @author ixxxxoooo

use super::*;
use std::path::{Path, PathBuf};

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("loom-items-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn put(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

#[test]
fn 解析_frontmatter() {
    let (fm, body) = split_frontmatter("---\nname: a\ndescription: \"说明: 含冒号\"\ntools:\n  - Read\n---\n\n正文");
    assert_eq!(fm, vec![("name".to_string(), "a".to_string()), ("description".into(), "说明: 含冒号".into()), ("tools".into(), "".into())]);
    assert_eq!(body, "正文");
    assert_eq!(split_frontmatter("没有头").1, "没有头");
}

#[test]
fn 扫描三类扩展() {
    let home = temp_dir("scan");
    let proj = home.join("proj");
    put(&home.join(".claude/commands/review.md"), "---\ndescription: 审查\n---\n内容");
    put(&home.join(".claude/commands/git/push.md"), "# 推送代码\n");
    put(&proj.join(".claude/commands/review.md"), "---\ndescription: 项目版审查\n---\n");
    put(&home.join(".claude/agents/x.md"), "---\nname: reviewer\ndescription: 审查员\n---\n");
    put(&home.join(".claude/agents/sub/ignored.md"), "子目录里的不算");
    put(&home.join(".claude/skills/ios/SKILL.md"), "---\nname: ios-development\ndescription: iOS\n---\n");
    put(&home.join(".claude/skills/hidden/SKILL.md"), "---\ndescription: 隐藏\nuser-invocable: false\n---\n");
    put(&home.join(".claude/skills/_shared/notes.md"), "没有 SKILL.md");

    let cmds = list_items(&home, Some(&proj), ItemKind::Command);
    let names: Vec<_> = cmds.iter().map(|i| (i.name.as_str(), i.scope)).collect();
    assert_eq!(names, vec![("git:push", Scope::User), ("review", Scope::User), ("review", Scope::Project)]);
    assert_eq!(cmds[0].description, "推送代码");

    let agents = list_items(&home, None, ItemKind::Agent);
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0].name, "reviewer");

    let skills = list_items(&home, None, ItemKind::Skill);
    assert_eq!(skills.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), vec!["hidden", "ios"]);

    let slash = slash_suggestions(&home, Some(&proj));
    assert_eq!(slash.iter().map(|s| s.0.as_str()).collect::<Vec<_>>(), vec!["/git:push", "/ios", "/review"]);
    assert_eq!(slash.iter().find(|s| s.0 == "/review").unwrap().1, "项目版审查");
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn 新建与删除() {
    let home = temp_dir("create");
    let p = create_item(&home, None, Scope::User, ItemKind::Skill, "my-skill", "做某事").unwrap();
    assert!(p.ends_with(".claude/skills/my-skill/SKILL.md"));
    assert!(std::fs::read_to_string(&p).unwrap().contains("name: my-skill"));
    assert!(create_item(&home, None, Scope::User, ItemKind::Skill, "my-skill", "重复").is_err());
    assert!(create_item(&home, None, Scope::User, ItemKind::Command, "Bad Name", "x").is_err());
    assert!(create_item(&home, None, Scope::User, ItemKind::Command, "ok", " ").is_err());
    assert!(create_item(&home, None, Scope::Project, ItemKind::Agent, "a", "x").is_err());

    let item = list_items(&home, None, ItemKind::Skill).remove(0);
    let trash = home.join(".Trash");
    trash_item(&item, &trash).unwrap();
    assert!(trash.join("my-skill/SKILL.md").is_file());
    assert!(list_items(&home, None, ItemKind::Skill).is_empty());
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn 记忆文件按加载顺序列出() {
    let home = temp_dir("memory");
    let proj = home.join("proj");
    put(&home.join(".claude/rules/style.md"), "x");
    put(&proj.join("CLAUDE.md"), "x");
    let files = memory_files(&home, Some(&proj));
    let groups: Vec<_> = files.iter().map(|f| (f.group, f.exists)).collect();
    assert_eq!(groups, vec![("用户", false), ("用户", true), ("项目", true), ("本地", false)]);
    assert_eq!(memory_files(&home, None).len(), 2);
    let _ = std::fs::remove_dir_all(home);
}
