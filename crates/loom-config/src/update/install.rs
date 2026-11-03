//! 更新安装:下载 dmg → 校验 sha256 → 挂载取出 app 暂存 → 退出后由脚本替换并重启。
//!
//! @author ixxxxoooo

use super::UpdateInfo;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

/// 暂存好的更新(已下载并校验)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StagedUpdate {
    pub version: String,
    pub build: u64,
    /// 暂存的 app 路径
    pub app_path: String,
    pub notes: Option<String>,
}

/// 暂存根目录 `~/.loomcode/updates`。
fn staging_root() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".loomcode").join("updates")
}

/// 清理全部暂存。
fn clear_staged() {
    let _ = std::fs::remove_dir_all(staging_root());
}

/// 读取已暂存且比当前版本新的更新;过期的暂存会被清理。
///
/// 参数:`current` 当前版本;`build` 当前构建号。
pub fn load_staged(current: &str, build: u64) -> Option<StagedUpdate> {
    let staged: StagedUpdate = serde_json::from_slice(&std::fs::read(staging_root().join("ready.json")).ok()?).ok()?;
    let order = super::compare(&staged.version, current);
    if !Path::new(&staged.app_path).exists() || !(order > 0 || (order == 0 && staged.build > build)) {
        clear_staged();
        return None;
    }
    Some(staged)
}

/// 文件的 sha256(小写十六进制),调用系统 `shasum`。
fn sha256(file: &Path) -> Result<String, String> {
    let out = Command::new("/usr/bin/shasum").args(["-a", "256"]).arg(file).output().map_err(|e| format!("无法计算校验值:{e}"))?;
    String::from_utf8_lossy(&out.stdout).split_whitespace().next().map(str::to_lowercase).ok_or_else(|| "无法计算校验值".to_string())
}

/// 运行系统命令,失败时返回输出。
fn run(tool: &str, args: &[&str]) -> Result<(), String> {
    let out = Command::new(tool).args(args).output().map_err(|e| format!("{tool}:{e}"))?;
    if out.status.success() { Ok(()) } else { Err(format!("{tool} 失败:{}", String::from_utf8_lossy(&out.stderr).trim())) }
}

/// 下载进度:下载 → 校验 → 解包验签三段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress {
    /// 下载中:已收到的字节数与总字节数(服务器没给长度时为 `None`)
    Downloading { received: u64, total: Option<u64> },
    /// 正在校验 sha256
    Verifying,
    /// 正在解包并验证签名
    Unpacking,
}

impl Default for Progress {
    fn default() -> Self {
        Progress::Downloading { received: 0, total: None }
    }
}

impl Progress {
    /// 完成比例 0..=1:只有下载阶段且知道总长度时才有。
    pub fn fraction(&self) -> Option<f32> {
        match *self {
            Progress::Downloading { received, total: Some(t) } if t > 0 => Some((received as f32 / t as f32).clamp(0., 1.)),
            _ => None,
        }
    }

    /// 简短文字:`42%` / `已下载 34.2 MB` / `校验中` / `解包中`。
    pub fn short(&self) -> String {
        match *self {
            Progress::Downloading { received, .. } => match self.fraction() {
                Some(f) => format!("{}%", (f * 100.).floor() as u32),
                None => format!("已下载 {}", format_bytes(received)),
            },
            Progress::Verifying => "校验中".into(),
            Progress::Unpacking => "解包中".into(),
        }
    }

    /// 详细文字:`42% · 34.2 / 82.0 MB` 等。
    pub fn detail(&self) -> String {
        match *self {
            Progress::Downloading { received, total: Some(t) } => format!("{} · {} / {}", self.short(), format_bytes(received), format_bytes(t)),
            Progress::Downloading { received, total: None } => format!("已下载 {}", format_bytes(received)),
            Progress::Verifying => "正在校验安装包…".into(),
            Progress::Unpacking => "正在解包并验证签名…".into(),
        }
    }
}

/// 字节数的可读文字(1024 进制,一位小数):`812 B` / `34.2 KB` / `82.0 MB`。
pub fn format_bytes(n: u64) -> String {
    const K: f64 = 1024.;
    let n = n as f64;
    if n < K {
        format!("{n:.0} B")
    } else if n < K * K {
        format!("{:.1} KB", n / K)
    } else if n < K * K * K {
        format!("{:.1} MB", n / K / K)
    } else {
        format!("{:.2} GB", n / K / K / K)
    }
}

/// 下载并校验、暂存更新(阻塞,界面在后台线程调用)。不关心进度时用这个。
///
/// 参数:`info` 版本信息(必须带 https 地址与 sha256)。返回:暂存结果或错误提示。
pub fn download(info: &UpdateInfo) -> Result<StagedUpdate, String> {
    download_with_progress(info, &mut |_| {})
}

/// 同 [`download`],并通过 `on_progress` 报告进度(下载阶段每 150 ms 至多一次,阶段切换立即报告)。
pub fn download_with_progress(info: &UpdateInfo, on_progress: &mut dyn FnMut(Progress)) -> Result<StagedUpdate, String> {
    if !info.url.starts_with("https://") {
        return Err("下载地址必须是 https".into());
    }
    let expected = info.sha256.as_deref().map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty()).ok_or("发布源缺少安装包校验值(sha256),已拒绝安装")?;
    clear_staged();
    let dir = staging_root().join(&info.version);
    std::fs::create_dir_all(&dir).map_err(|e| format!("无法创建暂存目录:{e}"))?;
    let dmg = dir.join("update.dmg");
    let resp = ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(600)).build().get(&info.url).call().map_err(|e| format!("下载失败:{e}"))?;
    let total = resp.header("Content-Length").and_then(|v| v.trim().parse::<u64>().ok()).filter(|n| *n > 0);
    let mut file = std::fs::File::create(&dmg).map_err(|e| format!("无法写入安装包:{e}"))?;
    copy_with_progress(&mut resp.into_reader(), &mut file, total, on_progress).map_err(|e| format!("下载中断:{e}"))?;
    drop(file);
    on_progress(Progress::Verifying);
    if sha256(&dmg)? != expected {
        clear_staged();
        return Err("安装包校验失败(sha256 不一致)".into());
    }
    on_progress(Progress::Unpacking);
    let app = extract_app(&dmg, &dir)?;
    let _ = std::fs::remove_file(&dmg);
    if let Err(e) = run("/usr/bin/codesign", &["--verify", "--deep", "--strict", &app.to_string_lossy()]) {
        clear_staged();
        return Err(format!("安装包签名无效:{e}"));
    }
    let staged = StagedUpdate { version: info.version.clone(), build: info.build.unwrap_or(0), app_path: app.to_string_lossy().to_string(), notes: info.notes.clone() };
    std::fs::write(staging_root().join("ready.json"), serde_json::to_vec(&staged).unwrap_or_default()).map_err(|e| format!("无法记录暂存:{e}"))?;
    Ok(staged)
}

/// 边读边写并报告进度:每 150 ms 至多报一次,结束时一定报一次最终字节数。
fn copy_with_progress(from: &mut dyn std::io::Read, to: &mut dyn std::io::Write, total: Option<u64>, on_progress: &mut dyn FnMut(Progress)) -> std::io::Result<u64> {
    let mut buf = vec![0u8; 64 * 1024];
    let (mut received, mut last) = (0u64, std::time::Instant::now());
    on_progress(Progress::Downloading { received: 0, total });
    loop {
        let n = from.read(&mut buf)?;
        if n == 0 {
            break;
        }
        to.write_all(&buf[..n])?;
        received += n as u64;
        if last.elapsed() >= std::time::Duration::from_millis(150) {
            last = std::time::Instant::now();
            on_progress(Progress::Downloading { received, total });
        }
    }
    on_progress(Progress::Downloading { received, total: total.or(Some(received)) });
    Ok(received)
}

/// 挂载 dmg,把其中的 .app 拷到暂存目录后卸载。
fn extract_app(dmg: &Path, dir: &Path) -> Result<PathBuf, String> {
    let mount = dir.join("mnt");
    std::fs::create_dir_all(&mount).map_err(|e| e.to_string())?;
    run("/usr/bin/hdiutil", &["attach", "-nobrowse", "-readonly", "-noverify", "-mountpoint", &mount.to_string_lossy(), &dmg.to_string_lossy()])?;
    let result = (|| {
        let name = std::fs::read_dir(&mount).map_err(|e| e.to_string())?.flatten().map(|e| e.file_name().to_string_lossy().to_string()).find(|n| n.ends_with(".app")).ok_or("安装包中没有应用")?;
        let dest = dir.join("LoomCode.app");
        run("/usr/bin/ditto", &[&mount.join(name).to_string_lossy(), &dest.to_string_lossy()])?;
        Ok(dest)
    })();
    let _ = run("/usr/bin/hdiutil", &["detach", "-quiet", "-force", &mount.to_string_lossy()]);
    let _ = std::fs::remove_dir_all(&mount);
    result
}

/// 「等旧进程退出 → 替换 → 重启」脚本。路径一律作为位置参数传入,脚本里不拼接路径。
/// 替换失败时恢复原应用并写下失败标记,下次启动提示。参数:$1 旧 PID、$2 目标 app、$3 暂存 app、$4 暂存根目录。
const REPLACE_SCRIPT: &str = r#"#!/bin/sh
PID="$1"; TARGET="$2"; STAGED="$3"; ROOT="$4"
LOG="$ROOT/replace.log"
i=0; while kill -0 "$PID" 2>/dev/null && [ $i -lt 120 ]; do sleep 0.5; i=$((i+1)); done
if kill -0 "$PID" 2>/dev/null; then exit 0; fi
BACKUP="$TARGET.old"
rm -rf "$BACKUP"
if mv "$TARGET" "$BACKUP" 2>>"$LOG"; then
  if /usr/bin/ditto "$STAGED" "$TARGET" 2>>"$LOG"; then
    /usr/bin/xattr -dr com.apple.quarantine "$TARGET" 2>/dev/null
    rm -rf "$BACKUP" "$ROOT"
  else
    rm -rf "$TARGET"; mv "$BACKUP" "$TARGET"; echo "copy" > "$ROOT/failed"
  fi
else
  echo "move" > "$ROOT/failed"
fi
/usr/bin/open "$TARGET"
"#;

/// 生成替换脚本并在后台启动;调用方随后应退出应用。
///
/// 参数:`staged` 暂存的更新;`target_app` 要被替换的应用(当前 bundle)。
pub fn launch_replacer(staged: &StagedUpdate, target_app: &Path) -> Result<(), String> {
    let root = staging_root();
    let script = root.join("replace.sh");
    std::fs::write(&script, REPLACE_SCRIPT).map_err(|e| format!("无法写入替换脚本:{e}"))?;
    Command::new("/bin/sh").arg(&script).arg(std::process::id().to_string()).arg(target_app).arg(&staged.app_path).arg(&root)
        .stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null())
        .spawn().map(|_| ()).map_err(|e| format!("无法启动替换脚本:{e}"))
}

/// 读取并清理上次安装失败的记录(常见原因是应用所在目录不可写)。返回失败说明;上次没有失败为 `None`。
pub fn take_install_failure() -> Option<String> {
    let root = staging_root();
    let step = std::fs::read_to_string(root.join("failed")).ok()?;
    let log = std::fs::read_to_string(root.join("replace.log")).unwrap_or_default();
    clear_staged();
    let tail: String = log.chars().rev().take(300).collect::<Vec<_>>().into_iter().rev().collect();
    Some(format!("{}:{}", step.trim(), if tail.trim().is_empty() { "-".into() } else { tail.trim().to_string() }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 字节数可读() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(812), "812 B");
        assert_eq!(format_bytes(35_000), "34.2 KB");
        assert_eq!(format_bytes(86_000_000), "82.0 MB");
        assert_eq!(format_bytes(3 * 1024 * 1024 * 1024), "3.00 GB");
    }

    #[test]
    fn 进度文字() {
        let p = Progress::Downloading { received: 43_000_000, total: Some(86_000_000) };
        assert_eq!(p.fraction(), Some(0.5));
        assert_eq!(p.short(), "50%");
        assert_eq!(p.detail(), "50% · 41.0 MB / 82.0 MB");
        // 不满 1% 向下取整,100% 只在真的收完时出现
        assert_eq!(Progress::Downloading { received: 999, total: Some(1000) }.short(), "99%");
        assert_eq!(Progress::Downloading { received: 1000, total: Some(1000) }.short(), "100%");
        // 服务器没给长度:只显示已下载量,没有比例
        let unknown = Progress::Downloading { received: 5 * 1024 * 1024, total: None };
        assert_eq!(unknown.fraction(), None);
        assert_eq!(unknown.short(), "已下载 5.0 MB");
        assert_eq!(Progress::Verifying.short(), "校验中");
        assert_eq!(Progress::Unpacking.fraction(), None);
        assert_eq!(Progress::default(), Progress::Downloading { received: 0, total: None });
    }

    #[test]
    fn 边拷边报进度且最终报满() {
        let data = vec![7u8; 300_000];
        let mut out = Vec::new();
        let mut seen = Vec::new();
        let n = copy_with_progress(&mut data.as_slice(), &mut out, Some(300_000), &mut |p| seen.push(p)).unwrap();
        assert_eq!(n, 300_000);
        assert_eq!(out, data);
        // 第一次报 0,最后一次报满;中间的已收字节单调不减
        assert_eq!(seen.first(), Some(&Progress::Downloading { received: 0, total: Some(300_000) }));
        assert_eq!(seen.last(), Some(&Progress::Downloading { received: 300_000, total: Some(300_000) }));
        let got: Vec<u64> = seen.iter().filter_map(|p| if let Progress::Downloading { received, .. } = p { Some(*received) } else { None }).collect();
        assert!(got.windows(2).all(|w| w[0] <= w[1]));
        // 没有总长度时,收完补上总长度,界面能显示到 100%
        let mut seen = Vec::new();
        copy_with_progress(&mut data.as_slice(), &mut Vec::new(), None, &mut |p| seen.push(p)).unwrap();
        assert_eq!(seen.last().and_then(Progress::fraction), Some(1.0));
    }
}
