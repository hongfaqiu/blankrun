//! 定时计划。不依赖 chrono：由平台层传入本地"星期 + 分钟数"，保持核心零时区依赖。
use crate::config::ScheduleConfig;
#[cfg(test)]
use crate::config::ScheduleEntry;

/// 0=周一 … 6=周日
pub fn parse_hhmm(s: &str) -> Option<u16> {
    let (h, m) = s.split_once(':')?;
    let (h, m): (u16, u16) = (h.trim().parse().ok()?, m.trim().parse().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

use crate::config::ALL_DAYS as DAYS;

/// 一周有多少分钟。时段用「一周内的分钟数」表示：周一 00:00 = 0
pub const WEEK: u32 = 7 * 1440;

/// 一段息屏时段（多组定时合并后的结果）。start < WEEK；end 可能超过 WEEK（周日晚到周一早）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span { pub start: u32, pub end: u32 }

impl Span {
    /// 时长（分钟）；合并后的时段长度总大于 0
    pub fn len(&self) -> u32 { self.end - self.start }
    pub fn is_empty(&self) -> bool { self.end <= self.start }
    /// 开始那天（0=周一）
    pub fn weekday(&self) -> u8 { (self.start / 1440) as u8 }
    /// 开始、结束的「一天中的分钟数」
    pub fn start_hm(&self) -> u16 { (self.start % 1440) as u16 }
    pub fn end_hm(&self) -> u16 { (self.end % 1440) as u16 }
    /// now（一周内的分钟数）在不在这段里；处理跨周
    fn offset(&self, now: u32) -> Option<u32> {
        let off = (now + WEEK - self.start % WEEK) % WEEK;
        (off < self.len()).then_some(off)
    }
}

/// 一组定时的开始分钟和长度；开始等于结束时长度为 0，不生效
fn entry_range(start: &str, end: &str) -> Option<(u32, u32)> {
    let (s, e) = (parse_hhmm(start)? as u32, parse_hhmm(end)? as u32);
    let len = if e >= s { e - s } else { e + 1440 - s };
    (len > 0).then_some((s, len))
}

/// 所有启用的定时在一周里的时段，重叠或首尾相接的合并成一段，按开始时间排好。
/// 跨午夜的时段算在开始那天：周五 23:30–07:30 是周五 23:30 到周六 07:30。
pub fn spans(cfg: &ScheduleConfig) -> Vec<Span> {
    if !cfg.enabled { return vec![]; }
    let mut v: Vec<Span> = Vec::new();
    for e in cfg.entries.iter().filter(|e| e.enabled) {
        let Some((s, len)) = entry_range(&e.start, &e.end) else { continue };
        for (d, name) in DAYS.iter().enumerate() {
            if e.days.iter().any(|x| x.eq_ignore_ascii_case(name)) {
                let start = d as u32 * 1440 + s;
                v.push(Span { start, end: start + len });
            }
        }
    }
    v.sort_by_key(|s| s.start);
    let mut out: Vec<Span> = Vec::with_capacity(v.len());
    for s in v {
        match out.last_mut() {
            Some(l) if s.start <= l.end => l.end = l.end.max(s.end),
            _ => out.push(s),
        }
    }
    // 跨周：最后一段（周日晚）接上了第一段（周一）
    while out.len() > 1 && out[out.len() - 1].end >= out[0].start + WEEK {
        let first = out.remove(0);
        let last = out.last_mut().unwrap();
        last.end = last.end.max(first.end + WEEK);
    }
    if let Some(l) = out.last_mut() { l.end = l.end.min(l.start + WEEK); } // 一周全满
    out
}

fn week_min(weekday: u8, minute: u16) -> u32 { (weekday as u32 % 7) * 1440 + minute as u32 % 1440 }

/// 正在进行的时段
pub fn current(cfg: &ScheduleConfig, weekday: u8, minute: u16) -> Option<Span> {
    let now = week_min(weekday, minute);
    spans(cfg).into_iter().find(|s| s.offset(now).is_some())
}

/// 正在进行的时段（距开始 0 分钟），或下一个时段和距它开始的分钟数
pub fn focus(cfg: &ScheduleConfig, weekday: u8, minute: u16) -> Option<(Span, u32)> {
    let now = week_min(weekday, minute);
    let v = spans(cfg);
    if let Some(s) = v.iter().find(|s| s.offset(now).is_some()) { return Some((*s, 0)); }
    v.into_iter().map(|s| ((s.start + WEEK - now) % WEEK, s)).min_by_key(|(d, _)| *d).map(|(d, s)| (s, d))
}

/// 当前时刻是否在息屏时段内
pub fn in_window(cfg: &ScheduleConfig, weekday: u8, minute: u16) -> bool { current(cfg, weekday, minute).is_some() }

/// 距下一次计划开始还有多少分钟（当前已在时段内返回 None）
pub fn minutes_until_start(cfg: &ScheduleConfig, weekday: u8, minute: u16) -> Option<u32> {
    focus(cfg, weekday, minute).and_then(|(_, d)| (d > 0).then_some(d))
}

/// 距本时段（正在时段内）或下一个时段结束还有多少分钟；用来给「跳过今晚」定一个过期时间
pub fn minutes_until_end(cfg: &ScheduleConfig, weekday: u8, minute: u16) -> Option<u32> {
    let now = week_min(weekday, minute);
    let (s, d) = focus(cfg, weekday, minute)?;
    Some(if d == 0 { s.len() - s.offset(now).unwrap_or(0) } else { d + s.len() })
}

/// 当前或下一个时段的长度（分钟）
pub fn duration_min(cfg: &ScheduleConfig, weekday: u8, minute: u16) -> Option<u32> {
    focus(cfg, weekday, minute).map(|(s, _)| s.len())
}

/// 这段时间由哪组定时开始（取第一个有名称的）；给界面显示名称用
pub fn label_of<'a>(cfg: &'a ScheduleConfig, span: &Span) -> Option<&'a str> {
    let (wd, hm) = (span.weekday() as usize, span.start_hm());
    cfg.entries.iter().filter(|e| e.enabled && !e.label.is_empty()).find(|e| {
        parse_hhmm(&e.start) == Some(hm) && e.days.iter().any(|x| x.eq_ignore_ascii_case(DAYS[wd]))
    }).map(|e| e.label.as_str())
}

/// 某一天（0=周一）里哪些分钟在时段内：[(开始, 结束)]，结束最大 1440。画时间轴用
pub fn day_segments(cfg: &ScheduleConfig, weekday: u8) -> Vec<(u16, u16)> {
    let d0 = week_min(weekday, 0) as i64;
    let mut out = Vec::new();
    for s in spans(cfg) {
        // 周日晚开始的时段会跨到下周一：也按往前挪一周看一次
        for shift in [0i64, WEEK as i64] {
            let (a, b) = ((s.start as i64 - shift).max(d0), (s.end as i64 - shift).min(d0 + 1440));
            if a < b { out.push(((a - d0) as u16, (b - d0) as u16)); }
        }
    }
    out.sort();
    out
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
    /// 撤销「跳过今晚」：如果现在正在时段内，下一次 update 会重新发出 Start
    pub fn unsuppress(&mut self) { self.suppressed = false; self.was_in = false; }
    pub fn suppressed(&self) -> bool { self.suppressed }
}

/// 开始前提醒：距下一次开始不超过 `remind_before_min` 分钟时触发一次（返回剩余分钟数）。
/// 启动时如果已经在提醒区间里，也会立即提醒一次。
/// 多组定时时按「哪一段」记：一段结束后紧接着另一段，也会各提醒一次。
#[derive(Debug, Default)]
pub struct Reminder { fired_for: Option<u32> }

impl Reminder {
    /// 这一轮提醒已经弹过（还没被重置）
    pub fn fired(&self) -> bool { self.fired_for.is_some() }
    pub fn update(&mut self, cfg: &ScheduleConfig, weekday: u8, minute: u16) -> Option<u32> {
        let r = cfg.remind_before_min;
        if !cfg.enabled || r == 0 { self.fired_for = None; return None; }
        match focus(cfg, weekday, minute) {
            Some((_, m)) if m > r => { self.fired_for = None; None }
            Some((s, m)) if m > 0 && self.fired_for != Some(s.start) => { self.fired_for = Some(s.start); Some(m) }
            _ => None,
        }
    }
}

/// 计划到点时要不要马上息屏。被「推迟」或正在全屏播放视频时先等着；
/// 条件解除且仍在时段内时再开始。时间用毫秒（任意单调时钟）。
#[derive(Debug, Default)]
pub struct Gate { pending: bool, until_ms: u64 }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Waiting { Postponed, Fullscreen }

impl Gate {
    /// 推迟到 now + by（开始前、开始后都可以）
    pub fn postpone(&mut self, now_ms: u64, by_ms: u64) { self.until_ms = now_ms + by_ms; }
    /// 改了计划时间：之前的推迟作废（还在等的开始不受影响）
    pub fn clear_postpone(&mut self) { self.until_ms = 0; }
    pub fn on_start(&mut self) { self.pending = true; }
    /// 时段结束 / 跳过今晚：什么都不等了
    pub fn reset(&mut self) { self.pending = false; self.until_ms = 0; }
    /// true = 现在该息屏了（只返回一次）
    pub fn poll(&mut self, now_ms: u64, fullscreen: bool) -> bool {
        if !self.pending || now_ms < self.until_ms || fullscreen { return false; }
        self.pending = false;
        true
    }
    /// 正在等什么（给界面显示）；推迟优先
    pub fn waiting(&self, now_ms: u64, fullscreen: bool) -> Option<Waiting> {
        if now_ms < self.until_ms { Some(Waiting::Postponed) }
        else if self.pending && fullscreen { Some(Waiting::Fullscreen) }
        else { None }
    }
    pub fn postponed_until(&self, now_ms: u64) -> Option<u64> { (now_ms < self.until_ms).then_some(self.until_ms) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overnight() {
        let mut c = ScheduleConfig::default();
        c.enabled = true;
        c.entries[0].days = vec!["fri".into()];
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
        assert_eq!(duration_min(&c, 3, 20 * 60), Some(480));
        c.entries[0].days = vec!["mon".into()];
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

    #[test]
    fn skip_and_unskip() {
        let mut c = ScheduleConfig::default();
        c.enabled = true; // 每天 23:30–07:30
        // 时段开始前跳过，再撤销：到点照常息屏
        let mut t = Tracker::default();
        assert_eq!(t.update(&c, 0, 22 * 60), None);
        t.suppress();
        assert!(t.suppressed());
        t.unsuppress();
        assert!(!t.suppressed());
        assert_eq!(t.update(&c, 0, 23 * 60 + 30), Some(Edge::Start));

        // 时段内跳过（或手动唤醒），再撤销：马上重新开始
        let mut t = Tracker::default();
        assert_eq!(t.update(&c, 0, 23 * 60 + 40), Some(Edge::Start));
        t.suppress();
        assert_eq!(t.update(&c, 0, 23 * 60 + 41), None);
        t.unsuppress();
        assert_eq!(t.update(&c, 0, 23 * 60 + 42), Some(Edge::Start));
        assert_eq!(t.update(&c, 1, 7 * 60 + 30), Some(Edge::End));

        // 跳过后关掉再打开计划：跳过标记被清掉
        let mut t = Tracker::default();
        t.suppress();
        let mut off = c.clone();
        off.enabled = false;
        t.update(&off, 0, 22 * 60);
        assert!(!t.suppressed());

        // 过期时间
        assert_eq!(minutes_until_end(&c, 0, 23 * 60 + 40), Some(7 * 60 + 50));
        assert_eq!(minutes_until_end(&c, 0, 22 * 60), Some(90 + 480));
        assert_eq!(minutes_until_end(&off, 0, 22 * 60), None);
    }

    #[test]
    fn reminder() {
        let mut c = ScheduleConfig::default();
        c.enabled = true; // 23:30 开始，默认提前 5 分钟
        let mut r = Reminder::default();
        assert_eq!(r.update(&c, 0, 23 * 60), None);
        assert_eq!(r.update(&c, 0, 23 * 60 + 25), Some(5));
        assert_eq!(r.update(&c, 0, 23 * 60 + 26), None); // 只提醒一次
        assert_eq!(r.update(&c, 0, 23 * 60 + 31), None); // 时段内
        assert_eq!(r.update(&c, 1, 12 * 60), None);
        assert_eq!(r.update(&c, 1, 23 * 60 + 27), Some(3)); // 第二天晚上照常提醒（启动晚了也提醒）
        c.remind_before_min = 0;
        let mut r = Reminder::default();
        assert_eq!(r.update(&c, 0, 23 * 60 + 25), None);
    }

    #[test]
    fn gate() {
        { let mut g = Gate::default(); g.postpone(0, 1000); g.clear_postpone(); g.on_start(); assert!(g.poll(10, false)); }
        let mut g = Gate::default();
        assert!(!g.poll(0, false)); // 没到开始
        g.on_start();
        assert_eq!(g.waiting(0, true), Some(Waiting::Fullscreen));
        assert!(!g.poll(0, true)); // 全屏视频：等
        assert!(g.poll(1000, false)); // 视频结束：开始
        assert!(!g.poll(2000, false)); // 只一次

        let mut g = Gate::default();
        g.postpone(0, 30 * 60_000); // 提醒时点了「推迟 30 分钟」
        g.on_start();
        assert_eq!(g.waiting(60_000, false), Some(Waiting::Postponed));
        assert!(!g.poll(60_000, false));
        assert!(g.poll(30 * 60_000, false));

        let mut g = Gate::default();
        g.on_start();
        g.reset(); // 跳过今晚 / 时段结束
        assert!(!g.poll(0, false));
        assert_eq!(g.waiting(0, true), None);
    }

    fn entry(start: &str, end: &str, days: &[&str]) -> ScheduleEntry {
        ScheduleEntry { enabled: true, label: String::new(), start: start.into(), end: end.into(),
                        days: days.iter().map(|d| d.to_string()).collect() }
    }
    fn multi(entries: Vec<ScheduleEntry>) -> ScheduleConfig {
        ScheduleConfig { enabled: true, entries, ..Default::default() }
    }
    const H: u16 = 60;

    #[test]
    fn multi_separate_days() {
        // 工作日 23:30–07:30，周末 01:00–09:00
        let wk = ["mon", "tue", "wed", "thu", "fri"];
        let c = multi(vec![entry("23:30", "07:30", &wk), entry("01:00", "09:00", &["sat", "sun"])]);
        assert!(in_window(&c, 4, 23 * H + 45)); // 周五晚
        assert!(in_window(&c, 5, 7 * H)); // 周六早：周五晚那段没结束
        assert!(in_window(&c, 5, 8 * H)); // 周六 01:00–09:00 和周五晚那段接上了
        assert!(!in_window(&c, 5, 9 * H));
        assert!(!in_window(&c, 5, 23 * H + 45)); // 周六晚不在工作日里
        assert!(in_window(&c, 6, 2 * H));
        assert!(!in_window(&c, 6, 23 * H + 45)); // 周日晚：属于周日，不在工作日那组里
        // 周四 20:00 → 周四 23:30
        assert_eq!(minutes_until_start(&c, 3, 20 * H), Some(210));
        // 周五晚的时段和周六的合并：从 23:30 到周六 09:00
        let (s, d) = focus(&c, 4, 22 * H).unwrap();
        assert_eq!((d, s.start_hm(), s.end_hm(), s.len()), (90, 23 * H + 30, 9 * H, 570));
        assert_eq!(minutes_until_end(&c, 5, 8 * H), Some(60));
        // 周日 10:00 → 周日没有晚上的计划 → 周一 23:30
        assert_eq!(minutes_until_start(&c, 6, 10 * H), Some(14 * 60 + 23 * 60 + 30));
    }

    #[test]
    fn multi_overlap_and_adjacent() {
        // 重叠：22:00–01:00 和 00:30–06:00 → 22:00–06:00 一段
        let c = multi(vec![entry("22:00", "01:00", &["mon"]), entry("00:30", "06:00", &["tue"])]);
        assert_eq!(spans(&c), vec![Span { start: 22 * 60, end: 1440 + 6 * 60 }]);
        let mut t = Tracker::default();
        assert_eq!(t.update(&c, 0, 22 * H), Some(Edge::Start));
        assert_eq!(t.update(&c, 1, H), None); // 第一组结束了，但还在合并后的时段里
        assert_eq!(t.update(&c, 1, 6 * H), Some(Edge::End));
        // 首尾相接：00:00 结束、00:00 开始 → 不在午夜断开
        let c = multi(vec![entry("20:00", "00:00", &["mon"]), entry("00:00", "02:00", &["tue"])]);
        assert_eq!(spans(&c).len(), 1);
        assert_eq!(minutes_until_end(&c, 0, 23 * H), Some(3 * 60));
        // 包含：大的盖住小的
        let c = multi(vec![entry("13:00", "14:00", &["wed"]), entry("12:00", "18:00", &["wed"])]);
        assert_eq!(spans(&c), vec![Span { start: 2 * 1440 + 12 * 60, end: 2 * 1440 + 18 * 60 }]);
        // 同一天两段不相连：午休 + 夜里，各自开始、结束
        let c = multi(vec![entry("12:30", "13:30", &["mon"]), entry("23:00", "07:00", &["mon"])]);
        let mut t = Tracker::default();
        assert_eq!(t.update(&c, 0, 12 * H + 30), Some(Edge::Start));
        assert_eq!(t.update(&c, 0, 13 * H + 30), Some(Edge::End));
        assert_eq!(t.update(&c, 0, 23 * H), Some(Edge::Start));
        assert_eq!(t.update(&c, 1, 7 * H), Some(Edge::End));
        assert_eq!(day_segments(&c, 0), vec![(12 * H + 30, 13 * H + 30), (23 * H, 1440)]);
        assert_eq!(day_segments(&c, 1), vec![(0, 7 * H)]);
    }

    #[test]
    fn multi_week_wrap_and_disabled() {
        // 周日 22:00–08:00 跨到下周一，和周一 07:00–09:00 合并
        let c = multi(vec![entry("22:00", "08:00", &["sun"]), entry("07:00", "09:00", &["mon"])]);
        assert_eq!(spans(&c), vec![Span { start: 6 * 1440 + 22 * 60, end: WEEK + 9 * 60 }]);
        assert!(in_window(&c, 0, 8 * H + 30));
        assert!(!in_window(&c, 0, 9 * H));
        assert_eq!(minutes_until_end(&c, 0, 8 * H), Some(60));
        assert_eq!(minutes_until_start(&c, 0, 10 * H), Some(6 * 1440 + 12 * 60));
        assert_eq!(day_segments(&c, 0), vec![(0, 9 * H)]);
        assert_eq!(day_segments(&c, 6), vec![(22 * H, 1440)]);
        // 关掉的组、开始等于结束的组、没选星期的组都不生效
        let mut off = entry("10:00", "11:00", &["mon"]);
        off.enabled = false;
        let c = multi(vec![off, entry("12:00", "12:00", &["mon"]), entry("13:00", "14:00", &[])]);
        assert!(spans(&c).is_empty());
        assert_eq!(minutes_until_start(&c, 0, 0), None);
        assert_eq!(minutes_until_end(&c, 0, 0), None);
        // 总开关关掉：都不生效
        let mut c = multi(vec![entry("10:00", "11:00", &["mon"])]);
        c.enabled = false;
        assert!(!in_window(&c, 0, 10 * H + 30));
        // 每天全天（只差 1 分钟）也不会出错
        let c = multi(vec![entry("00:00", "23:59", &DAYS)]);
        assert!(in_window(&c, 3, 12 * H) && !in_window(&c, 3, 23 * H + 59));
    }

    #[test]
    fn multi_reminder_and_skip_per_occurrence() {
        // 两段之间只隔 3 分钟：第二段也要提醒
        let mut c = multi(vec![entry("12:00", "13:00", &["mon"]), entry("13:03", "14:00", &["mon"])]);
        c.remind_before_min = 5;
        let mut r = Reminder::default();
        assert_eq!(r.update(&c, 0, 11 * H + 55), Some(5));
        assert_eq!(r.update(&c, 0, 12 * H + 10), None);
        assert_eq!(r.update(&c, 0, 13 * H), Some(3));
        assert_eq!(r.update(&c, 0, 13 * H + 1), None);
        // 跳过只管当前这一段：下一段照常开始
        let mut t = Tracker::default();
        assert_eq!(t.update(&c, 0, 11 * H), None);
        t.suppress();
        assert_eq!(t.update(&c, 0, 12 * H), None);
        assert_eq!(t.update(&c, 0, 13 * H), None);
        assert!(!t.suppressed());
        assert_eq!(t.update(&c, 0, 13 * H + 3), Some(Edge::Start));
        // 跳过的过期时间是当前这段的结束
        assert_eq!(minutes_until_end(&c, 0, 11 * H), Some(120));
        assert_eq!(minutes_until_end(&c, 0, 13 * H + 30), Some(30));
        // 名称
        let mut c = c.clone();
        c.entries[1].label = "午休".into();
        let (s, _) = focus(&c, 0, 13 * H + 1).unwrap();
        assert_eq!(label_of(&c, &s), Some("午休"));
        assert_eq!(label_of(&c, &focus(&c, 0, 11 * H).unwrap().0), None);
    }
}
