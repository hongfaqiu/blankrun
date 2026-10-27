//! DDC/CI 唤醒时的写入决策（纯逻辑）。只决定“写不写 D6=1、怎么写”，永远不涉及 D6=5。

/// 恢复时对一台显示器怎么做
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WakeWrite {
    /// 读得到 D6 且不是 1：写 1（失败重试）
    Retry,
    /// 读不到 D6、又不是虚拟屏：只试写一次 1
    Once,
    /// 已经是开着的，或是虚拟屏：不动
    Skip,
}

/// `d6`：读到的 VCP D6（None = 读不到）；`is_virtual` 只在读不到时才调用（查驱动描述有开销）
pub fn wake_write(d6: Option<u32>, is_virtual: impl FnOnce() -> bool) -> WakeWrite {
    match d6 {
        Some(1) => WakeWrite::Skip,
        Some(_) => WakeWrite::Retry,
        None if is_virtual() => WakeWrite::Skip,
        None => WakeWrite::Once,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn on_monitor_untouched() {
        assert_eq!(wake_write(Some(1), || panic!("不该去查虚拟屏")), WakeWrite::Skip);
    }

    #[test]
    fn off_or_standby_gets_d6_1() {
        for v in [2, 3, 4, 5] { assert_eq!(wake_write(Some(v), || false), WakeWrite::Retry); }
    }

    #[test]
    fn unreadable_physical_gets_one_try() {
        assert_eq!(wake_write(None, || false), WakeWrite::Once);
    }

    #[test]
    fn unreadable_virtual_skipped() {
        assert_eq!(wake_write(None, || true), WakeWrite::Skip);
    }
}
