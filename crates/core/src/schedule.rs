//! 定时计划。不依赖 chrono：由平台层传入本地"星期 + 分钟数"，保持核心零时区依赖。
use crate::config::ScheduleConfig;

/// 0=周一 … 6=周日
pub fn parse_hhmm(s: &str) -> Option<u16> {
    let (h, m) = s.split_once(':')?;
    let (h, m): (u16, u16) = (h.trim().parse().ok()?, m.trim().parse().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

const DAYS: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];

/// 当前时刻是否在息屏时段内。跨午夜时段（23:30–07:30）按"开始那天"判断。
pub fn in_window(cfg: &ScheduleConfig, weekday: u8, minute: u16) -> bool {
    if !cfg.enabled { return false; }
    let (Some(s), Some(e)) = (parse_hhmm(&cfg.start), parse_hhmm(&cfg.end)) else { return false };
    let day_on = |d: u8| cfg.days.iter().any(|x| x.eq_ignore_ascii_case(DAYS[(d % 7) as usize]));
    if s <= e {
        day_on(weekday) && minute >= s && minute < e
    } else if minute >= s {
        day_on(weekday)
    } else {
        minute < e && day_on((weekday + 6) % 7)
    }
}

/// 距下一次计划开始还有多少分钟（当前已在时段内返回 None）。只看未来 8 天。
pub fn minutes_until_start(cfg: &ScheduleConfig, weekday: u8, minute: u16) -> Option<u32> {
    if !cfg.enabled || in_window(cfg, weekday, minute) { return None; }
    let s = parse_hhmm(&cfg.start)? as u32;
    let now = minute as u32;
    (0..8u32).find_map(|d| {
        let wd = ((weekday as u32 + d) % 7) as usize;
        let at = d * 1440 + s;
        (at > now && cfg.days.iter().any(|x| x.eq_ignore_ascii_case(DAYS[wd]))).then(|| at - now)
    })
}

/// 时段长度（分钟），跨午夜自动加一天
pub fn duration_min(cfg: &ScheduleConfig) -> Option<u32> {
    let (s, e) = (parse_hhmm(&cfg.start)? as u32, parse_hhmm(&cfg.end)? as u32);
    Some(if e > s { e - s } else { e + 1440 - s })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge { Start, End }

/// 把"是否在时段内"变成开始/结束两个边沿。时段内手动唤醒或"跳过今晚"后，本时段不再触发，直到时段结束。
#[derive(Debug, Default)]
pub struct Tracker { was_in: bool, suppressed: bool }

impl Tracker {
    pub fn update(&mut self, cfg: &ScheduleConfig, weekday: u8, minute: u16) -> Option<Edge> {
        let now_in = in_window(cfg, weekday, minute);
        let edge = match (self.was_in, now_in) {
            (false, true) if !self.suppressed => Some(Edge::Start),
            (true, false) => { let e = (!self.suppressed).then_some(Edge::End); self.suppressed = false; e }
            _ => None,
        };
        if !cfg.enabled { self.suppressed = false; } // 计划被关掉：清掉"跳过"标记
        self.was_in = now_in;
        edge
    }
    /// 本时段（或即将到来的下一个时段）不再自动息屏
    pub fn suppress(&mut self) { self.suppressed = true; }
    pub fn suppressed(&self) -> bool { self.suppressed }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overnight() {
        let mut c = ScheduleConfig::default();
        c.enabled = true;
        c.days = vec!["fri".into()];
        assert!(in_window(&c, 4, 23 * 60 + 45)); // 周五 23:45
        assert!(in_window(&c, 5, 6 * 60)); // 周六 06:00 属于周五晚的时段
        assert!(!in_window(&c, 5, 23 * 60 + 45));
        assert!(!in_window(&c, 4, 12 * 60));
    }

    #[test]
    fn until_and_tracker() {
        let mut c = ScheduleConfig::default();
        c.enabled = true; // 每天 23:30–07:30
        assert_eq!(minutes_until_start(&c, 3, 20 * 60 + 43), Some(167)); // 2 小时 47 分
        assert_eq!(minutes_until_start(&c, 3, 23 * 60 + 40), None);
        assert_eq!(duration_min(&c), Some(480));
        c.days = vec!["mon".into()];
        assert_eq!(minutes_until_start(&c, 1, 8 * 60), Some(6 * 1440 + 23 * 60 + 30 - 8 * 60)); // 周二 8 点 → 下周一 23:30

        let mut c = ScheduleConfig::default();
        c.enabled = true;
        let mut t = Tracker::default();
        assert_eq!(t.update(&c, 0, 23 * 60), None);
        assert_eq!(t.update(&c, 0, 23 * 60 + 30), Some(Edge::Start));
        assert_eq!(t.update(&c, 0, 23 * 60 + 31), None);
        t.suppress(); // 手动唤醒
        assert_eq!(t.update(&c, 1, 7 * 60 + 30), None); // 结束时不再发 End
        assert!(!t.suppressed());
        t.suppress(); // 跳过今晚
        assert_eq!(t.update(&c, 1, 23 * 60 + 30), None);
        assert_eq!(t.update(&c, 2, 7 * 60 + 30), None);
        assert_eq!(t.update(&c, 2, 23 * 60 + 30), Some(Edge::Start));
    }
}
