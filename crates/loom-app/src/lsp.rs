//! 语言服务器(LSP)接入编辑器:悬停语义、跳转到定义(⌘ 悬停出下划线、⌘ 点击或 F12 跳转)、补全、诊断波浪线。
//!
//! 编辑器(`gpui-base`)已经自带 LSP 脚手架(悬停气泡、⌘ 悬停下划线、补全菜单、诊断渲染),这里只实现它的
//! `HoverProvider` / `DefinitionProvider` / `CompletionProvider` 三个接口,并把 `loom-lsp` 的连接接上去。
//!
//! 位置换算:编辑器的 `Position.character` 按字符计,LSP 默认按 UTF-16 码元计,行里有中文 / emoji 时不同,
//! 所以所有进出的位置都经过 `loom_lsp::position` 换算(悬停范围、定义目标、补全的文本编辑、诊断范围)。
//!
//! 文档同步:打开时 `didOpen`(整篇),编辑后停顿 0.25 秒 `didChange`(整篇),任何请求发出前也会先补一次同步,
//! 保存时 `didSave`,文档释放时 `didClose`。
//!
//! @author ixxxxoooo

use crate::ide::IdeView;
use anyhow::{Result, anyhow};
use gpui_kit::component::highlighter::{Diagnostic, DiagnosticSeverity};
use gpui_kit::component::input::{CompletionProvider, DefinitionProvider, HoverProvider, Position, Rope};
use gpui_kit::component::RopeExt as _;
use gpui_kit::*;
use loom_lsp::Client;
use loom_lsp::position::{chars_to_utf16, utf16_to_chars};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

/// 一个文档与语言服务器的同步状态。
pub struct DocSync {
    pub client: Arc<Client>,
    pub uri: String,
    /// (已发送的版本号, 已发送内容的哈希)
    state: Mutex<(i32, u64)>,
}

fn hash_of(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

impl DocSync {
    /// 发 `didOpen` 并返回同步句柄。
    pub fn open(client: Arc<Client>, path: &Path, text: &str) -> Arc<DocSync> {
        // 用规范化路径(解开符号链接),与工作区根的写法一致
        let canon = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let uri = loom_lsp::uri::from_path(&canon);
        let lang = loom_lsp::servers::language_id(path);
        client.notify("textDocument/didOpen", json!({ "textDocument": { "uri": uri, "languageId": lang, "version": 1, "text": text } }));
        Arc::new(DocSync { client, uri, state: Mutex::new((1, hash_of(text))) })
    }

    /// 内容与上次发送的不同才发 `didChange`;返回当前版本号。
    pub fn sync(&self, text: &str) -> i32 {
        let h = hash_of(text);
        let mut st = self.state.lock().unwrap();
        if st.1 != h {
            st.0 += 1;
            st.1 = h;
            self.client.notify("textDocument/didChange", json!({ "textDocument": { "uri": self.uri, "version": st.0 }, "contentChanges": [{ "text": text }] }));
        }
        st.0
    }

    /// 已发送的最新版本号。
    pub fn version(&self) -> i32 {
        self.state.lock().unwrap().0
    }

    /// 保存通知(带全文,服务器声明 `includeText` 时用得上)。
    pub fn did_save(&self, text: &str) {
        self.sync(text);
        self.client.notify("textDocument/didSave", json!({ "textDocument": { "uri": self.uri }, "text": text }));
    }
}

impl Drop for DocSync {
    fn drop(&mut self) {
        self.client.notify("textDocument/didClose", json!({ "textDocument": { "uri": self.uri } }));
    }
}

/// 把整篇文本按行切开(去掉行尾的 `\r`)。
pub(crate) fn split_lines(text: &str) -> Vec<&str> {
    text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect()
}

/// 编辑器偏移 → LSP 位置(UTF-16 列)。
pub(crate) fn to_lsp_pos(rope: &Rope, offset: usize, lines: &[&str]) -> Value {
    let p = rope.offset_to_position(offset);
    let col = lines.get(p.line as usize).map_or(p.character as usize, |l| chars_to_utf16(l, p.character as usize));
    json!({ "line": p.line, "character": col })
}

/// LSP 位置(UTF-16 列)→ 字符列,就地换算。
pub(crate) fn fix_pos(p: &mut lsp_types::Position, lines: &[&str]) {
    if let Some(l) = lines.get(p.line as usize) {
        p.character = utf16_to_chars(l, p.character as usize) as u32;
    }
}

pub(crate) fn fix_range(r: &mut lsp_types::Range, lines: &[&str]) {
    fix_pos(&mut r.start, lines);
    fix_pos(&mut r.end, lines);
}

/// 接到编辑器上的三个提供者(共用一个同步句柄)。
pub struct LspProvider {
    pub sync: Arc<DocSync>,
    /// 补全触发字符(服务器声明的)
    triggers: Vec<String>,
    /// 所属视图与文档路径(悬停结果写回去)
    view: WeakEntity<IdeView>,
    path: PathBuf,
    /// 定义请求按词合并的缓存(见 `DefCache`)
    defs: Arc<Mutex<DefCache>>,
}

impl LspProvider {
    pub fn new(sync: Arc<DocSync>, view: WeakEntity<IdeView>, path: PathBuf) -> Self {
        let triggers = sync.client.completion_triggers();
        Self { sync, triggers, view, path, defs: Default::default() }
    }
}

impl HoverProvider for LspProvider {
    /// 向语言服务器要悬停内容,结果写进文档由 `ide_hover` 渲染;返回 `None` 让编辑器自带的气泡不出现。
    fn hover(&self, text: &Rope, offset: usize, window: &mut Window, cx: &mut App) -> Task<Result<Option<lsp_types::Hover>>> {
        let sync = self.sync.clone();
        let (full, rope) = (text.to_string(), text.clone());
        let (view, path) = (self.view.clone(), self.path.clone());
        let mouse = window.mouse_position();
        cx.spawn(async move |cx| {
            let started = std::time::Instant::now();
            // 当前卡片已经覆盖这个位置:不重复请求
            if view.update(cx, |this, _| this.hover_blocked(&path, offset)).unwrap_or(false) {
                return Ok(None);
            }
            let (req_rope, req_full) = (rope.clone(), full.clone());
            let res = cx
                .background_executor()
                .spawn(async move {
                    let lines = split_lines(&req_full);
                    let pos = to_lsp_pos(&req_rope, offset, &lines);
                    sync.sync(&req_full);
                    let rx = sync.client.request("textDocument/hover", json!({ "textDocument": { "uri": sync.uri }, "position": pos }));
                    let v = rx.recv().await.map_err(|e| anyhow!("{e}"))?.map_err(|e| anyhow!(e))?;
                    if v.is_null() {
                        loom_diag::debug!("LSP 悬停:服务器没有结果(偏移 {offset})");
                        return Ok::<_, anyhow::Error>(None);
                    }
                    let mut hover: lsp_types::Hover = serde_json::from_value(v)?;
                    if let Some(r) = hover.range.as_mut() {
                        fix_range(r, &lines);
                    }
                    Ok(Some(hover))
                })
                .await;
                        let spent = started.elapsed();
            // 编辑器已固定等过 150ms,剩下的延迟从偏好里取;请求耗时算在里面,先到也要等够
            let wait = std::time::Duration::from_millis(u64::from(cx.update(|cx| crate::prefs(cx).editor.hover_delay_ms)).saturating_sub(150));
            if spent < wait {
                cx.background_executor().timer(wait - spent).await;
            }
            let card = match res {
                Ok(Some(h)) => crate::ide_hover::hover_markdown(&h).map(|md| {
                    // Go:去掉指向私有模块的 pkg.go.dev 链接(点开只会 404)
                    if path.extension().is_some_and(|e| e == "go") {
                        loom_lsp::golinks::strip_private_links(&md, &path, loom_lsp::golinks::private_patterns())
                    } else {
                        md
                    }
                }).filter(|md| !md.trim().is_empty()).map(|md| {
                    let range = match h.range {
                        Some(r) => rope.position_to_offset(&r.start)..rope.position_to_offset(&r.end),
                        None => rope.word_range(offset).unwrap_or(offset..offset),
                    };
                    crate::ide_hover::HoverCard { range, markdown: md.into(), anchor: (0, 0, 0), mouse }
                }),
                _ => None,
            };
            // 没有结果(途经空白 / 标点)不清掉当前卡片:收起交给粘性计时,免得鼠标还没走进卡片就消失
            if card.is_some() {
                let _ = view.update(cx, |this, cx| this.set_hover_card(&path, card, cx));
            }
            Ok(None)
        })
    }
}

/// 定义请求的应答(错误转成字符串以便克隆给多个等待者)。
type DefReply = std::result::Result<Vec<lsp_types::LocationLink>, String>;

/// 「同一个词」的定义缓存与在途请求合并。
///
/// 编辑器库在 ⌘ 悬停时,**鼠标每移动一个像素就换一次偏移、重新请求并取消上一个**,而下划线要等结果回来才出现,
/// 所以鼠标在词上慢慢移动时请求不断被取消,迟迟没有下划线(点击却能跳,因为点击只发一次)。
/// VS Code 的链接检测器按「词」去重:鼠标还在同一个词里就不再重新请求。这里同样按词(范围 + 文本长度 + 已同步版本)
/// 合并:在途请求脱离调用方的取消继续跑,同词的后续调用直接等它的结果或拿缓存;只缓存非空成功结果
/// (服务器还在加载时的空结果不能粘住)。
#[derive(Default)]
struct DefCache {
    key: Option<(std::ops::Range<usize>, usize, i32)>,
    done: Option<Vec<lsp_types::LocationLink>>,
    waiters: Vec<async_channel::Sender<DefReply>>,
}

/// 向服务器要某偏移处的定义,并把范围换算成编辑器的字符列。
async fn definition_request(sync: Arc<DocSync>, full: String, rope: Rope, offset: usize) -> Result<Vec<lsp_types::LocationLink>> {
    let lines = split_lines(&full);
    let pos = to_lsp_pos(&rope, offset, &lines);
    sync.sync(&full);
    let rx = sync.client.request("textDocument/definition", json!({ "textDocument": { "uri": sync.uri }, "position": pos }));
    let v = rx.recv().await.map_err(|e| anyhow!("{e}"))?.map_err(|e| anyhow!(e))?;
    if v.is_null() {
        return Ok(vec![]);
    }
    let resp: lsp_types::GotoDefinitionResponse = serde_json::from_value(v)?;
    let mut links = definition_links(resp);
    loom_diag::debug!("LSP 定义:偏移 {offset} → {} 个结果", links.len());
    for l in &mut links {
        if let Some(r) = l.origin_selection_range.as_mut() {
            fix_range(r, &lines);
        }
        // 目标在当前文件就用内存里的内容换算,否则读目标文件
        let same = l.target_uri.as_str() == sync.uri;
        let other = (!same).then(|| loom_lsp::uri::to_path(l.target_uri.as_str()).and_then(|p| std::fs::read_to_string(p).ok())).flatten();
        let target_lines = if same { Some(lines.clone()) } else { other.as_deref().map(split_lines) };
        if let Some(tl) = target_lines {
            fix_range(&mut l.target_range, &tl);
            fix_range(&mut l.target_selection_range, &tl);
        }
    }
    Ok(links)
}

impl DefinitionProvider for LspProvider {
    fn definitions(&self, text: &Rope, offset: usize, _window: &mut Window, cx: &mut App) -> Task<Result<Vec<lsp_types::LocationLink>>> {
        let key = (text.word_range(offset).unwrap_or(offset..offset), text.len(), self.sync.version());
        let (tx, rx) = async_channel::bounded::<DefReply>(1);
        let start = {
            let mut c = self.defs.lock().unwrap();
            if c.key.as_ref() == Some(&key) {
                if let Some(done) = &c.done {
                    return Task::ready(Ok(done.clone()));
                }
                c.waiters.push(tx);
                false
            } else {
                // 换词了:旧的等待者随发送端丢弃而收到「已取消」,旧请求的结果因键不符被丢弃
                *c = DefCache { key: Some(key.clone()), done: None, waiters: vec![tx] };
                true
            }
        };
        if start {
            let (sync, defs) = (self.sync.clone(), self.defs.clone());
            let (full, rope) = (text.to_string(), text.clone());
            cx.background_spawn(async move {
                let res: DefReply = definition_request(sync, full, rope, offset).await.map_err(|e| e.to_string());
                let mut c = defs.lock().unwrap();
                if c.key.as_ref() == Some(&key) {
                    match &res {
                        Ok(l) if !l.is_empty() => c.done = Some(l.clone()),
                        _ => c.key = None,
                    }
                    for w in c.waiters.drain(..) {
                        let _ = w.try_send(res.clone());
                    }
                }
            })
            .detach();
        }
        cx.background_spawn(async move { rx.recv().await.map_err(|e| anyhow!("{e}"))?.map_err(|e| anyhow!(e)) })
    }
}

/// 把三种定义应答统一成 `LocationLink` 列表。
pub(crate) fn definition_links(resp: lsp_types::GotoDefinitionResponse) -> Vec<lsp_types::LocationLink> {
    use lsp_types::GotoDefinitionResponse as R;
    let from_loc = |l: lsp_types::Location| lsp_types::LocationLink { origin_selection_range: None, target_uri: l.uri, target_range: l.range, target_selection_range: l.range };
    match resp {
        R::Scalar(l) => vec![from_loc(l)],
        R::Array(v) => v.into_iter().map(from_loc).collect(),
        R::Link(v) => v,
    }
}

impl CompletionProvider for LspProvider {
    fn completions(&self, text: &Rope, offset: usize, trigger: lsp_types::CompletionContext, _window: &mut Window, cx: &mut App) -> Task<Result<lsp_types::CompletionResponse>> {
        let sync = self.sync.clone();
        let (full, rope) = (text.to_string(), text.clone());
        cx.background_spawn(async move {
            let lines = split_lines(&full);
            let pos = to_lsp_pos(&rope, offset, &lines);
            sync.sync(&full);
            let ctx = serde_json::to_value(&trigger)?;
            let rx = sync.client.request("textDocument/completion", json!({ "textDocument": { "uri": sync.uri }, "position": pos, "context": ctx }));
            let v = rx.recv().await.map_err(|e| anyhow!("{e}"))?.map_err(|e| anyhow!(e))?;
            if v.is_null() {
                return Ok(lsp_types::CompletionResponse::Array(vec![]));
            }
            let mut resp: lsp_types::CompletionResponse = serde_json::from_value(v)?;
            let items = match &mut resp {
                lsp_types::CompletionResponse::Array(v) => v,
                lsp_types::CompletionResponse::List(l) => &mut l.items,
            };
            for it in items.iter_mut() {
                fix_completion_edit(it, &lines);
            }
            Ok(resp)
        })
    }

    fn is_completion_trigger(&self, _offset: usize, new_text: &str, _cx: &mut App) -> bool {
        is_trigger(new_text, &self.triggers)
    }
}

/// 输入单个标识符字符,或服务器声明的触发字符(如 `.`、`::` 的最后一个 `:`)时触发补全。
fn is_trigger(new_text: &str, triggers: &[String]) -> bool {
    let mut it = new_text.chars();
    let (Some(c), None) = (it.next(), it.next()) else { return false };
    c.is_alphanumeric() || c == '_' || triggers.iter().any(|t| t.chars().last() == Some(c))
}

/// 补全项的 `textEdit` 范围是 UTF-16 列,换算成字符列(补全都发生在光标所在行)。
fn fix_completion_edit(it: &mut lsp_types::CompletionItem, lines: &[&str]) {
    use lsp_types::CompletionTextEdit as E;
    match it.text_edit.as_mut() {
        Some(E::Edit(e)) => fix_range(&mut e.range, lines),
        Some(E::InsertAndReplace(e)) => {
            fix_range(&mut e.insert, lines);
            fix_range(&mut e.replace, lines);
        }
        None => {}
    }
}

/// 把 LSP 诊断数组转成编辑器的诊断(范围按文档内容换算成字符列)。返回 `(诊断, 错误数, 警告数)`。
pub fn convert_diagnostics(value: &Value, text: &str) -> (Vec<Diagnostic>, usize, usize) {
    let lines = split_lines(text);
    let mut out = Vec::new();
    let (mut errors, mut warnings) = (0, 0);
    for d in value.as_array().map(Vec::as_slice).unwrap_or_default() {
        let Ok(d) = serde_json::from_value::<lsp_types::Diagnostic>(d.clone()) else { continue };
        let mut r = d.range;
        fix_range(&mut r, &lines);
        let sev = d.severity.unwrap_or(lsp_types::DiagnosticSeverity::ERROR);
        match sev {
            lsp_types::DiagnosticSeverity::ERROR => errors += 1,
            lsp_types::DiagnosticSeverity::WARNING => warnings += 1,
            _ => {}
        }
        let mut diag = Diagnostic::new(Position::new(r.start.line, r.start.character)..Position::new(r.end.line, r.end.character), d.message).with_severity(DiagnosticSeverity::from(sev));
        if let Some(code) = d.code {
            diag = diag.with_code(match code {
                lsp_types::NumberOrString::Number(n) => n.to_string(),
                lsp_types::NumberOrString::String(s) => s,
            });
        }
        if let Some(src) = d.source {
            diag = diag.with_source(src);
        }
        out.push(diag);
    }
    (out, errors, warnings)
}

impl IdeView {
    /// 给已打开的文档接上语言服务器(没有对应服务器、功能关闭、未安装都静默跳过)。
    pub(crate) fn lsp_attach(&mut self, path: &Path, cx: &mut Context<Self>) {
        if !crate::prefs(cx).editor.lsp {
            return;
        }
        let Some(spec) = loom_lsp::spec_for_path(path) else { return };
        if !self.docs.contains_key(path) {
            return;
        }
        // 项目根:文件在当前项目里用项目根(语言服务器要整个工程的上下文),否则向上找工程标记文件
        let root = loom_lsp::servers::project_root(path, &self.root).to_string_lossy().to_string();
        self.lsp_start_listener(cx);
        let path = path.to_path_buf();
        cx.spawn(async move |this, cx| {
            let r = root.clone();
            let res = cx.background_spawn(async move { loom_lsp::hub::get_or_start(&r, spec) }).await;
            let _ = this.update(cx, |this, cx| this.lsp_attached(&path, res, cx));
        })
        .detach();
    }

    fn lsp_attached(&mut self, path: &Path, res: Result<Arc<Client>, String>, cx: &mut Context<Self>) {
        let client = match res {
            Ok(c) => c,
            Err(e) => {
                loom_diag::debug!("{} 未接入语言服务器:{e}", path.display());
                // 没装服务器:每种服务器每次运行只提醒一次(否则跳转 / 悬停 / 诊断悄悄失效,用户无从知道)
                if e.starts_with("未安装") {
                    static TOLD: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
                    if let Ok(mut told) = TOLD.lock() && !told.contains(&e) {
                        told.push(e.clone());
                        let cmd = e.split_once("安装:").map(|(_, c)| c.trim_end_matches(')').to_string());
                        let action = cmd.map(|c| crate::notice::NoticeAction {
                            label: "复制安装命令".into(),
                            run: std::rc::Rc::new(move |_, cx| cx.write_to_clipboard(ClipboardItem::new_string(c.clone()))),
                        });
                        crate::notice::post(cx, crate::notice::Kind::Warning, format!("{e},转到定义 / 悬停 / 诊断暂不可用"), action);
                    }
                }
                cx.notify();
                return;
            }
        };
        let weak = cx.weak_entity();
        let Some(doc) = self.docs.get_mut(path) else { return };
        if doc.lsp.is_some() {
            return;
        }
        let text = doc.editor.read(cx).value().to_string();
        let sync = DocSync::open(client.clone(), path, &text);
        let provider = Rc::new(LspProvider::new(sync.clone(), weak.clone(), path.to_path_buf()));
        doc.editor.update(cx, |ed, _| {
            let l = ed.lsp_mut();
            if client.supports("hoverProvider") {
                l.hover_provider = Some(provider.clone());
            }
            if client.supports("definitionProvider") {
                l.definition_provider = Some(provider.clone());
            }
            if client.supports("completionProvider") {
                l.completion_provider = Some(provider.clone());
            }
            // 跳转到定义:统一交给 IdeView 打开目标文件并定位(推迟到本次事件之后,避免在编辑器更新期间回读它)
            l.show_document = Some(Rc::new(move |params, window, cx| {
                let Some(target) = loom_lsp::uri::to_path(params.uri.as_str()) else { return false };
                let (line, col) = params.selection.map_or((0, 0), |s| (s.start.line, s.start.character));
                let weak = weak.clone();
                window.defer(cx, move |window, cx| {
                    let _ = weak.update(cx, |this, cx| this.open_at(target, line, col, window, cx));
                });
                true
            }));
        });
        loom_diag::info!("{} 已接入 {}(悬停 {} / 定义 {} / 补全 {})", path.display(), client.name, client.supports("hoverProvider"), client.supports("definitionProvider"), client.supports("completionProvider"));
        doc.lsp = Some(sync);
        self.refresh_outline(path, cx);
        cx.notify();
    }

    /// 打开文件并把光标放到指定位置(行、列均 0 起始、按字符)。
    pub fn open_at(&mut self, path: PathBuf, line: u32, col: u32, window: &mut Window, cx: &mut Context<Self>) {
        // 服务器返回的是规范化路径;若与某个已打开文档指向同一文件,沿用那个文档的路径,避免出现重复标签
        let canon = path.canonicalize().ok();
        let path = canon.as_ref().and_then(|c| self.docs.keys().find(|k| k.canonicalize().ok().as_ref() == Some(c)).cloned()).unwrap_or(path);
        self.open(path, false, window, cx);
        self.goto(line + 1, col + 1, window, cx);
    }

    /// 编辑后 0.25 秒把内容同步给语言服务器(防抖)。
    pub(crate) fn schedule_lsp_sync(&mut self, path: &Path, cx: &mut Context<Self>) {
        let Some(doc) = self.docs.get_mut(path).filter(|d| d.lsp.is_some()) else { return };
        doc.lsp_gen += 1;
        let (path, g) = (path.to_path_buf(), doc.lsp_gen);
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(std::time::Duration::from_millis(250)).await;
            let _ = this.update(cx, |this, cx| {
                if let Some(doc) = this.docs.get(&path).filter(|d| d.lsp_gen == g) {
                    if let Some(sync) = doc.lsp.clone() {
                        let text = doc.editor.read(cx).value().to_string();
                        sync.sync(&text);
                    }
                }
            });
        })
        .detach();
    }

    /// 启动诊断监听(只启动一次):语言服务器的 `publishDiagnostics` 经通道送回界面线程。
    fn lsp_start_listener(&mut self, cx: &mut Context<Self>) {
        if self.lsp_listening {
            return;
        }
        self.lsp_listening = true;
        let (tx, rx) = async_channel::unbounded::<(String, Option<i64>, Value)>();
        loom_lsp::hub::set_diagnostics_sink(Arc::new(move |uri, version, diags| {
            let _ = tx.try_send((uri, version, diags));
        }));
        cx.spawn(async move |this, cx| {
            while let Ok((uri, version, diags)) = rx.recv().await {
                if this.update(cx, |this, cx| this.lsp_diagnostics(&uri, version, &diags, cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    /// 把诊断铺到对应文档上。版本号落后于已同步版本(又改过)的诊断丢弃,等服务器重新发布。
    fn lsp_diagnostics(&mut self, uri: &str, version: Option<i64>, diags: &Value, cx: &mut Context<Self>) {
        let Some(doc) = self.docs.values_mut().find(|d| d.lsp.as_ref().is_some_and(|s| s.uri == uri)) else { return };
        let Some(sync) = doc.lsp.clone() else { return };
        if version.is_some_and(|v| v != i64::from(sync.version())) {
            return;
        }
        let text = doc.editor.read(cx).value().to_string();
        let (list, errors, warnings) = convert_diagnostics(diags, &text);
        doc.diag = (errors, warnings);
        let mut marks: Vec<(usize, bool)> = Vec::new();
        doc.editor.update(cx, |ed, cx| {
            let rope = ed.text().clone();
            if let Some(set) = ed.diagnostics_mut() {
                set.reset(&rope);
                set.extend(list);
                marks = set.iter().filter(|e| matches!(e.severity, DiagnosticSeverity::Error | DiagnosticSeverity::Warning)).map(|e| (rope.offset_to_position(e.range.start).line as usize, matches!(e.severity, DiagnosticSeverity::Error))).collect();
                loom_diag::debug!("诊断已铺到编辑器:{} 条", set.len());
            }
            cx.notify();
        });
        marks.dedup();
        doc.diag_marks = Rc::new(marks);
        cx.notify();
    }

    /// 状态栏用:当前文档的语言服务器摘要 `(名称与状态文字, 错误数, 警告数)`;当前文档没有接入服务器返回 `None`。
    pub fn lsp_summary(&self) -> Option<(String, usize, usize)> {
        let doc = self.tabs.active().and_then(|p| self.docs.get(p))?;
        let sync = doc.lsp.as_ref()?;
        let c = &sync.client;
        let label = match (c.is_alive(), c.activity()) {
            (false, _) => format!("{}(已退出)", c.name),
            (true, Some(work)) => format!("{} · {work}", c.name),
            (true, None) => c.name.clone(),
        };
        Some((label, doc.diag.0, doc.diag.1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 触发字符判定() {
        let trig = vec![".".to_string(), "::".to_string()];
        assert!(is_trigger("a", &trig));
        assert!(is_trigger("_", &trig));
        assert!(is_trigger(".", &trig));
        assert!(is_trigger(":", &trig));
        assert!(!is_trigger(" ", &trig));
        assert!(!is_trigger("ab", &trig));
        assert!(!is_trigger("", &trig));
        assert!(!is_trigger("(", &trig));
    }

    #[::core::prelude::v1::test]
    fn 定义应答三种形态统一() {
        use lsp_types::{GotoDefinitionResponse as R, Location, LocationLink, Position as P, Range};
        let range = Range::new(P::new(1, 2), P::new(1, 5));
        let uri: lsp_types::Uri = "file:///a.rs".parse().unwrap();
        let loc = Location::new(uri.clone(), range);
        assert_eq!(definition_links(R::Scalar(loc.clone())).len(), 1);
        let l = definition_links(R::Array(vec![loc.clone(), loc]));
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].target_selection_range, range);
        assert!(l[0].origin_selection_range.is_none());
        let link = LocationLink { origin_selection_range: Some(range), target_uri: uri, target_range: range, target_selection_range: range };
        assert_eq!(definition_links(R::Link(vec![link])).len(), 1);
    }

    #[::core::prelude::v1::test]
    fn 诊断转换含换算与计数() {
        // 第 0 行「中文 x = 1」:服务器按 UTF-16 报 x 在第 3 列(中文各占 1 个码元),字符列也是 3
        let text = "中文 x = 1\nlet 😀 y;\n";
        let v = json!([
            { "range": {"start": {"line": 0, "character": 3}, "end": {"line": 0, "character": 4}}, "severity": 1, "message": "错误一", "code": "E0425", "source": "rustc" },
            // 第 1 行 y 在 UTF-16 第 7 列(😀 占两个码元),字符列应为 6
            { "range": {"start": {"line": 1, "character": 7}, "end": {"line": 1, "character": 8}}, "severity": 2, "message": "警告" },
            { "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 1}}, "severity": 4, "message": "提示" }
        ]);
        let (list, e, w) = convert_diagnostics(&v, text);
        assert_eq!((list.len(), e, w), (3, 1, 1));
        assert_eq!(list[0].range.start, Position::new(0, 3));
        assert_eq!(list[0].code.as_deref(), Some("E0425"));
        assert_eq!(list[0].source.as_deref(), Some("rustc"));
        assert_eq!(list[1].range.start, Position::new(1, 6));
    }

    #[::core::prelude::v1::test]
    fn 补全编辑范围换算() {
        use lsp_types::{CompletionItem, CompletionTextEdit, Range, TextEdit};
        let lines = ["a😀bc"];
        let mut it = CompletionItem { text_edit: Some(CompletionTextEdit::Edit(TextEdit::new(Range::new(lsp_types::Position::new(0, 3), lsp_types::Position::new(0, 4)), "x".into()))), ..Default::default() };
        fix_completion_edit(&mut it, &lines);
        let Some(CompletionTextEdit::Edit(e)) = it.text_edit else { panic!() };
        assert_eq!((e.range.start.character, e.range.end.character), (2, 3));
    }

    #[::core::prelude::v1::test]
    fn 按行切分去回车() {
        assert_eq!(split_lines("a\r\nb\nc"), vec!["a", "b", "c"]);
    }
}
