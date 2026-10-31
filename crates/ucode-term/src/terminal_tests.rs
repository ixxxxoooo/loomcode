//! `Terminal` 的真实 pty 测试(拆出来是为了让 terminal.rs 不超过 300 行)。
//!
//! @author ixxxxoooo

use super::*;
use std::time::{Duration, Instant};

/// 轮询直到画面满足条件,最多等 5 秒;返回最后一次的画面文本。
fn wait_for(t: &Terminal, cond: impl Fn(&Snapshot) -> bool) -> Snapshot {
    let start = Instant::now();
    loop {
        let s = t.snapshot();
        if cond(&s) || start.elapsed() > Duration::from_secs(5) {
            return s;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn sh(script: &str) -> Terminal {
    Terminal::spawn(SpawnOptions { command: Some(vec!["/bin/sh".into(), "-c".into(), script.into()]), cols: 80, rows: 24, ..Default::default() }, || {}).unwrap()
}

#[test]
fn 命令输出与退出码() {
    let t = sh("echo hello-ucode; exit 3");
    let s = wait_for(&t, |s| s.exit_code.is_some());
    assert!(s.text().contains("hello-ucode"), "画面:{:?}", s.text());
    assert_eq!(s.exit_code, Some(3));
    assert_eq!(t.exit_code(), Some(3));
}

#[test]
fn 交互输入与回显() {
    let t = Terminal::spawn(SpawnOptions { command: Some(vec!["/bin/sh".into()]), cols: 80, rows: 24, ..Default::default() }, || {}).unwrap();
    t.write(b"echo $((6*7))\n");
    let s = wait_for(&t, |s| s.text().lines().any(|l| l.trim() == "42"));
    assert!(s.text().lines().any(|l| l.trim() == "42"), "画面:{:?}", s.text());
    t.write(b"exit 0\n");
    assert_eq!(wait_for(&t, |s| s.exit_code.is_some()).exit_code, Some(0));
}

#[test]
fn 调整大小后程序看到新尺寸() {
    let t = Terminal::spawn(SpawnOptions { command: Some(vec!["/bin/sh".into()]), cols: 80, rows: 24, ..Default::default() }, || {}).unwrap();
    t.resize(100, 30);
    t.write(b"stty size\n");
    let s = wait_for(&t, |s| s.text().contains("30 100"));
    assert!(s.text().contains("30 100"), "画面:{:?}", s.text());
    assert_eq!((s.rows, s.cols), (30, 100));
}

#[test]
fn 工作目录与颜色输出() {
    let dir = std::env::temp_dir().canonicalize().unwrap();
    let t = Terminal::spawn(SpawnOptions { cwd: Some(dir.to_string_lossy().to_string()), command: Some(vec!["/bin/sh".into(), "-c".into(), "pwd; printf '\\033[31mred\\033[0m\\n'".into()]), cols: 80, rows: 24 }, || {}).unwrap();
    let s = wait_for(&t, |s| s.exit_code.is_some());
    assert!(s.text().contains(dir.to_string_lossy().as_ref()), "画面:{:?}", s.text());
    let red = s.lines.iter().flatten().find(|r| r.text == "red").expect("应有红色文字段");
    assert_eq!(red.fg, crate::TermColor::Indexed(1));
}

#[test]
fn 先输出后缩窄不丢内容() {
    // 模拟「命令在默认 80 列下已经输出完,面板才量出实际宽度并缩窄」
    let t = sh("for i in 0 1 2 3 4 5 6 7; do echo row$i-xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx; done; sleep 1");
    wait_for(&t, |s| s.text().contains("row7-"));
    t.resize(40, 60);
    let s = t.snapshot();
    let flat: String = s.text().chars().filter(|c| !c.is_whitespace()).collect();
    for i in 0..8 {
        assert!(flat.contains(&format!("row{i}-")), "丢了 row{i},画面:{:?}", s.text());
    }
}

#[test]
fn 更新回调会被触发() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let n = Arc::new(AtomicUsize::new(0));
    let n2 = n.clone();
    let t = Terminal::spawn(SpawnOptions { command: Some(vec!["/bin/sh".into(), "-c".into(), "echo x".into()]), cols: 80, rows: 24, ..Default::default() }, move || {
        n2.fetch_add(1, Ordering::SeqCst);
    })
    .unwrap();
    wait_for(&t, |s| s.exit_code.is_some());
    assert!(n.load(Ordering::SeqCst) >= 1);
}

#[test]
fn kill_后读线程会收尸() {
    let t = sh("sleep 30");
    assert_eq!(t.exit_code(), None);
    t.kill();
    let s = wait_for(&t, |s| s.exit_code.is_some());
    assert!(s.exit_code.is_some(), "kill 后读线程应收尸");
}

/// 进程是否还活着(`kill -0` 不发信号,只探测)。
fn alive(pid: u32) -> bool {
    std::process::Command::new("kill").args(["-0", &pid.to_string()]).stderr(std::process::Stdio::null()).status().map(|s| s.success()).unwrap_or(false)
}

#[test]
fn 丢弃终端会结束子进程() {
    let t = sh("sleep 30");
    let pid = t.child.lock().unwrap().process_id().expect("应有进程号");
    assert!(alive(pid));
    drop(t);
    let start = Instant::now();
    while alive(pid) && start.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(!alive(pid), "Drop 后子进程应被结束");
}

/// 找到画面里包含某段文字的行号。
fn row_of(t: &Terminal, needle: &str) -> usize {
    let text = t.snapshot().text();
    text.lines().position(|l| l.contains(needle)).unwrap_or_else(|| panic!("画面里没有 {needle}:{text:?}"))
}

#[test]
fn 拖动选择字符并取文本() {
    let t = sh("echo hello world; sleep 2");
    wait_for(&t, |s| s.text().contains("hello world"));
    let row = row_of(&t, "hello world");
    t.start_selection(row, 0, false, SelectKind::Simple);
    t.update_selection(row, 4, true);
    assert_eq!(t.selection_text().as_deref(), Some("hello"));
    assert_eq!(t.snapshot().selection, Some(((row, 0), (row, 4))));
    t.clear_selection();
    assert_eq!(t.selection_text(), None);
    assert_eq!(t.snapshot().selection, None);
}

#[test]
fn 双击选单词_三击选整行() {
    let t = sh("echo hello world; sleep 2");
    wait_for(&t, |s| s.text().contains("hello world"));
    let row = row_of(&t, "hello world");
    t.start_selection(row, 8, false, SelectKind::Word);
    assert_eq!(t.selection_text().as_deref(), Some("world"));
    t.start_selection(row, 3, false, SelectKind::Line);
    assert!(t.selection_text().unwrap().contains("hello world"));
}

#[test]
fn 越界坐标被夹到边缘不会崩() {
    let t = sh("echo hi; sleep 2");
    wait_for(&t, |s| s.text().contains("hi"));
    t.start_selection(9999, 9999, true, SelectKind::Simple);
    t.update_selection(0, 0, false);
    assert!(t.snapshot().selection.is_some());
}
