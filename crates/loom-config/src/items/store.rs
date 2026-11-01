//! 扫描、新建、删除(移到废纸篓)命令 / 子 Agent / Skills。
//!
//! @author ixxxxoooo

use super::frontmatter::{get, split_frontmatter};
use super::{Item, ItemKind};
use crate::claude::Scope;
use std::io::{Error, Result};
use std::path::{Path, PathBuf};

/// 递归收集目录下的 `.md` 文件(命令支持子目录命名空间)。
fn md_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            md_files(&p, out);
        } else if p.extension().is_some_and(|e| e == "md") {
            out.push(p);
        }
    }
}

/// 读取一个文件的 frontmatter 生成条目。
fn make_item(kind: ItemKind, scope: Scope, path: PathBuf, default_name: String) -> Item {
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let (fm, body) = split_frontmatter(&text);
    let name = if kind == ItemKind::Agent { get(&fm, "name").filter(|n| !n.is_empty()).map(str::to_string).unwrap_or(default_name) } else { default_name };
    // 没写 description 时退回正文第一行
    let description = get(&fm, "description").map(str::to_string).unwrap_or_else(|| body.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim_start_matches('#').trim().to_string());
    let user_invocable = get(&fm, "user-invocable") != Some("false");
    Item { kind, name, description, scope, path, user_invocable }
}

/// 列出某种扩展在用户层与项目层的全部条目(用户层在前,各自按名称排序)。
///
/// 参数:`home` 家目录;`project` 当前项目;`kind` 种类。返回:条目列表。
pub fn list_items(home: &Path, project: Option<&Path>, kind: ItemKind) -> Vec<Item> {
    let mut out = Vec::new();
    for scope in [Scope::User, Scope::Project] {
        let Some(root) = kind.root(home, project, scope) else { continue };
        let mut items: Vec<Item> = match kind {
            ItemKind::Command | ItemKind::Agent => {
                let mut files = Vec::new();
                md_files(&root, &mut files);
                files
                    .into_iter()
                    .filter(|p| kind == ItemKind::Command || p.parent() == Some(root.as_path()))
                    .map(|p| {
                        let rel = p.strip_prefix(&root).unwrap_or(&p).with_extension("");
                        let name = rel.to_string_lossy().replace(std::path::MAIN_SEPARATOR, ":");
                        make_item(kind, scope, p, name)
                    })
                    .collect()
            }
            ItemKind::Skill => std::fs::read_dir(&root)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .filter(|d| d.join("SKILL.md").is_file())
                .map(|d| {
                    let name = d.file_name().unwrap_or_default().to_string_lossy().to_string();
                    make_item(kind, scope, d.join("SKILL.md"), name)
                })
                .collect(),
        };
        items.sort_by(|a, b| a.name.cmp(&b.name));
        out.extend(items);
    }
    out
}

/// 校验新建名称:小写字母、数字、`-`、`_`。
fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_') {
        return Err(Error::other("名称只能包含小写字母、数字、- 和 _"));
    }
    Ok(())
}

/// 新建文件的模板内容。
fn template(kind: ItemKind, name: &str, description: &str) -> String {
    let desc = description.replace('\n', " ");
    match kind {
        ItemKind::Command => format!("---\ndescription: {desc}\nargument-hint: [参数]\n---\n\n在这里写命令的提示词。可以用 $ARGUMENTS 引用调用时传入的参数。\n"),
        ItemKind::Agent => format!("---\nname: {name}\ndescription: {desc}\ntools: Read, Grep, Glob\nmodel: inherit\n---\n\n你是一个……(在这里写子 Agent 的系统提示词)\n"),
        ItemKind::Skill => format!("---\nname: {name}\ndescription: {desc}\n---\n\n# {name}\n\n在这里写这个 Skill 的说明与步骤。\n"),
    }
}

/// 新建一条扩展。
///
/// 参数:`home` / `project` / `scope` 目标层;`kind` 种类;`name` 名称;`description` 描述。返回:新建的 Markdown 路径;已存在时报错。
pub fn create_item(home: &Path, project: Option<&Path>, scope: Scope, kind: ItemKind, name: &str, description: &str) -> Result<PathBuf> {
    validate_name(name)?;
    if description.trim().is_empty() {
        return Err(Error::other("请填写描述,Claude 据此判断何时使用"));
    }
    let root = kind.root(home, project, scope).ok_or_else(|| Error::other("先打开一个项目"))?;
    let path = match kind {
        ItemKind::Skill => root.join(name).join("SKILL.md"),
        _ => root.join(format!("{name}.md")),
    };
    if path.exists() {
        return Err(Error::other(format!("{} 已存在", path.display())));
    }
    std::fs::create_dir_all(path.parent().unwrap_or(&root))?;
    std::fs::write(&path, template(kind, name, description.trim()))?;
    Ok(path)
}

/// 把条目移到废纸篓(Skill 整个目录)。
///
/// 参数:`item` 条目;`trash` 废纸篓目录(通常为 `~/.Trash`)。返回:IO 结果。
pub fn trash_item(item: &Item, trash: &Path) -> Result<()> {
    let src = if item.kind == ItemKind::Skill { item.path.parent().map(Path::to_path_buf).unwrap_or_else(|| item.path.clone()) } else { item.path.clone() };
    std::fs::create_dir_all(trash)?;
    let base = src.file_name().unwrap_or_default().to_string_lossy().to_string();
    let mut dst = trash.join(&base);
    let mut n = 1;
    while dst.exists() {
        dst = trash.join(format!("{base} {n}"));
        n += 1;
    }
    std::fs::rename(&src, &dst)
}

/// 输入框 `/` 补全用的自定义命令:斜杠命令 + 可手动调用的 Skills;同名时项目层优先。
///
/// 参数:`home` 家目录;`project` 当前项目。返回:(`/名称`, 描述) 列表。
pub fn slash_suggestions(home: &Path, project: Option<&Path>) -> Vec<(String, String)> {
    let mut items: Vec<Item> = list_items(home, project, ItemKind::Command);
    items.extend(list_items(home, project, ItemKind::Skill).into_iter().filter(|i| i.user_invocable));
    items.sort_by_key(|i| i.scope != Scope::Project);
    let mut out: Vec<(String, String)> = Vec::new();
    for i in items {
        let cmd = format!("/{}", i.name);
        if !out.iter().any(|(c, _)| *c == cmd) {
            out.push((cmd, i.description));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}
