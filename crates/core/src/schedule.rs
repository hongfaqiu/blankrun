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
}
