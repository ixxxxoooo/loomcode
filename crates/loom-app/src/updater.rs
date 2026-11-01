//! 应用更新的界面侧状态与动作:检查 → 下载校验 → 重启替换。网络与安装逻辑在 `loom_config::update`。
//!
//! @author ixxxxoooo

use crate::workbench::Workbench;
use gpui_kit::*;
use loom_config::update::{self, StagedUpdate, UpdateInfo, UpdateResult};
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
    Downloading(UpdateInfo),
    Ready(StagedUpdate),
    Failed(String),
}

/// 当前运行的 `.app` 包路径;不是从应用包启动(开发版、`cargo run`)时为 `None`。
pub fn current_bundle() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    exe.ancestors().find(|p| p.extension().is_some_and(|e| e == "app")).map(PathBuf::from)
}

impl Workbench {
    /// 启动时的更新动作:恢复已暂存的更新、提示上次安装失败,并(按设置)自动检查。
    pub fn startup_update(&mut self, cx: &mut Context<Self>) {
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

    /// 后台检查更新。
    pub fn check_update(&mut self, cx: &mut Context<Self>) {
        if matches!(self.update, UpdateState::Checking | UpdateState::Downloading(_)) {
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
                        UpdateState::Available(info)
                    }
                    UpdateResult::Failed(e) => {
                        loom_diag::warn!("检查更新失败:{e}");
                        UpdateState::Failed(e)
                    }
                };
                cx.notify();
            });
        })
        .detach();
    }

    /// 后台下载并校验新版本,完成后进入「可重启更新」。
    pub fn download_update(&mut self, cx: &mut Context<Self>) {
        let UpdateState::Available(info) = self.update.clone() else { return };
        self.update = UpdateState::Downloading(info.clone());
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx.background_spawn(async move { update::download(&info) }).await;
            let _ = this.update(cx, |this, cx| {
                this.update = match result {
                    Ok(staged) => {
                        loom_diag::info!("更新包下载并校验完成");
                        UpdateState::Ready(staged)
                    }
                    Err(e) => {
                        loom_diag::warn!("更新包下载失败:{e}");
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
            Ok(()) => cx.quit(),
            Err(e) => {
                self.update = UpdateState::Failed(e);
                cx.notify();
            }
        }
    }

    /// 更新状态的一句话说明(关于页显示)。
    pub fn update_text(&self) -> String {
        match &self.update {
            UpdateState::Idle => "尚未检查".into(),
            UpdateState::Checking => "正在检查更新…".into(),
            UpdateState::UpToDate => "已是最新版本".into(),
            UpdateState::Available(i) => format!("发现新版本 v{}", i.version),
            UpdateState::Downloading(i) => format!("正在下载 v{}…", i.version),
            UpdateState::Ready(s) => format!("v{} 已下载并校验,重启即可更新", s.version),
            UpdateState::Failed(e) => e.clone(),
        }
    }
}
