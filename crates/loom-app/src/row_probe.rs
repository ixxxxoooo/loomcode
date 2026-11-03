//! 会话流行位置探针:每帧记录「实际画出来的行」的顶边 Y(窗口坐标)。
//!
//! 贴底跟随时 gpui 列表的逻辑滚动位置不可靠(可能还是「末尾」占位),据它判断一行有没有滚出顶部会误判;
//! 改成直接看这一帧各行画在哪里:画出来的行比较顶边与视口顶边,没画出来的行按它在已画范围的前 / 后判断。
//!
//! @author ixxxxoooo

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

/// 行位置探针(界面线程独占,内部可变)。
#[derive(Default)]
pub struct RowProbe {
    tops: RefCell<HashMap<usize, f32>>,
    max_ix: Cell<Option<usize>>,
    /// 调试:各行上次画出的高度(跨帧保留,不随 `begin_frame` 清掉)。
    heights: RefCell<HashMap<usize, f32>>,
}

impl RowProbe {
    /// 新一帧开始:清掉上一帧的记录。
    pub fn begin_frame(&self) {
        self.tops.borrow_mut().clear();
        self.max_ix.set(None);
    }

    /// 记录一行的顶边。参数:`ix` 行下标;`top` 顶边 Y。
    pub fn record(&self, ix: usize, top: f32) {
        self.tops.borrow_mut().insert(ix, top);
        self.max_ix.set(Some(self.max_ix.get().map_or(ix, |m| m.max(ix))));
    }

    /// 调试:记录一行画出的高度,与上次不同(差 > 0.5px)时返回旧值。参数:`ix` 行下标;`h` 高度。
    pub fn note_height(&self, ix: usize, h: f32) -> Option<f32> {
        let old = self.heights.borrow_mut().insert(ix, h);
        old.filter(|o| (o - h).abs() > 0.5)
    }

    /// 这一帧画出来的行的顶边;没画出来返回 None。
    pub fn top_of(&self, ix: usize) -> Option<f32> {
        self.tops.borrow().get(&ix).copied()
    }

    /// 这一帧画出来的最大行下标。
    pub fn max_ix(&self) -> Option<usize> {
        self.max_ix.get()
    }

    /// 某行是否在已画范围之前(没画出来、且下标小于已画的最大下标 = 滚到了上面)。
    pub fn is_before_painted(&self, ix: usize) -> bool {
        self.top_of(ix).is_none() && self.max_ix.get().is_some_and(|m| ix < m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 已画范围之前才算滚过() {
        let p = RowProbe::default();
        assert!(!p.is_before_painted(1));
        p.record(5, 10.);
        p.record(6, 90.);
        assert!(p.is_before_painted(2));
        assert!(!p.is_before_painted(5));
        assert!(!p.is_before_painted(9));
        assert_eq!(p.max_ix(), Some(6));
        p.begin_frame();
        assert_eq!(p.max_ix(), None);
    }
}
