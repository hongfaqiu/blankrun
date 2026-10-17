//! 息屏记录：最近 30 次，存成 config.toml 旁边的 history.json。
//! 息屏进行中的那一条 ended = "running"，end 是最近一次心跳的时间；
//! App 崩溃/强退/断电后下次启动时，把它记成 "crash"（时长算到最后一次心跳）。
use serde::{Deserialize, Serialize};

pub const CAP: usize = 30;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Session {
    /// unix 秒
    pub start: u64,
    pub end: u64,
    /// manual（面板/托盘/命令行）| hotkey（立即息屏快捷键）| schedule
    pub trigger: String,
    /// wake（按唤醒方式唤醒）| emergency | schedule（时段结束）| manual（面板/托盘）| quit | no_hook | crash | running
    pub ended: String,
    /// 被拦下的误触次数
    #[serde(default)]
    pub touches: u32,
    /// 屏幕被点亮后自动重新息屏的次数
    #[serde(default)]
    pub reblanks: u32,
}

impl Session {
    pub fn duration_s(&self) -> u64 { self.end.saturating_sub(self.start) }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct History {
    /// 旧的在前
    pub sessions: Vec<Session>,
}

impl History {
    /// 文件坏了就当没有记录（不影响 App 运行）
    pub fn from_json(s: &str) -> Self {
        let mut h: History = serde_json::from_str(s).unwrap_or_default();
        h.trim();
        h
    }
    pub fn to_json(&self) -> String { serde_json::to_string_pretty(self).unwrap_or_default() }

    fn trim(&mut self) {
        if self.sessions.len() > CAP { self.sessions.drain(..self.sessions.len() - CAP); }
    }

    /// 开始一次息屏（先作为 running 记下，崩溃了也有据可查）
    pub fn begin(&mut self, start: u64, trigger: &str) {
        self.recover();
        self.sessions.push(Session { start, end: start, trigger: trigger.into(), ended: "running".into(), touches: 0, reblanks: 0 });
        self.trim();
    }

    /// 进行中的那一条
    pub fn current(&mut self) -> Option<&mut Session> {
        self.sessions.last_mut().filter(|s| s.ended == "running")
    }

    /// 心跳 / 更新计数
    pub fn touch(&mut self, now: u64, touches: u32, reblanks: u32) {
        if let Some(s) = self.current() { s.end = now.max(s.start); s.touches = touches; s.reblanks = reblanks; }
    }

    pub fn finish(&mut self, now: u64, ended: &str, touches: u32, reblanks: u32) {
        if let Some(s) = self.current() { s.end = now.max(s.start); s.ended = ended.into(); s.touches = touches; s.reblanks = reblanks; }
    }

    /// 启动时调用：上次没结束的记为崩溃恢复。返回是否改动了
    pub fn recover(&mut self) -> bool {
        let mut changed = false;
        for s in self.sessions.iter_mut().filter(|s| s.ended == "running") { s.ended = "crash".into(); changed = true; }
        changed
    }

    pub fn clear(&mut self) { self.sessions.clear(); }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_keeps_last_30() {
        let mut h = History::default();
        for i in 0..45u64 { h.begin(i * 100, "manual"); h.finish(i * 100 + 50, "wake", 1, 0); }
        assert_eq!(h.sessions.len(), CAP);
        assert_eq!(h.sessions[0].start, 1500);
        assert_eq!(h.sessions.last().unwrap().start, 4400);
        assert_eq!(h.sessions.last().unwrap().duration_s(), 50);
    }

    #[test]
    fn running_then_crash_recovered() {
        let mut h = History::default();
        h.begin(1000, "schedule");
        h.touch(1600, 3, 1);
        let j = h.to_json();
        let mut back = History::from_json(&j);
        assert_eq!(back, h);
        assert!(back.recover());
        let s = &back.sessions[0];
        assert_eq!((s.ended.as_str(), s.duration_s(), s.touches, s.reblanks), ("crash", 600, 3, 1));
        assert!(!back.recover());
        // 新的一次开始时也会先把遗留的 running 收尾
        let mut h2 = History::from_json(&j);
        h2.begin(2000, "hotkey");
        assert_eq!(h2.sessions[0].ended, "crash");
        assert_eq!(h2.sessions[1].ended, "running");
    }

    #[test]
    fn serialization_tolerant() {
        assert_eq!(History::from_json("not json"), History::default());
        assert_eq!(History::from_json(""), History::default());
        // 少了计数字段（旧格式）也能读
        let h = History::from_json(r#"{"sessions":[{"start":1,"end":5,"trigger":"manual","ended":"wake"}]}"#);
        assert_eq!(h.sessions[0].touches, 0);
        // 文件里超过 30 条：只留最新的
        let many: Vec<String> = (0..40).map(|i| format!(r#"{{"start":{i},"end":{i},"trigger":"manual","ended":"wake"}}"#)).collect();
        let h = History::from_json(&format!(r#"{{"sessions":[{}]}}"#, many.join(",")));
        assert_eq!((h.sessions.len(), h.sessions[0].start), (CAP, 10));
        let mut h = h; h.clear(); assert!(h.sessions.is_empty());
        // finish 没有进行中的记录时什么都不做
        h.finish(9, "wake", 0, 0); assert!(h.sessions.is_empty());
    }
}
