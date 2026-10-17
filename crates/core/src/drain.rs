//! 唤醒后的「排空」：按住唤醒快捷键唤醒时，组合键还按着，如果马上卸掉输入钩子，
//! 按键的自动重复和抬起会漏给前台程序（一串 WWWW）。唤醒后先继续吞键盘事件，
//! 直到唤醒时按着的键（以及排空期间新按下的键）全部抬起、再过一小段宽限期；
//! 最长不超过 CAP_MS，保证键盘绝不会一直被拦着。鼠标不受影响。
use std::collections::BTreeSet;

/// 全部抬起后再吞这么久（防抖、吸收迟到的重复）
pub const GRACE_MS: u64 = 300;
/// 最长排空时间：到点无条件放行
pub const CAP_MS: u64 = 2000;

#[derive(Debug, Clone)]
pub struct Drain {
    held: BTreeSet<u32>,
    start: u64,
    /// 最近一次「全部抬起」的时间
    clear_at: Option<u64>,
}

impl Drain {
    /// `held`：唤醒那一刻按着的键（平台自己的键码）
    pub fn new(held: impl IntoIterator<Item = u32>, now: u64) -> Self {
        let held: BTreeSet<u32> = held.into_iter().collect();
        let clear_at = held.is_empty().then_some(now);
        Self { held, start: now, clear_at }
    }

    /// 排空期间的一个键盘事件；返回 true = 吞掉
    pub fn key(&mut self, code: u32, down: bool, now: u64) -> bool {
        if self.done(now) { return false; }
        if down {
            // 组合键的自动重复，或者排空期间新按的键：吞掉并记下，等它抬起
            self.held.insert(code);
            self.clear_at = None;
            true
        } else if self.held.remove(&code) {
            if self.held.is_empty() { self.clear_at = Some(now); }
            true
        } else {
            // 唤醒前就没在我们这儿按下的键：抬起放行，免得别的程序以为它一直按着
            false
        }
    }

    /// 可以卸掉钩子了
    pub fn done(&self, now: u64) -> bool {
        now.saturating_sub(self.start) >= CAP_MS || self.clear_at.is_some_and(|t| now.saturating_sub(t) >= GRACE_MS)
    }

    /// 唤醒时根本没有按着的键：不用排空
    pub fn needed(&self) -> bool { !self.held.is_empty() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swallows_repeats_and_ups_until_released_plus_grace() {
        // Ctrl(17) + Alt(18) + W(87) 还按着
        let mut d = Drain::new([17, 18, 87], 0);
        assert!(d.needed());
        assert!(d.key(87, true, 30)); // 自动重复
        assert!(d.key(87, true, 60));
        assert!(d.key(87, false, 100));
        assert!(d.key(18, false, 120));
        assert!(!d.done(150));
        assert!(d.key(17, false, 200)); // 最后一个抬起，开始宽限
        assert!(!d.done(400));
        assert!(d.done(500)); // 200 + 300
        assert!(!d.key(65, true, 510)); // 排空结束：正常输入
    }

    #[test]
    fn new_key_during_grace_is_tracked() {
        let mut d = Drain::new([87], 0);
        assert!(d.key(87, false, 50));
        assert!(d.key(65, true, 100)); // 宽限期里又按了一个键
        assert!(!d.done(400)); // 还按着，不结束
        assert!(d.key(65, false, 450));
        assert!(!d.done(700));
        assert!(d.done(750));
    }

    #[test]
    fn untracked_key_up_passes() {
        let mut d = Drain::new([87], 0);
        assert!(!d.key(16, false, 10)); // Shift 是钩子装上之前按下的
    }

    #[test]
    fn cap_never_blocks_forever() {
        let mut d = Drain::new([87], 0);
        for t in (0..CAP_MS).step_by(30) { assert!(d.key(87, true, t)); } // 一直按着不放
        assert!(d.done(CAP_MS));
        assert!(!d.key(87, true, CAP_MS + 10));
    }

    #[test]
    fn nothing_held_ends_after_grace() {
        let d = Drain::new([], 0);
        assert!(!d.needed());
        assert!(d.done(GRACE_MS));
    }
}
