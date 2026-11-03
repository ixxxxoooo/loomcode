//! 把构建时间与提交短哈希编进二进制,给「关于」窗口显示。
//!
//! @author ixxxxoooo

fn main() {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    println!("cargo:rustc-env=LOOM_BUILD_TIME={now}");
    let hash = std::process::Command::new("git").args(["rev-parse", "--short", "HEAD"]).output().ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    println!("cargo:rustc-env=LOOM_GIT_HASH={hash}");
    // 每次提交(HEAD 或所在分支引用变化)后重新取;没有 .git 时不报错
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/refs/heads");
    println!("cargo:rerun-if-env-changed=LOOM_BUILD");
}
