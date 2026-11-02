//! 一个语言服务器连接:读线程分发响应 / 通知 / 服务端请求,写线程按序写出,请求按 id 配对。
//!
//! 传输与进程分开:`from_io` 接任意读写流(测试里用 UnixStream 对),`spawn` 在其上起真实子进程。

use crate::framing;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// 请求的结果:`Ok(result)` 或 `Err(错误说明)`。
pub type Reply = Result<Value, String>;

/// 收到服务器通知时的回调(在读线程里调用,别做重活)。
pub type NotifyFn = Arc<dyn Fn(&str, Value) + Send + Sync>;

/// 进行中的后台工作:令牌 → (标题, 进度说明)
type Activity = Arc<Mutex<HashMap<String, (String, String)>>>;

type Pending = Arc<Mutex<HashMap<i64, async_channel::Sender<Reply>>>>;

pub struct Client {
    /// 界面显示名
    pub name: String,
    tx: mpsc::Sender<Vec<u8>>,
    pending: Pending,
    next: AtomicI64,
    alive: Arc<AtomicBool>,
    child: Mutex<Option<Child>>,
    /// 服务器在 `initialize` 应答里声明的能力
    caps: Mutex<Value>,
    /// 进行中的后台工作(`$/progress`):令牌 → 说明,状态栏显示「索引中…」
    activity: Activity,
    /// 服务器标准错误的末尾几行(启动失败时用来说明原因)
    stderr_tail: Arc<Mutex<Vec<String>>>,
}

impl Client {
    /// 用已有的读写流建立连接(不负责握手)。
    pub fn from_io<R, W>(name: &str, reader: R, mut writer: W, on_notify: NotifyFn) -> Arc<Client>
    where
        R: Read + Send + 'static,
        W: Write + Send + 'static,
    {
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        let pending: Pending = Arc::default();
        let alive = Arc::new(AtomicBool::new(true));
        let activity: Activity = Arc::default();
        let client = Arc::new(Client { name: name.to_string(), tx: tx.clone(), pending: pending.clone(), next: AtomicI64::new(1), alive: alive.clone(), child: Mutex::new(None), caps: Mutex::new(Value::Null), activity: activity.clone(), stderr_tail: Arc::default() });
        // 写线程:按序写,通道关闭(客户端释放)后自然结束
        std::thread::Builder::new()
            .name("loom-lsp-write".into())
            .spawn(move || {
                while let Ok(buf) = rx.recv() {
                    if writer.write_all(&buf).and_then(|()| writer.flush()).is_err() {
                        break;
                    }
                }
            })
            .ok();
        // 读线程:不持有 Client(避免循环引用),只持有分发需要的几样
        let reader_name = name.to_string();
        std::thread::Builder::new()
            .name("loom-lsp-read".into())
            .spawn(move || {
                let mut r = BufReader::new(reader);
                loop {
                    match framing::read_message(&mut r) {
                        Ok(Some(msg)) => dispatch(msg, &pending, &tx, &on_notify, &activity),
                        Ok(None) => break,
                        Err(e) => {
                            loom_diag::warn!("语言服务器 {reader_name} 的输出无法解析:{e}");
                            break;
                        }
                    }
                }
                alive.store(false, Ordering::SeqCst);
                // 连接断了:所有等待中的请求立刻失败,不让界面一直转
                let drained: Vec<_> = pending.lock().map(|mut p| p.drain().collect()).unwrap_or_default();
                for (_, s) in drained {
                    let _ = s.try_send(Err("语言服务器已退出".into()));
                }
                loom_diag::info!("语言服务器 {reader_name} 连接已关闭");
            })
            .ok();
        client
    }

    /// 启动语言服务器子进程并完成 `initialize` 握手(阻塞,请在后台线程调用)。
    pub fn spawn(name: &str, exe: &Path, args: &[&str], root: &Path, init_options: Option<Value>, on_notify: NotifyFn) -> Result<Arc<Client>, String> {
        let mut cmd = Command::new(exe);
        cmd.args(args).current_dir(root).env("PATH", crate::servers::child_path()).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        for (k, v) in crate::servers::extra_env(if name == "gopls" { "gopls" } else { "" }) {
            cmd.env(k, v);
        }
        // 与终端同理:fork 期间不能有别的线程调用 libc 时间函数
        let mut child = {
            let _g = loom_diag::fork_guard();
            cmd.spawn().map_err(|e| format!("无法启动 {name}:{e}"))?
        };
        let stdin = child.stdin.take().ok_or("无法获得服务器的标准输入")?;
        let stdout = child.stdout.take().ok_or("无法获得服务器的标准输出")?;
        let client = Client::from_io(name, stdout, stdin, on_notify);
        // 读标准错误:留末尾 8 行,启动就退出时拿来解释原因
        if let Some(err) = child.stderr.take() {
            let tail = client.stderr_tail.clone();
            let n = name.to_string();
            std::thread::Builder::new()
                .name("loom-lsp-stderr".into())
                .spawn(move || {
                    use std::io::BufRead;
                    for line in BufReader::new(err).lines().map_while(Result::ok) {
                        loom_diag::debug!("[{n}] {line}");
                        if let Ok(mut t) = tail.lock() {
                            t.push(line);
                            if t.len() > 8 {
                                t.remove(0);
                            }
                        }
                    }
                })
                .ok();
        }
        *client.child.lock().unwrap() = Some(child);
        client.initialize(root, init_options).map_err(|e| {
            // 退出得太快,标准错误可能还没读完:稍等一下再取
            std::thread::sleep(Duration::from_millis(100));
            let tail = client.stderr_tail.lock().map(|t| t.iter().rev().take(2).rev().cloned().collect::<Vec<_>>().join(" / ")).unwrap_or_default();
            if tail.is_empty() { e } else { format!("{e}({tail})") }
        })?;
        Ok(client)
    }

    /// `initialize` + `initialized` 握手。
    pub fn initialize(&self, root: &Path, init_options: Option<Value>) -> Result<(), String> {
        let uri = crate::uri::from_path(root);
        let folder = root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "workspace".into());
        let params = json!({
            "processId": std::process::id(),
            "clientInfo": { "name": "LoomCode" },
            "rootUri": uri,
            "workspaceFolders": [{ "uri": uri, "name": folder }],
            "initializationOptions": init_options,
            "capabilities": {
                "general": { "positionEncodings": ["utf-16"] },
                "workspace": { "configuration": true, "workspaceFolders": true },
                "window": { "workDoneProgress": true },
                "textDocument": {
                    "synchronization": { "didSave": true, "dynamicRegistration": false },
                    "hover": { "contentFormat": ["markdown", "plaintext"] },
                    "definition": { "linkSupport": true },
                    "completion": {
                        "completionItem": { "snippetSupport": false, "documentationFormat": ["markdown", "plaintext"] },
                        "contextSupport": true
                    },
                    "publishDiagnostics": { "relatedInformation": true, "versionSupport": true }
                }
            }
        });
        let result = self.request_blocking("initialize", params, Duration::from_secs(60))?;
        *self.caps.lock().unwrap() = result.get("capabilities").cloned().unwrap_or(Value::Null);
        self.notify("initialized", json!({}));
        Ok(())
    }

    /// 服务器声明的某项能力(如 `hoverProvider`)是否可用(存在且不是 `false`)。
    pub fn supports(&self, capability: &str) -> bool {
        let caps = self.caps.lock().unwrap();
        match caps.get(capability) {
            None | Some(Value::Null) | Some(Value::Bool(false)) => false,
            Some(_) => true,
        }
    }

    /// 补全触发字符。
    pub fn completion_triggers(&self) -> Vec<String> {
        let caps = self.caps.lock().unwrap();
        caps.pointer("/completionProvider/triggerCharacters")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
            .unwrap_or_default()
    }

    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    /// 正在进行的后台工作说明(索引、加载…),没有则 `None`。
    pub fn activity(&self) -> Option<String> {
        let a = self.activity.lock().ok()?;
        a.values().next().map(|(t, m)| if m.is_empty() { t.clone() } else if t.is_empty() { m.clone() } else { format!("{t} {m}") })
    }

    /// 发请求,返回一次性的应答通道(在异步任务里 `recv().await`)。
    pub fn request(&self, method: &str, params: Value) -> async_channel::Receiver<Reply> {
        let (s, r) = async_channel::bounded(1);
        if !self.is_alive() {
            let _ = s.try_send(Err("语言服务器已退出".into()));
            return r;
        }
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        self.pending.lock().unwrap().insert(id, s.clone());
        let msg = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        if self.tx.send(framing::encode(&msg)).is_err() {
            self.pending.lock().unwrap().remove(&id);
            let _ = s.try_send(Err("语言服务器已退出".into()));
        }
        r
    }

    /// 阻塞式请求(带超时);只用于握手这类必须同步的场合。
    pub fn request_blocking(&self, method: &str, params: Value, timeout: Duration) -> Reply {
        let r = self.request(method, params);
        let start = Instant::now();
        loop {
            match r.try_recv() {
                Ok(v) => return v,
                Err(async_channel::TryRecvError::Closed) => return Err("连接已关闭".into()),
                Err(async_channel::TryRecvError::Empty) => {}
            }
            if start.elapsed() > timeout {
                return Err(format!("等待 {method} 应答超时"));
            }
            std::thread::sleep(Duration::from_millis(15));
        }
    }

    /// 发通知(无应答)。
    pub fn notify(&self, method: &str, params: Value) {
        let msg = json!({ "jsonrpc": "2.0", "method": method, "params": params });
        let _ = self.tx.send(framing::encode(&msg));
    }

    /// 立刻结束子进程(不等待),退出应用时用。
    pub fn kill(&self) {
        if let Some(mut c) = self.child.lock().ok().and_then(|mut c| c.take()) {
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    /// 礼貌退出:`shutdown` + `exit`,再确保子进程结束。
    pub fn shutdown(&self) {
        if self.is_alive() {
            let _ = self.request_blocking("shutdown", Value::Null, Duration::from_secs(2));
            self.notify("exit", Value::Null);
        }
        if let Some(mut c) = self.child.lock().ok().and_then(|mut c| c.take()) {
            // 给它一点时间自己退出,否则强杀,避免遗留孤儿进程
            for _ in 0..20 {
                if matches!(c.try_wait(), Ok(Some(_))) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Client({})", self.name)
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        if let Some(mut c) = self.child.lock().ok().and_then(|mut c| c.take()) {
            // 通道随之关闭,服务器读到 EOF 通常会自行退出;这里兜底强杀
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

/// 分发一条来自服务器的消息。
fn dispatch(msg: Value, pending: &Pending, tx: &mpsc::Sender<Vec<u8>>, on_notify: &NotifyFn, activity: &Activity) {
    let method = msg.get("method").and_then(Value::as_str).map(str::to_string);
    let id = msg.get("id").cloned();
    match (method, id) {
        // 服务端发来的请求:必须应答,否则部分服务器会一直等
        (Some(method), Some(id)) => {
            let result = match method.as_str() {
                // 配置请求:按条目数返回等长的 null 数组
                "workspace/configuration" => {
                    let n = msg.pointer("/params/items").and_then(Value::as_array).map_or(0, Vec::len);
                    Value::Array(vec![Value::Null; n])
                }
                _ => Value::Null,
            };
            let reply = json!({ "jsonrpc": "2.0", "id": id, "result": result });
            let _ = tx.send(framing::encode(&reply));
        }
        // 通知
        (Some(method), None) => {
            let params = msg.get("params").cloned().unwrap_or(Value::Null);
            if method == "$/progress" {
                track_progress(&params, activity);
            }
            on_notify(&method, params);
        }
        // 应答
        (None, Some(id)) => {
            let Some(id) = id.as_i64() else { return };
            let sender = pending.lock().ok().and_then(|mut p| p.remove(&id));
            if let Some(s) = sender {
                let reply = match msg.get("error") {
                    Some(e) if !e.is_null() => Err(e.get("message").and_then(Value::as_str).unwrap_or("服务器返回错误").to_string()),
                    _ => Ok(msg.get("result").cloned().unwrap_or(Value::Null)),
                };
                let _ = s.try_send(reply);
            }
        }
        (None, None) => {}
    }
}

/// 记录 `$/progress` 的开始 / 结束,供状态栏显示。
fn track_progress(params: &Value, activity: &Activity) {
    let token = match params.get("token") {
        Some(Value::String(s)) => s.clone(),
        Some(v) => v.to_string(),
        None => return,
    };
    let value = params.get("value").cloned().unwrap_or(Value::Null);
    let Ok(mut a) = activity.lock() else { return };
    let text = |k: &str| value.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    match value.get("kind").and_then(Value::as_str) {
        Some("begin") => {
            a.insert(token, (text("title"), text("message")));
        }
        // report 里通常不带标题:沿用开始时的标题,只更新进度说明
        Some("report") => {
            let entry = a.entry(token).or_default();
            if !text("title").is_empty() {
                entry.0 = text("title");
            }
            entry.1 = text("message");
        }
        Some("end") => {
            a.remove(&token);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufReader;
    use std::os::unix::net::UnixStream;

    /// 起一个假服务器:读请求,按方法名回应;并主动发一条通知与一条服务端请求。
    fn fake_server(stream: UnixStream) {
        std::thread::spawn(move || {
            let mut w = stream.try_clone().unwrap();
            let mut r = BufReader::new(stream);
            while let Ok(Some(msg)) = framing::read_message(&mut r) {
                let Some(id) = msg.get("id").cloned() else { continue };
                match msg["method"].as_str() {
                    Some("initialize") => {
                        framing::write_message(&mut w, &json!({"jsonrpc":"2.0","id":id,"result":{"capabilities":{"hoverProvider":true,"definitionProvider":false,"completionProvider":{"triggerCharacters":[".",":"]}}}})).unwrap();
                    }
                    Some("echo") => {
                        // 先发一条通知,再发服务端请求,最后才回应
                        framing::write_message(&mut w, &json!({"jsonrpc":"2.0","method":"note","params":{"n":1}})).unwrap();
                        framing::write_message(&mut w, &json!({"jsonrpc":"2.0","id":900,"method":"workspace/configuration","params":{"items":[{},{}]}})).unwrap();
                        framing::write_message(&mut w, &json!({"jsonrpc":"2.0","id":id,"result":msg["params"]})).unwrap();
                    }
                    Some("boom") => {
                        framing::write_message(&mut w, &json!({"jsonrpc":"2.0","id":id,"error":{"code":-32603,"message":"炸了"}})).unwrap();
                    }
                    Some("die") => return,
                    _ => {}
                }
            }
        });
    }

    fn connect() -> (Arc<Client>, Arc<Mutex<Vec<(String, Value)>>>) {
        let (a, b) = UnixStream::pair().unwrap();
        fake_server(b);
        let notes: Arc<Mutex<Vec<(String, Value)>>> = Arc::default();
        let n2 = notes.clone();
        let c = Client::from_io("fake", a.try_clone().unwrap(), a, Arc::new(move |m, p| n2.lock().unwrap().push((m.to_string(), p))));
        (c, notes)
    }

    #[test]
    fn 握手读取能力() {
        let (c, _) = connect();
        c.initialize(Path::new("/tmp"), None).unwrap();
        assert!(c.supports("hoverProvider"));
        assert!(!c.supports("definitionProvider"));
        assert!(!c.supports("renameProvider"));
        assert_eq!(c.completion_triggers(), vec![".", ":"]);
    }

    #[test]
    fn 请求配对_通知分发_服务端请求被应答() {
        let (c, notes) = connect();
        let v = c.request_blocking("echo", json!({"x": 7}), Duration::from_secs(3)).unwrap();
        assert_eq!(v, json!({"x": 7}));
        assert_eq!(notes.lock().unwrap().first().map(|n| n.0.clone()), Some("note".to_string()));
    }

    #[test]
    fn 错误应答变成_err() {
        let (c, _) = connect();
        let e = c.request_blocking("boom", Value::Null, Duration::from_secs(3)).unwrap_err();
        assert_eq!(e, "炸了");
    }

    #[test]
    fn 服务器退出后等待中的请求立刻失败() {
        let (c, _) = connect();
        let e = c.request_blocking("die", Value::Null, Duration::from_secs(3)).unwrap_err();
        assert!(e.contains("退出") || e.contains("关闭"), "{e}");
        assert!(!c.is_alive());
        // 之后的请求也立即失败
        assert!(c.request_blocking("echo", Value::Null, Duration::from_secs(1)).is_err());
    }

    #[test]
    fn 进度通知用于状态栏() {
        let a: Activity = Arc::default();
        track_progress(&json!({"token":"t","value":{"kind":"begin","title":"Indexing"}}), &a);
        assert_eq!(a.lock().unwrap().get("t"), Some(&("Indexing".to_string(), String::new())));
        track_progress(&json!({"token":"t","value":{"kind":"report","message":"3/10"}}), &a);
        assert_eq!(a.lock().unwrap().get("t"), Some(&("Indexing".to_string(), "3/10".to_string())));
        track_progress(&json!({"token":"t","value":{"kind":"end"}}), &a);
        assert!(a.lock().unwrap().is_empty());
    }
}
