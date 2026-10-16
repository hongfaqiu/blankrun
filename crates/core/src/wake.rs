//! 唤醒策略：把原始输入事件过滤成"是否真的要唤醒"，用于防误触（猫踩、桌子震动）。
use crate::config::{WakeConfig, WakeMethod};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Input {
    Key { is_wake_hotkey: bool, is_emergency: bool },
    MouseMove { dx: f64, dy: f64, t_ms: u64 },
    Click,
}

#[derive(Debug, Default)]
pub struct WakePolicy {
    acc_px: f64,
    window_start_ms: Option<u64>,
}

impl WakePolicy {
    pub fn reset(&mut self) { *self = Self::default(); }

    /// 返回 true 表示应唤醒。紧急快捷键在任何模式下都生效。
    pub fn feed(&mut self, cfg: &WakeConfig, input: Input) -> bool {
        if let Input::Key { is_emergency: true, .. } = input { return true; }
        match (cfg.method, input) {
            (WakeMethod::AnyKey, Input::Key { .. }) => true,
            (WakeMethod::Hotkey, Input::Key { is_wake_hotkey, .. }) => is_wake_hotkey,
            (WakeMethod::Click, Input::Click) => true,
            (WakeMethod::MouseMove, Input::MouseMove { dx, dy, t_ms }) => {
                let start = *self.window_start_ms.get_or_insert(t_ms);
                if t_ms.saturating_sub(start) > cfg.mouse_window_ms as u64 {
                    self.window_start_ms = Some(t_ms);
                    self.acc_px = 0.0;
                }
                self.acc_px += (dx * dx + dy * dy).sqrt();
                self.acc_px >= cfg.mouse_threshold_px as f64
            }
            // TODO(M3): Password 模式需要平台层的密码输入 UI
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn jitter_filtered() {
        let cfg = WakeConfig { method: WakeMethod::MouseMove, ..Default::default() };
        let mut p = WakePolicy::default();
        for i in 0..10 { assert!(!p.feed(&cfg, Input::MouseMove { dx: 2.0, dy: 0.0, t_ms: i * 500 })); }
        let mut p = WakePolicy::default();
        assert!(!p.feed(&cfg, Input::MouseMove { dx: 100.0, dy: 0.0, t_ms: 0 }));
        assert!(p.feed(&cfg, Input::MouseMove { dx: 60.0, dy: 0.0, t_ms: 300 }));
    }
    #[test]
    fn emergency_always() {
        let cfg = WakeConfig { method: WakeMethod::ScheduleOnly, ..Default::default() };
        assert!(WakePolicy::default().feed(&cfg, Input::Key { is_wake_hotkey: false, is_emergency: true }));
    }
}
