//! 服务器中心:按「项目根 + 服务器 id」复用语言服务器,首次使用时才启动;诊断通过全局回调送出。

use crate::client::{Client, NotifyFn};
use crate::servers::{self, ServerSpec};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

/// 收到 `publishDiagnostics` 时的回调:`(uri, 文档版本, 诊断数组)`。
pub type DiagnosticsSink = Arc<dyn Fn(String, Option<i64>, Value) + Send + Sync>;

type Slot = Arc<OnceLock<Result<Arc<Client>, String>>>;

fn table() -> &'static Mutex<HashMap<(String, &'static str), (Slot, &'static ServerSpec)>> {
    static T: OnceLock<Mutex<HashMap<(String, &'static str), (Slot, &'static ServerSpec)>>> = OnceLock::new();
    T.get_or_init(Mutex::default)
}

fn sink() -> &'static Mutex<Option<DiagnosticsSink>> {
    static S: OnceLock<Mutex<Option<DiagnosticsSink>>> = OnceLock::new();
    S.get_or_init(Mutex::default)
}

/// 设置诊断回调(界面启动时调用一次)。
pub fn set_diagnostics_sink(f: DiagnosticsSink) {
    *sink().lock().unwrap() = Some(f);
}

/// 取得(必要时启动)某项目的语言服务器。阻塞,请在后台线程调用;并发调用同一个键只会启动一次,其余等待。
/// 失败(未安装、启动或握手失败)的结果会被记住,本次运行内不再重试。
pub fn get_or_start(root: &str, spec: &'static ServerSpec) -> Result<Arc<Client>, String> {
    let slot = {
        let mut t = table().lock().unwrap();
        t.entry((root.to_string(), spec.id)).or_insert_with(|| (Slot::default(), spec)).0.clone()
    };
    slot.get_or_init(|| start(root, spec)).clone()
}

fn start(root: &str, spec: &'static ServerSpec) -> Result<Arc<Client>, String> {
    let Some(exe) = servers::find_binary(spec.cmd) else {
        loom_diag::info!("未找到语言服务器 {}({}),跳过", spec.label, spec.cmd);
        return Err(format!("未安装 {}(安装:{})", spec.label, spec.install));
    };
    let notify: NotifyFn = Arc::new(|method, params| {
        if method == "textDocument/publishDiagnostics" {
            let uri = params.get("uri").and_then(Value::as_str).unwrap_or("").to_string();
            let version = params.get("version").and_then(Value::as_i64);
            let diags = params.get("diagnostics").cloned().unwrap_or(Value::Array(vec![]));
            if let Some(f) = sink().lock().ok().and_then(|s| s.clone()) {
                f(uri, version, diags);
            }
        }
    });
    let t = std::time::Instant::now();
    loom_diag::info!("启动语言服务器 {}:{} (项目 {root})", spec.label, exe.display());
    match Client::spawn(spec.label, &exe, spec.args, Path::new(root), servers::init_options(spec), notify) {
        Ok(c) => {
            loom_diag::info!("语言服务器 {} 已就绪,握手耗时 {} ms", spec.label, t.elapsed().as_millis());
            Ok(c)
        }
        Err(e) => {
            loom_diag::warn!("语言服务器 {} 启动失败:{e}", spec.label);
            Err(e)
        }
    }
}

/// 某服务器的当前状态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerState {
    Starting,
    /// 就绪;带正在进行的后台工作(如「Indexing」)
    Ready(Option<String>),
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct ServerStatus {
    pub label: &'static str,
    pub root: String,
    pub state: ServerState,
}

/// 所有服务器的状态快照(状态栏用)。
pub fn status() -> Vec<ServerStatus> {
    let t = table().lock().unwrap();
    let mut v: Vec<ServerStatus> = t
        .iter()
        .map(|((root, _), (slot, spec))| ServerStatus {
            label: spec.label,
            root: root.clone(),
            state: match slot.get() {
                None => ServerState::Starting,
                Some(Ok(c)) if c.is_alive() => ServerState::Ready(c.activity()),
                Some(Ok(_)) => ServerState::Failed("已退出".into()),
                Some(Err(e)) => ServerState::Failed(e.clone()),
            },
        })
        .collect();
    v.sort_by(|a, b| (a.label, &a.root).cmp(&(b.label, &b.root)));
    v
}

/// 立刻结束全部语言服务器(应用退出前调用,不等待,避免遗留孤儿进程)。
pub fn kill_all() {
    let clients: Vec<Arc<Client>> = table().lock().unwrap().values().filter_map(|(s, _)| s.get().and_then(|r| r.as_ref().ok().cloned())).collect();
    for c in clients {
        c.kill();
    }
}

/// 礼貌地关闭全部语言服务器(`shutdown` + `exit`,最多等几秒)。
pub fn shutdown_all() {
    let clients: Vec<Arc<Client>> = table().lock().unwrap().values().filter_map(|(s, _)| s.get().and_then(|r| r.as_ref().ok().cloned())).collect();
    for c in clients {
        c.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 未安装的服务器返回提示且结果被记住() {
        static FAKE: ServerSpec = ServerSpec { id: "no-such", label: "不存在的服务器", cmd: "loom-no-such-lsp-binary", args: &[], install: "别装了", exts: &["zzz"] };
        let e = get_or_start("/tmp/loom-hub-test", &FAKE).unwrap_err();
        assert!(e.contains("未安装") && e.contains("别装了"), "{e}");
        // 第二次直接取缓存的失败结果
        assert_eq!(get_or_start("/tmp/loom-hub-test", &FAKE).unwrap_err(), e);
        let st = status();
        let me = st.iter().find(|s| s.label == "不存在的服务器").unwrap();
        assert!(matches!(&me.state, ServerState::Failed(m) if m.contains("未安装")));
    }
}
