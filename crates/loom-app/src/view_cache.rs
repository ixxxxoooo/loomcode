//! 已打开会话视图的淘汰策略:每个会话对应一个 claude 子进程,不能无限累积。
//!
//! 纯函数,不依赖界面,方便单元测试。
//!
//! @author ixxxxoooo

/// 同时保留的已打开会话数上限(含当前会话)。
pub const MAX_OPEN_VIEWS: usize = 6;

/// 记录一次使用:把 `id` 移到队尾(最近使用)。
///
/// 参数:`order` 使用顺序,队首最久未用;`id` 刚被使用的会话。
pub fn touch(order: &mut Vec<String>, id: &str) {
    order.retain(|x| x != id);
    order.push(id.to_string());
}

/// 选出需要淘汰的会话,按最久未用优先。
///
/// 参数:
/// - `order` 使用顺序(队首最久未用);
/// - `active` 当前会话,永不淘汰;
/// - `busy` 判断会话是否忙碌(运行中、等待用户、有草稿、尚无内容),忙碌的永不淘汰;
/// - `max` 保留上限。
///
/// 返回:要淘汰的会话 id。忙碌会话太多时可能少于需要的数量,宁可超限也不打断用户。
pub fn pick_evictions(order: &[String], active: Option<&str>, busy: impl Fn(&str) -> bool, max: usize) -> Vec<String> {
    let excess = order.len().saturating_sub(max);
    order.iter().filter(|id| Some(id.as_str()) != active && !busy(id)).take(excess).cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn 未超限不淘汰() {
        assert!(pick_evictions(&ids(&["a", "b"]), Some("b"), |_| false, 6).is_empty());
    }

    #[test]
    fn 超限时淘汰最久未用() {
        let order = ids(&["a", "b", "c", "d"]);
        assert_eq!(pick_evictions(&order, Some("d"), |_| false, 2), ids(&["a", "b"]));
    }

    #[test]
    fn 忙碌与当前会话不被淘汰() {
        let order = ids(&["a", "b", "c", "d"]);
        // a 在运行、d 是当前会话:只能淘汰 b、c 中的前面一个
        assert_eq!(pick_evictions(&order, Some("d"), |id| id == "a", 3), ids(&["b"]));
        // 全都忙碌时宁可超限
        assert!(pick_evictions(&order, Some("d"), |_| true, 1).is_empty());
    }

    #[test]
    fn 使用后移到队尾() {
        let mut order = ids(&["a", "b", "c"]);
        touch(&mut order, "a");
        assert_eq!(order, ids(&["b", "c", "a"]));
        touch(&mut order, "z");
        assert_eq!(order, ids(&["b", "c", "a", "z"]));
    }
}
