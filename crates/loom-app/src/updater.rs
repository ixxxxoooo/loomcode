//! 应用更新的界面侧状态与动作:检查 → 下载校验 → 重启替换。网络与安装逻辑在 `loom_config::update`。
//!
//! @author ixxxxoooo

use crate::workbench::Workbench;
use gpui_kit::*;
use loom_config::update::{self, Progress, StagedUpdate, UpdateInfo, UpdateResult};
use std::path::PathBuf;

/// 当前构建号(发布脚本通过环境变量 `LOOM_BUILD` 注入,开发版为 0)。
pub fn current_build() -> u64 {
    option_env!("LOOM_BUILD").and_then(|b| b.parse().ok()).unwrap_or(0)
}

/// 更新流程的状态。
#[derive(Clone, Default)]
pub enum UpdateState {
    #[default]
    Idle,
    Checking,
    UpToDate,
    Available(UpdateInfo),
    /// 下载中:版本信息 + 当前进度(下载 / 校验 / 解包)
    Downloading(UpdateInfo, Progress),
    Ready(StagedUpdate),
    Failed(String),
}

/// 当前运行的 `.app` 包路径;不是从应用包启动(开发版、`cargo run`)时为 `None`。
pub fn current_bundle() -> Option<PathBuf> {
    // 开发版也在 .app 包里(target/dev/LoomCode Dev.app),但那不是可被更新替换的正式安装
    if std::env::var_os("LOOM_DEV").is_some() {
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    exe.ancestors().find(|p| p.extension().is_some_and(|e| e == "app")).map(PathBuf::from)
}

impl Workbench {
    /// 启动时的更新动作:恢复已暂存的更新、提示上次安装失败,并(按设置)自动检查。
    pub fn startup_update(&mut self, cx: &mut Context<Self>) {
        // 新窗口进程不管更新(主窗口负责,免得两个进程同时下载 / 安装)
        if crate::secondary::is() {
            return;
        }
        // 调试用:LOOM_UPDATE_DEMO=1 模拟一次完整的下载过程(下载 → 校验 → 解包 → 可重启);=hold 停在 42% 方便截图
        if let Ok(mode) = std::env::var("LOOM_UPDATE_DEMO") {
            self.simulate_update(mode == "hold", cx);
            return;
        }
        if let Some(why) = update::take_install_failure() {
            self.update = UpdateState::Failed(format!("上次更新安装失败({why});常见原因是应用所在目录不可写,可手动下载安装"));
            return;
        }
        if let Some(staged) = update::load_staged(env!("CARGO_PKG_VERSION"), current_build()) {
            self.update = UpdateState::Ready(staged);
        } else if self.settings.auto_update_check && std::env::var_os("LOOM_DEV").is_none() {
            self.check_update(cx);
        }
    }

    /// 模拟下载过程(只改界面状态,不联网)。`hold` 为真时停在 42%。
    fn simulate_update(&mut self, hold: bool, cx: &mut Context<Self>) {
        const TOTAL: u64 = 86_000_000;
        let info = UpdateInfo { version: "9.9.9".into(), build: Some(9999), url: "https://example.invalid/LoomCode.dmg".into(), sha256: None, notes: None };
        let at = |received| Progress::Downloading { received, total: Some(TOTAL) };
        self.update = UpdateState::Downloading(info.clone(), at(if hold { TOTAL * 42 / 100 } else { 0 }));
        cx.notify();
        if hold {
            return;
        }
        cx.spawn(async move |this, cx| {
            let set = |p: Progress, cx: &mut AsyncApp| { let _ = this.update(cx, |this, cx| { if let UpdateState::Downloading(_, cur) = &mut this.update { *cur = p; cx.notify(); } }); };
            for pct in 1..=100u64 {
                cx.background_executor().timer(std::time::Duration::from_millis(120)).await;
                set(at(TOTAL * pct / 100), cx);
            }
            set(Progress::Verifying, cx);
            cx.background_executor().timer(std::time::Duration::from_millis(1500)).await;
            set(Progress::Unpacking, cx);
            cx.background_executor().timer(std::time::Duration::from_millis(1500)).await;
            let _ = this.update(cx, |this, cx| {
                this.update = UpdateState::Ready(StagedUpdate { version: info.version.clone(), build: 9999, app_path: String::new(), notes: None });
                this.announce_update(format!("v{} 已下载并校验,重启即可更新", info.version), "重启更新", Self::install_update, cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// 后台检查更新。
    pub fn check_update(&mut self, cx: &mut Context<Self>) {
        if matches!(self.update, UpdateState::Checking | UpdateState::Downloading(..)) {
            return;
        }
        self.update = UpdateState::Checking;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx.background_spawn(async { update::check("", env!("CARGO_PKG_VERSION"), current_build()) }).await;
            let _ = this.update(cx, |this, cx| {
                this.update = match result {
                    UpdateResult::UpToDate => {
                        loom_diag::info!("检查更新:已是最新版本");
                        UpdateState::UpToDate
                    }
                    UpdateResult::Available(info) => {
                        loom_diag::info!("检查更新:发现新版本");
                        crate::notice::post(cx, crate::notice::Kind::Info, format!("发现新版本 v{},正在后台下载…", info.version), None);
                        if crate::notice::has_ide_window(cx) {
                            this.update_reminded = true;
                        }
                        UpdateState::Available(info)
                    }
                    UpdateResult::Failed(e) => {
                        loom_diag::warn!("检查更新失败:{e}");
                        UpdateState::Failed(e)
                    }
                };
                cx.notify();
                // 发现新版本就自动下载(进度显示在侧栏胶囊、IDE 状态栏、关于页与设置里)
                if matches!(this.update, UpdateState::Available(_)) {
                    this.download_update(cx);
                }
            });
        })
        .detach();
    }

    /// 把更新进展发到通知中心(IDE 右下角弹出并留在铃铛历史里,带动作按钮);Agent 窗口不显示。
    fn announce_update(&mut self, message: String, label: &'static str, run: fn(&mut Workbench, &mut Context<Workbench>), cx: &mut Context<Self>) {
        if crate::notice::has_ide_window(cx) {
            self.update_reminded = true;
        }
        let wb = cx.entity().downgrade();
        let action = crate::notice::NoticeAction {
            label: label.into(),
            run: std::rc::Rc::new(move |_, cx| { let _ = wb.update(cx, |w, cx| run(w, cx)); }),
        };
        crate::notice::post(cx, crate::notice::Kind::Info, message, Some(action));
    }

    /// 进入 IDE(融合或独立窗口)时:如果有新版本(或已下载待重启)而 IDE 里还没提醒过,补发一条通知。
    /// IDE 窗口此时多半还没登记 / 没渲染,稍等片刻再发,这样右下角能弹出来。
    pub fn remind_update_in_ide(&mut self, cx: &mut Context<Self>) {
        if self.update_reminded || !matches!(self.update, UpdateState::Available(_) | UpdateState::Ready(_)) {
            return;
        }
        self.update_reminded = true;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(std::time::Duration::from_millis(800)).await;
            let _ = this.update(cx, |this, cx| match this.update.clone() {
                UpdateState::Available(i) => this.announce_update(format!("发现新版本 v{}", i.version), "下载更新", Self::download_update, cx),
                UpdateState::Ready(s) => this.announce_update(format!("v{} 已下载并校验,重启即可更新", s.version), "重启更新", Self::install_update, cx),
                _ => {}
            });
        })
        .detach();
    }

    /// 后台下载并校验新版本,完成后进入「可重启更新」。下载 / 校验 / 解包的进度随时更新到 `UpdateState::Downloading`。
    pub fn download_update(&mut self, cx: &mut Context<Self>) {
        let UpdateState::Available(info) = self.update.clone() else { return };
        self.update = UpdateState::Downloading(info.clone(), Progress::default());
        cx.notify();
        // 后台线程把进度送进通道,界面侧只在显示文字变了(百分比跳一格、换阶段)时刷新,省得每个数据块都重绘
        let (tx, rx) = async_channel::unbounded::<Progress>();
        cx.spawn(async move |this, cx| {
            let mut shown = String::new();
            while let Ok(p) = rx.recv().await {
                let text = p.short();
                if text == shown {
                    continue;
                }
                shown = text;
                let alive = this.update(cx, |this, cx| {
                    if let UpdateState::Downloading(_, cur) = &mut this.update {
                        *cur = p;
                        cx.notify();
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
        cx.spawn(async move |this, cx| {
            let result = cx.background_spawn(async move { update::download_with_progress(&info, &mut |p| { let _ = tx.try_send(p); }) }).await;
            let _ = this.update(cx, |this, cx| {
                this.update = match result {
                    Ok(staged) => {
                        loom_diag::info!("更新包下载并校验完成");
                        this.announce_update(format!("v{} 已下载并校验,重启即可更新", staged.version), "重启更新", Self::install_update, cx);
                        UpdateState::Ready(staged)
                    }
                    Err(e) => {
                        loom_diag::warn!("更新包下载失败:{e}");
                        crate::notice::post(cx, crate::notice::Kind::Warning, format!("更新下载失败:{e}"), None);
                        UpdateState::Failed(e)
                    }
                };
                cx.notify();
            });
        })
        .detach();
    }

    /// 退出并由脚本替换应用后重启。不是从应用包运行时只提示。
    pub fn install_update(&mut self, cx: &mut Context<Self>) {
        let UpdateState::Ready(staged) = self.update.clone() else { return };
        let Some(bundle) = current_bundle() else {
            self.update = UpdateState::Failed("当前不是从应用包(.app)运行,无法自动替换;请手动安装新版本".into());
            cx.notify();
            return;
        };
        match update::launch_replacer(&staged, &bundle) {
            Ok(()) => {
                loom_lsp::hub::kill_all();
                loom_diag::crash::clean_exit("应用更新后重启");
                cx.quit()
            }
            Err(e) => {
                self.update = UpdateState::Failed(e);
                cx.notify();
            }
        }
    }

    /// 正在下载 / 校验 / 解包新版本时,状态栏要显示的 (文字, 悬停说明);其它状态为 `None`。
    pub fn update_badge(&self) -> Option<(String, String)> {
        let UpdateState::Downloading(i, p) = &self.update else { return None };
        let text = match p {
            Progress::Downloading { .. } => format!("更新 {}", p.short()),
            _ => format!("更新{}", p.short()),
        };
        Some((text, format!("v{} · {}", i.version, p.detail())))
    }

    /// 更新状态的一句话说明(关于页显示)。
    pub fn update_text(&self) -> String {
        match &self.update {
            UpdateState::Idle => "尚未检查".into(),
            UpdateState::Checking => "正在检查更新…".into(),
            UpdateState::UpToDate => "已是最新版本".into(),
            UpdateState::Available(i) => format!("发现新版本 v{}", i.version),
            UpdateState::Downloading(i, p) => match p {
                Progress::Downloading { .. } => format!("正在下载 v{} · {}", i.version, p.detail()),
                _ => format!("v{} · {}", i.version, p.detail()),
            },
            UpdateState::Ready(s) => format!("v{} 已下载并校验,重启即可更新", s.version),
            UpdateState::Failed(e) => e.clone(),
        }
    }
}
