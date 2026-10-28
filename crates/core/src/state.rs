//! 状态机：Idle → Armed（防睡眠已开，倒计时/等待计划）→ ScreenOff → Waking → Idle
//! 平台层根据返回的 Action 执行副作用，核心本身无副作用、可单测。

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State { Idle, Armed, ScreenOff, Waking }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Arm,             // 用户点"立即息屏"或计划开始
    Blank,           // 倒计时结束
    ScreenLitUnexpectedly, // 系统关屏后被输入点亮（RESEARCH.md §5 风险 1）
    WakeRequested,   // WakePolicy 判定唤醒 / 计划结束 / 紧急键
    RestoreDone,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    StartKeepAwake, StopKeepAwake,
    DisplayOff, DisplayRestore,
    BacklightOff, BacklightRestore,
    InputHookOn, InputHookOff,
    LockScreen,
}

#[derive(Debug)]
pub struct StateMachine { pub state: State, pub lock_on_wake: bool }

impl StateMachine {
    pub fn new(lock_on_wake: bool) -> Self { Self { state: State::Idle, lock_on_wake } }

    pub fn handle(&mut self, ev: Event) -> Vec<Action> {
        use Action::*;
        use State::*;
        let (next, actions) = match (self.state, ev) {
            (Idle, Event::Arm) => (Armed, vec![StartKeepAwake]),
            (Armed, Event::Blank) => (ScreenOff, vec![InputHookOn, BacklightOff, DisplayOff]),
            (Armed, Event::Cancel) => (Idle, vec![StopKeepAwake]),
            (ScreenOff, Event::ScreenLitUnexpectedly) => (ScreenOff, vec![DisplayOff]),
            (ScreenOff, Event::WakeRequested) => {
                // 要锁屏就先锁、再亮屏：否则亮屏（DDC 逐台写 D6=1、撤遮罩）到锁屏之间桌面会露出来一两秒。
                // 输入钩子仍然最先关，免得锁屏界面收不到键鼠
                let mut a = vec![InputHookOff];
                if self.lock_on_wake { a.push(LockScreen); }
                a.extend([DisplayRestore, BacklightRestore]);
                (Waking, a)
            }
            (Waking, Event::RestoreDone) => (Idle, vec![StopKeepAwake]),
            (s, _) => (s, vec![]),
        };
        self.state = next;
        actions
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn full_cycle() {
        let mut m = StateMachine::new(true);
        assert_eq!(m.handle(Event::Arm), vec![Action::StartKeepAwake]);
        m.handle(Event::Blank);
        assert_eq!(m.state, State::ScreenOff);
        assert_eq!(m.handle(Event::ScreenLitUnexpectedly), vec![Action::DisplayOff]);
        assert!(m.handle(Event::WakeRequested).contains(&Action::LockScreen));
        m.handle(Event::RestoreDone);
        assert_eq!(m.state, State::Idle);
        assert!(m.handle(Event::Blank).is_empty());
    }

    fn blanked(lock: bool) -> StateMachine {
        let mut m = StateMachine::new(lock);
        m.handle(Event::Arm);
        m.handle(Event::Blank);
        m
    }

    #[test]
    fn locks_before_lighting_the_screen() {
        let a = blanked(true).handle(Event::WakeRequested);
        assert_eq!(a, vec![Action::InputHookOff, Action::LockScreen, Action::DisplayRestore, Action::BacklightRestore]);
    }

    #[test]
    fn no_lock_when_disabled() {
        let a = blanked(false).handle(Event::WakeRequested);
        assert_eq!(a, vec![Action::InputHookOff, Action::DisplayRestore, Action::BacklightRestore]);
    }
}
