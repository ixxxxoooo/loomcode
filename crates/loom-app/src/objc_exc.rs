//! Objective-C 未捕获异常的记录:`NSSetUncaughtExceptionHandler` 在系统终止进程前被调用,
//! 把异常名、原因与调用栈写进崩溃记录(这类崩溃不经过 Rust 的 panic 钩子)。
//!
//! @author ixxxxoooo

#[cfg(target_os = "macos")]
mod imp {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::msg_send;
    use objc2_foundation::NSString;

    unsafe extern "C" {
        fn NSSetUncaughtExceptionHandler(handler: Option<extern "C-unwind" fn(*mut AnyObject)>);
    }

    /// 取对象的 `description` 文本。
    fn describe(obj: &AnyObject) -> String {
        // SAFETY: 任何 NSObject 都响应 `description`,返回 NSString。
        let d: Option<Retained<NSString>> = unsafe { msg_send![obj, description] };
        d.map(|s| s.to_string()).unwrap_or_default()
    }

    extern "C-unwind" fn on_exception(ex: *mut AnyObject) {
        // SAFETY: 系统传入的是有效的 NSException 指针(只在本回调内使用)。
        let Some(ex) = (unsafe { ex.as_ref() }) else { return };
        let summary = describe(ex);
        // SAFETY: NSException 响应 `callStackSymbols`,返回 NSArray。
        let stack: Option<Retained<AnyObject>> = unsafe { msg_send![ex, callStackSymbols] };
        let detail = format!("异常:{summary}\n\n调用栈:\n{}", stack.map(|s| describe(&s)).unwrap_or_default());
        loom_diag::crash::record_event("nsexception", summary, detail);
    }

    /// 安装处理器。
    pub fn install() {
        // SAFETY: 传入的函数在整个进程生命周期内有效。
        unsafe { NSSetUncaughtExceptionHandler(Some(on_exception)) };
    }
}

/// 安装 Objective-C 异常记录(非 macOS 平台什么都不做)。
pub fn install() {
    #[cfg(target_os = "macos")]
    imp::install();
}
