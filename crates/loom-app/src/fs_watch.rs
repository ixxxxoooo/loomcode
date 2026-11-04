//! 项目目录的文件系统监视(对标 VS Code 的递归文件监视器):外部改动(Claude、终端、`git checkout`…)
//! 让文件树自动刷新,不用再手动点刷新。
//!
//! macOS 上 `notify` 走 FSEvents,递归监视一个目录只占一个系统流,开销很小。被「代码智能」忽略规则命中的目录
//! (`.git`、`target`、`node_modules` 等)里的事件直接丢掉,避免编译产物刷屏;`.DS_Store` 同理。
//!
//! @author ixxxxoooo

use loom_config::IntelPrefs;
use notify::{EventKind, RecursiveMode, Watcher};
use std::path::Path;

/// 持有监视器;丢弃即停止监视(同时关闭事件通道,消费任务随之退出)。
pub struct FsWatch {
    _watcher: notify::RecommendedWatcher,
}

impl FsWatch {
    /// 开始监视 `root`。返回监视器与「有相关改动」的信号通道(容量 1,多次改动合并成一次信号);
    /// 目录不存在或系统拒绝监视时返回 `None`。
    pub fn start(root: &Path, intel: IntelPrefs) -> Option<(Self, async_channel::Receiver<()>)> {
        let (tx, rx) = async_channel::bounded(1);
        let base = root.to_path_buf();
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            let Ok(ev) = res else { return };
            if matches!(ev.kind, EventKind::Access(_)) {
                return;
            }
            if ev.paths.iter().any(|p| relevant(&base, p, &intel)) {
                let _ = tx.try_send(());
            }
        })
        .map_err(|e| loom_diag::warn!("文件监视:创建失败:{e}"))
        .ok()?;
        watcher.watch(root, RecursiveMode::Recursive).map_err(|e| loom_diag::warn!("文件监视:{} 监视失败:{e}", root.display())).ok()?;
        Some((Self { _watcher: watcher }, rx))
    }
}

/// 这条路径上的事件是否值得刷新文件树:路径里(相对项目根的部分)没有被忽略规则命中的目录名,也不是 `.DS_Store`。
/// FSEvents 报的是规范化路径(`/private/var/…`),与项目根的写法可能不同,取不到相对路径时退回整条路径。
pub fn relevant(root: &Path, path: &Path, intel: &IntelPrefs) -> bool {
    let rel = path.strip_prefix(root).unwrap_or(path);
    !rel.components().any(|c| {
        let name = c.as_os_str().to_string_lossy();
        name == ".DS_Store" || intel.ignored(&name)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 忽略目录与系统文件里的事件不刷新() {
        let intel = IntelPrefs::default();
        let root = Path::new("/p");
        assert!(relevant(root, Path::new("/p/src/main.rs"), &intel));
        assert!(relevant(root, Path::new("/p/new_dir"), &intel));
        assert!(!relevant(root, Path::new("/p/target/debug/x"), &intel));
        assert!(!relevant(root, Path::new("/p/.git/index.lock"), &intel));
        assert!(!relevant(root, Path::new("/p/web/node_modules/a/b.js"), &intel));
        assert!(!relevant(root, Path::new("/p/src/.DS_Store"), &intel));
    }

    #[::core::prelude::v1::test]
    fn 根路径写法不同时按整条路径判断() {
        let intel = IntelPrefs::default();
        assert!(relevant(Path::new("/var/p"), Path::new("/private/var/p/a.rs"), &intel));
        assert!(!relevant(Path::new("/var/p"), Path::new("/private/var/p/target/a"), &intel));
    }

    #[::core::prelude::v1::test]
    fn 外部新建文件会触发信号_忽略目录里的不触发() {
        let dir = std::env::temp_dir().join(format!("loom-fswatch-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("target")).unwrap();
        let (_w, rx) = FsWatch::start(&dir, IntelPrefs::default()).expect("监视应当能启动");
        std::thread::sleep(std::time::Duration::from_millis(300));
        std::fs::write(dir.join("target").join("junk.o"), b"x").unwrap();
        assert!(rx.recv_blocking_timeout(1200).is_none(), "忽略目录里的改动不该触发");
        std::fs::write(dir.join("new.txt"), b"hi").unwrap();
        assert!(rx.recv_blocking_timeout(3000).is_some(), "新建文件应当触发信号");
        let _ = std::fs::remove_dir_all(&dir);
    }

    trait RecvTimeout {
        fn recv_blocking_timeout(&self, ms: u64) -> Option<()>;
    }
    impl RecvTimeout for async_channel::Receiver<()> {
        fn recv_blocking_timeout(&self, ms: u64) -> Option<()> {
            let end = std::time::Instant::now() + std::time::Duration::from_millis(ms);
            while std::time::Instant::now() < end {
                if self.try_recv().is_ok() {
                    return Some(());
                }
                std::thread::sleep(std::time::Duration::from_millis(30));
            }
            None
        }
    }
}
