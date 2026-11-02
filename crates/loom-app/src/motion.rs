//! 会话流的微动效(对标 Cursor):新行淡入并轻微上浮、展开箭头旋转。
//!
//! 动效只在「刚发生」时播放:新行出现、箭头切换都记下时间,超过时长就不再包动画。
//! 这样虚拟列表把行滚出再滚回视口(元素状态被丢弃)时不会重播,历史回放也不会满屏闪动。
//!
//! @author ixxxxoooo

use crate::row_groups::Row;
use crate::ui::icon;
use gpui_kit::assets::IconName;
use gpui_kit::*;
use loom_core::TimelineItem;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// 新行淡入时长。
const APPEAR_MS: u64 = 180;
/// 箭头旋转时长。
const CHEVRON_MS: u64 = 120;
/// 一批事件新增的行数超过这个就视为回放 / 批量载入,不做动效。
const MAX_ANIMATED_ROWS: usize = 3;

/// 动效的时间记录(挂在 `SessionModel` 上)。
#[derive(Default)]
pub struct Motion {
    appeared: HashMap<String, Instant>,
    toggled: HashMap<String, Instant>,
}

/// 显示行的稳定键:单个条目取其 id,分组取首个条目的 id,「处理中」行固定。
///
/// 参数:`row` 显示行;`items` 时间线条目。
pub fn row_key(row: &Row, items: &[TimelineItem]) -> String {
    match row {
        Row::Item(i) => items.get(*i).map_or_else(String::new, |x| x.id().to_string()),
        Row::Tools(r) | Row::Agents(r) | Row::Digest(r) => items.get(r.start).map_or_else(String::new, |x| x.id().to_string()),
        Row::Working => "working".into(),
    }
}

/// 从键集合里挑出这次新出现、需要淡入的键(新增过多视为批量载入,返回空)。
///
/// 参数:`old` 旧行的键;`new` 新行的键;`live` 回合是否在实时进行。
pub fn fresh_keys(old: &[String], new: &[String], live: bool) -> Vec<String> {
    if !live {
        return Vec::new();
    }
    let fresh: Vec<String> = new.iter().filter(|k| !k.is_empty() && !old.contains(k)).cloned().collect();
    if fresh.len() > MAX_ANIMATED_ROWS { Vec::new() } else { fresh }
}

impl Motion {
    /// 记录新出现的行。
    pub fn mark_appeared(&mut self, keys: Vec<String>) {
        let now = Instant::now();
        self.appeared.retain(|_, t| now.duration_since(*t) < Duration::from_secs(2));
        for k in keys {
            self.appeared.insert(k, now);
        }
    }

    /// 记录一次展开 / 收起。
    pub fn mark_toggled(&mut self, key: &str) {
        let now = Instant::now();
        self.toggled.retain(|_, t| now.duration_since(*t) < Duration::from_secs(2));
        self.toggled.insert(key.to_string(), now);
    }

    /// 某行是否还在淡入期(返回出现时刻,用作动画 id 的一部分)。
    fn appearing(&self, key: &str) -> Option<Instant> {
        self.appeared.get(key).copied().filter(|t| t.elapsed() < Duration::from_millis(APPEAR_MS + 120))
    }

    /// 某个箭头是否还在旋转期。
    fn rotating(&self, key: &str) -> Option<Instant> {
        self.toggled.get(key).copied().filter(|t| t.elapsed() < Duration::from_millis(CHEVRON_MS + 120))
    }

    /// 给刚出现的行包一层淡入 + 上浮 4px(用 `top` 偏移,不改变布局高度,虚拟列表无需重测)。
    ///
    /// 参数:`key` 行键;`el` 行内容。
    pub fn appear(&self, key: &str, el: Div) -> AnyElement {
        match self.appearing(key) {
            Some(t) => {
                let id = SharedString::from(format!("appear-{key}-{:?}", t));
                el.relative().with_animation(id, Animation::new(Duration::from_millis(APPEAR_MS)).with_easing(ease_out_quint()), |d, e| d.opacity(e).top(px(4. * (1. - e))))
                    .into_any_element()
            }
            None => el.into_any_element(),
        }
    }

    /// 展开箭头:收起朝右、展开朝下;刚切换时用 120ms 旋转过渡。
    ///
    /// 参数:`key` 展开状态的键;`expanded` 是否展开;`size` 图标大小;`color` 颜色。
    pub fn chevron(&self, key: &str, expanded: bool, size: f32, color: Hsla) -> AnyElement {
        let angle = |p: f32| Transformation::rotate(radians(std::f32::consts::FRAC_PI_2 * p));
        let base = icon(IconName::ChevronRight, size, color);
        match self.rotating(key) {
            Some(t) => {
                let id = SharedString::from(format!("chev-{key}-{expanded}-{:?}", t));
                base.with_animation(id, Animation::new(Duration::from_millis(CHEVRON_MS)).with_easing(ease_out_quint()), move |ic, e| {
                    ic.transform(angle(if expanded { e } else { 1. - e }))
                })
                .into_any_element()
            }
            None => base.transform(angle(if expanded { 1. } else { 0. })).into_any_element(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[::core::prelude::v1::test]
    fn 只有实时且少量新增才淡入() {
        assert_eq!(fresh_keys(&keys(&["a"]), &keys(&["a", "b"]), true), keys(&["b"]));
        assert!(fresh_keys(&keys(&["a"]), &keys(&["a", "b"]), false).is_empty());
        assert!(fresh_keys(&[], &keys(&["a", "b", "c", "d"]), true).is_empty());
        // 分组合并(键不变)不算新行
        assert!(fresh_keys(&keys(&["a"]), &keys(&["a"]), true).is_empty());
    }
}
