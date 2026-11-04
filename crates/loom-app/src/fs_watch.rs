//! 项目目录的文件系统监视(对标 VS Code 的递归文件监视器)。
//!
//! 分层:① 系统接口(macOS 的 FSEvents,经 `notify`)→ ② 事件整理(本文件:过滤、防抖、限频、归类成 [`Batch`])
//! → ③ 订阅者各取所需(文件树按目录增量重读、编辑器重载已打开文件、源代码管理刷新状态)。
//!
//! - **共享**:同一个项目根只建一个监视器(Agent 右栏、IDE 左栏各自订阅同一份),没有订阅者后自动销毁。
//! - **过滤**:被「代码智能」忽略规则命中的目录(`target`、`node_modules`…)与 `.DS_Store` 里的事件丢弃;
//!   `.git` 里只认 `HEAD`、`index`、`packed-refs`、`refs/**`(提交 / 切分支 / 暂存会改它们),`objects` 等噪音丢弃。
//! - **整理**:75 ms 防抖把同一批改动攒成一个 [`Batch`],批与批之间至少隔 250 ms;FSEvents 报告「丢了事件」时
//!   置 `rescan`,订阅者要整体重读。FSEvents 报的是规范化路径(`/private/var/…`),这里换回项目根的写法。
//! - 改名不做配对:订阅者拿到的是「受影响的目录」,旧名消失、新名出现都落在同一个目录的重读里。
//!
//! @author ixxxxoooo

use loom_config::IntelPrefs;
use notify::{EventKind, RecursiveMode, Watcher};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

/// 防抖:收到第一个事件后等这么久,让同一批改动落完。
const DEBOUNCE: Duration = Duration::from_millis(75);
/// 限频:两批之间的最小间隔(编译、`git checkout` 这类突发改动不会刷屏)。
const THROTTLE: Duration = Duration::from_millis(250);

/// 一批整理好的文件系统改动。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Batch {
    /// 目录列表可能变了的目录(有条目新建 / 删除 / 改名的父目录,以及这些条目本身)
    pub dirs: HashSet<PathBuf>,
    /// 所有受影响的路径(含内容修改),用来对上已打开的文件
    pub paths: HashSet<PathBuf>,
    /// `.git` 里的关键文件变了(提交、切分支、暂存…)
    pub git: bool,
    /// 系统报告丢了事件,订阅者应整体重读
    pub rescan: bool,
}

impl Batch {
    pub fn is_empty(&self) -> bool {
        self.dirs.is_empty() && self.paths.is_empty() && !self.git && !self.rescan
    }
}

/// 原始事件的种类(从 `notify` 的事件种类归并而来)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Create,
    Remove,
    Rename,
    Modify,
    Other,
}

/// 一条原始事件。
#[derive(Debug, Clone)]
pub struct Raw {
    pub path: PathBuf,
    pub kind: Kind,
    /// 系统报告丢了事件(此时没有有效路径)
    pub rescan: bool,
}

/// 路径归类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    Ignore,
    /// `.git` 里的关键文件
    Git,
    Normal,
}

/// 判断一条路径的事件该怎么处理。`root` 是项目根(路径已换成它的写法)。
pub fn classify(root: &Path, path: &Path, intel: &IntelPrefs) -> Class {
    let rel = path.strip_prefix(root).unwrap_or(path);
    let comps: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().to_string()).collect();
    if let Some(i) = comps.iter().position(|c| c == ".git") {
        let after = &comps[i + 1..];
        let key = matches!(after.first().map(String::as_str), Some("HEAD" | "index" | "packed-refs" | "refs"));
        let lock = after.last().is_some_and(|n| n.ends_with(".lock"));
        return if key && !lock { Class::Git } else { Class::Ignore };
    }
    if comps.iter().any(|c| c == ".DS_Store" || intel.ignored(c)) { Class::Ignore } else { Class::Normal }
}

/// 把 FSEvents 的规范化路径换回项目根的写法(`/private/var/p/a` → `/var/p/a`)。
pub fn rebase(path: &Path, root: &Path, canon_root: Option<&Path>) -> PathBuf {
    match canon_root.and_then(|c| path.strip_prefix(c).ok()) {
        Some(rel) => root.join(rel),
        None => path.to_path_buf(),
    }
}

/// 把一批原始事件整理成 [`Batch`]。
pub fn build_batch(root: &Path, canon_root: Option<&Path>, raws: &[Raw], intel: &IntelPrefs) -> Batch {
    let mut b = Batch::default();
    for r in raws {
        if r.rescan {
            b.rescan = true;
            continue;
        }
        let p = rebase(&r.path, root, canon_root);
        match classify(root, &p, intel) {
            Class::Ignore => {}
            Class::Git => b.git = true,
            Class::Normal => {
                if !matches!(r.kind, Kind::Modify) {
                    // 新建 / 删除 / 改名 / 未知:父目录的列表变了;条目本身若是目录,它自己的列表也作废
                    if let Some(parent) = p.parent() {
                        b.dirs.insert(parent.to_path_buf());
                    }
                    b.dirs.insert(p.clone());
                }
                b.paths.insert(p);
            }
        }
    }
    b
}

/// `notify` 事件种类 → [`Kind`]。
fn kind_of(k: &EventKind) -> Option<Kind> {
    use notify::event::ModifyKind;
    match k {
        EventKind::Access(_) => None,
        EventKind::Create(_) => Some(Kind::Create),
        EventKind::Remove(_) => Some(Kind::Remove),
        EventKind::Modify(ModifyKind::Name(_)) => Some(Kind::Rename),
        EventKind::Modify(_) => Some(Kind::Modify),
        _ => Some(Kind::Other),
    }
}

/// 一个项目根的共享监视:订阅者列表 + 监视器本体(丢弃即停止)。
struct RootWatch {
    subs: Arc<Mutex<Vec<async_channel::Sender<Arc<Batch>>>>>,
    _watcher: notify::RecommendedWatcher,
}

fn hub() -> &'static Mutex<HashMap<PathBuf, RootWatch>> {
    static HUB: OnceLock<Mutex<HashMap<PathBuf, RootWatch>>> = OnceLock::new();
    HUB.get_or_init(Default::default)
}

/// 订阅项目根的文件系统改动。同一个根共用一个监视器;所有订阅者的接收端都被丢弃后,监视器随下一批事件销毁。
/// 目录不存在或系统拒绝监视时返回 `None`。
pub fn subscribe(root: &Path, intel: IntelPrefs) -> Option<async_channel::Receiver<Arc<Batch>>> {
    let (tx, rx) = async_channel::unbounded();
    let mut map = hub().lock().ok()?;
    if let Some(w) = map.get(root) {
        w.subs.lock().ok()?.push(tx);
        return Some(rx);
    }
    let pending: Arc<Mutex<Vec<Raw>>> = Arc::default();
    let (sig_tx, sig_rx) = std::sync::mpsc::sync_channel::<()>(1);
    let cb_pending = pending.clone();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let Ok(ev) = res else { return };
        let mut raws = Vec::new();
        if ev.need_rescan() {
            raws.push(Raw { path: PathBuf::new(), kind: Kind::Other, rescan: true });
        }
        if let Some(kind) = kind_of(&ev.kind) {
            raws.extend(ev.paths.iter().map(|p| Raw { path: p.clone(), kind, rescan: false }));
        }
        if raws.is_empty() {
            return;
        }
        if let Ok(mut p) = cb_pending.lock() {
            p.extend(raws);
        }
        let _ = sig_tx.try_send(());
    })
    .map_err(|e| loom_diag::warn!("文件监视:创建失败:{e}"))
    .ok()?;
    watcher.watch(root, RecursiveMode::Recursive).map_err(|e| loom_diag::warn!("文件监视:{} 监视失败:{e}", root.display())).ok()?;
    let subs = Arc::new(Mutex::new(vec![tx]));
    let (thread_subs, root_key) = (subs.clone(), root.to_path_buf());
    let canon = std::fs::canonicalize(root).ok();
    let spawned = std::thread::Builder::new().name("loom-fs-watch".into()).spawn(move || {
        // 监视器被丢弃后回调(连同 sig_tx)一起释放,recv 报错,线程退出
        while sig_rx.recv().is_ok() {
            std::thread::sleep(DEBOUNCE);
            let raws = pending.lock().map(|mut p| std::mem::take(&mut *p)).unwrap_or_default();
            while sig_rx.try_recv().is_ok() {}
            let batch = Arc::new(build_batch(&root_key, canon.as_deref(), &raws, &intel));
            if !batch.is_empty() {
                let empty = thread_subs.lock().map(|mut s| {
                    s.retain(|t| t.try_send(batch.clone()).is_ok());
                    s.is_empty()
                });
                if empty.unwrap_or(true) {
                    // 没有订阅者了:先拿全局锁再复查(与 subscribe 的加锁顺序一致),确认后销毁监视器
                    if let Ok(mut map) = hub().lock() {
                        if thread_subs.lock().map(|s| s.is_empty()).unwrap_or(true) {
                            map.remove(&root_key);
                            return;
                        }
                    }
                }
            }
            std::thread::sleep(THROTTLE);
        }
    });
    if spawned.is_err() {
        return None;
    }
    map.insert(root.to_path_buf(), RootWatch { subs, _watcher: watcher });
    Some(rx)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(path: &str, kind: Kind) -> Raw {
        Raw { path: path.into(), kind, rescan: false }
    }

    #[::core::prelude::v1::test]
    fn 归类_忽略目录与系统文件() {
        let (intel, root) = (IntelPrefs::default(), Path::new("/p"));
        assert_eq!(classify(root, Path::new("/p/src/main.rs"), &intel), Class::Normal);
        assert_eq!(classify(root, Path::new("/p/target/debug/x"), &intel), Class::Ignore);
        assert_eq!(classify(root, Path::new("/p/web/node_modules/a.js"), &intel), Class::Ignore);
        assert_eq!(classify(root, Path::new("/p/src/.DS_Store"), &intel), Class::Ignore);
    }

    #[::core::prelude::v1::test]
    fn 归类_git_只认关键文件() {
        let (intel, root) = (IntelPrefs::default(), Path::new("/p"));
        for ok in ["/p/.git/HEAD", "/p/.git/index", "/p/.git/packed-refs", "/p/.git/refs/heads/main", "/p/sub/.git/index"] {
            assert_eq!(classify(root, Path::new(ok), &intel), Class::Git, "{ok}");
        }
        for no in ["/p/.git/objects/ab/cd", "/p/.git/index.lock", "/p/.git/refs/heads/x.lock", "/p/.git/logs/HEAD", "/p/.git"] {
            assert_eq!(classify(root, Path::new(no), &intel), Class::Ignore, "{no}");
        }
    }

    #[::core::prelude::v1::test]
    fn 规范化路径换回项目根写法() {
        let (root, canon) = (Path::new("/var/p"), Path::new("/private/var/p"));
        assert_eq!(rebase(Path::new("/private/var/p/a/b.rs"), root, Some(canon)), PathBuf::from("/var/p/a/b.rs"));
        assert_eq!(rebase(Path::new("/x/y"), root, Some(canon)), PathBuf::from("/x/y"));
        assert_eq!(rebase(Path::new("/var/p/a"), root, None), PathBuf::from("/var/p/a"));
    }

    #[::core::prelude::v1::test]
    fn 整理批次_结构变化记目录_内容修改只记路径() {
        let intel = IntelPrefs::default();
        let root = Path::new("/p");
        let raws = [
            raw("/p/src/new.rs", Kind::Create),
            raw("/p/src/a.rs", Kind::Modify),
            raw("/p/docs/old.md", Kind::Rename),
            raw("/p/target/x", Kind::Create),
            raw("/p/.git/HEAD", Kind::Modify),
            raw("/p/.git/objects/zz", Kind::Create),
        ];
        let b = build_batch(root, None, &raws, &intel);
        assert!(b.git && !b.rescan);
        assert!(b.dirs.contains(Path::new("/p/src")) && b.dirs.contains(Path::new("/p/docs")));
        assert!(!b.dirs.iter().any(|d| d.starts_with("/p/target")));
        assert!(b.paths.contains(Path::new("/p/src/a.rs")) && !b.dirs.contains(Path::new("/p/src/a.rs")));
    }

    #[::core::prelude::v1::test]
    fn 丢事件置_rescan() {
        let b = build_batch(Path::new("/p"), None, &[Raw { path: PathBuf::new(), kind: Kind::Other, rescan: true }], &IntelPrefs::default());
        assert!(b.rescan && !b.is_empty());
    }

    fn wait(rx: &async_channel::Receiver<Arc<Batch>>, ms: u64) -> Option<Arc<Batch>> {
        let end = std::time::Instant::now() + Duration::from_millis(ms);
        while std::time::Instant::now() < end {
            if let Ok(b) = rx.try_recv() {
                return Some(b);
            }
            std::thread::sleep(Duration::from_millis(30));
        }
        None
    }

    #[::core::prelude::v1::test]
    fn 真实文件系统_新建文件与_git_关键文件触发_噪音不触发() {
        let dir = std::env::temp_dir().join(format!("loom-fswatch-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("target")).unwrap();
        std::fs::create_dir_all(dir.join(".git/objects")).unwrap();
        let rx = subscribe(&dir, IntelPrefs::default()).expect("监视应当能启动");
        // 同一个根再订阅一次,共用监视器
        let rx2 = subscribe(&dir, IntelPrefs::default()).expect("第二个订阅者");
        std::thread::sleep(Duration::from_millis(300));
        std::fs::write(dir.join("target/junk.o"), b"x").unwrap();
        std::fs::write(dir.join(".git/objects/obj"), b"x").unwrap();
        assert!(wait(&rx, 1200).is_none(), "忽略目录与 .git/objects 里的改动不该触发");
        std::fs::write(dir.join("new.txt"), b"hi").unwrap();
        let b = wait(&rx, 3000).expect("新建文件应当触发");
        assert!(b.paths.contains(&dir.join("new.txt")), "路径应是项目根的写法:{:?}", b.paths);
        assert!(b.dirs.contains(&dir), "父目录(项目根)应在 dirs 里:{:?}", b.dirs);
        assert!(wait(&rx2, 1000).is_some(), "第二个订阅者也应收到");
        std::thread::sleep(Duration::from_millis(400));
        while rx.try_recv().is_ok() {}
        std::fs::write(dir.join(".git/HEAD"), b"ref: refs/heads/main").unwrap();
        let b = wait(&rx, 3000).expect(".git/HEAD 变化应当触发");
        assert!(b.git);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
