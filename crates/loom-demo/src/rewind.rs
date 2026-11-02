//! 演示后端的「回退文件」:不碰磁盘,给出一份示例预览,执行时报告成功。
//!
//! @author ixxxxoooo

use loom_core::RewindOutcome;

/// 演示用的用户消息 uuid(由条目 id 派生,保证同一条消息稳定)。
pub fn uuid_for(item_id: &str) -> String {
    format!("demo-{item_id}")
}

/// 模拟一次回退请求。参数:`uuid` 目标消息,`dry_run` 是否预览。返回:结果。
pub fn outcome(uuid: &str, dry_run: bool) -> RewindOutcome {
    if !dry_run {
        return RewindOutcome { uuid: uuid.into(), can_rewind: true, ..Default::default() };
    }
    let root = crate::workspace::default_root().join("router-demo");
    let files = ["README.md", "src/router.rs"].iter().map(|f| root.join(f).to_string_lossy().into_owned()).collect();
    RewindOutcome { uuid: uuid.into(), dry_run: true, can_rewind: true, files, insertions: 3, deletions: 12, error: None }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 预览给出示例文件_执行报告成功() {
        let p = outcome("demo-u1", true);
        assert!(p.has_changes() && p.files.len() == 2);
        let d = outcome("demo-u1", false);
        assert!(d.can_rewind && !d.dry_run && d.error.is_none());
    }
}
