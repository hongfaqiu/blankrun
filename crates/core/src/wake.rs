//! 唤醒策略：把原始输入事件过滤成"是否真的要唤醒"，用于防误触（猫踩、桌子震动）。
//! 平台钩子只负责把原生事件翻译成 `Input`（规范键名 + 修饰键位），匹配和"按住多久"都在这里判断。
use crate::config::{Config, WakeMethod};
use crate::hotkey::Hotkey;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Input {
    /// `key` 为 hotkey::norm_key 规范名；纯修饰键按下时为 "Mod"
    Key { key: &'static str, mods: u8, down: bool, t_ms: u64 },
    MouseMove { dx: f64, dy: f64, t_ms: u64 },
    Click,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Verdict {
    Wake,
    /// 正在按住唤醒键，进度 0..1（给遮罩画进度条）
    Holding(f32),
    /// 被拦下的普通输入（屏幕可能被它点亮，平台层据此提示并安排重新息屏）
    Ignored,
}

/// 息屏时一条输入怎么处理：远程软件 / 屏幕键盘「注入」的输入，在「放行远程控制」开着时直接放行，
/// 既不吞掉，也不算误触、不参与唤醒判断。物理输入一律交给 WakePolicy。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route { PassThrough, Policy }
pub fn route(injected: bool, allow_remote: bool) -> Route {
    if injected && allow_remote { Route::PassThrough } else { Route::Policy }
}

/// 「键鼠照常用」（Mac Mini 模式）时，输入钩子对每个按键的取舍：只吞唤醒键 / 紧急键这两个组合
/// （按下、自动重复和对应的抬起），别的键原样送给前台 App。修饰键永远放行，
/// 所以前台 App 看到的是一次完整的 ⌃⌥ 按下-抬起，不会以为它们卡住了。
/// 被吞掉的组合仍然交给 WakePolicy 判断按住多久，所以快捷键照常能唤醒。
#[derive(Debug, Clone, Default)]
pub struct KeepFilter {
    keys: Vec<Hotkey>,
    /// 正被吞掉的主键（它的抬起也要吞，哪怕那时修饰键已经先松开了）
    held: Option<&'static str>,
}
impl KeepFilter {
    /// `wake`：唤醒方式需要唤醒快捷键时才传（「仅按计划」不传）；紧急键总是认
    pub fn new(wake: Option<&str>, emergency: &str) -> Self {
        Self { keys: wake.and_then(Hotkey::parse).into_iter().chain(Hotkey::parse(emergency)).collect(), held: None }
    }
    pub fn from_config(cfg: &Config) -> Self {
        let wake = (keep_input_method(cfg.wake.method) == WakeMethod::Hotkey).then_some(cfg.wake.hotkey.as_str());
        Self::new(wake, &cfg.safety.emergency_hotkey)
    }
    /// 一个键盘事件（`key` 为规范键名，纯修饰键为 "Mod"）；返回 true = 吞掉
    pub fn key(&mut self, key: &'static str, mods: u8, down: bool) -> bool {
        if key == "Mod" { return false; }
        if down {
            if self.keys.iter().any(|h| h.matches(key, mods)) { self.held = Some(key); return true; }
            // 按住组合时先松了一个修饰键，主键的自动重复还在：继续吞，直到它抬起
            self.held == Some(key)
        } else if self.held == Some(key) {
            self.held = None;
            true
        } else { false }
    }
}

/// 「键鼠照常用」时实际生效的唤醒方式：任意键 / 鼠标 / 点击这些方式会被正常使用的输入立刻触发，
/// 一律改成「按住唤醒快捷键」；「仅按计划」保持（紧急键照样能用）。
pub fn keep_input_method(m: WakeMethod) -> WakeMethod {
    match m { WakeMethod::ScheduleOnly => WakeMethod::ScheduleOnly, _ => WakeMethod::Hotkey }
}

#[derive(Debug, Default)]
pub struct WakePolicy {
    acc_px: f64,
    window_start_ms: Option<u64>,
    hold_start_ms: Option<u64>,
    wake: Option<Hotkey>,
    emergency: Option<Hotkey>,
    method: Option<WakeMethod>,
    hold_ms: u64,
    mouse_threshold_px: f64,
    mouse_window_ms: u64,
    /// 最近一次 Wake 是紧急键触发的（写息屏记录用）
    pub by_emergency: bool,
    /// 本次息屏是「键鼠照常用」：唤醒方式按 keep_input_method 改写
    keep_input: bool,
}

impl WakePolicy {
    pub fn new(cfg: &Config) -> Self {
        let mut p = Self::default();
        p.configure(cfg);
        p
    }
    pub fn configure(&mut self, cfg: &Config) {
        self.wake = Hotkey::parse(&cfg.wake.hotkey);
        self.emergency = Hotkey::parse(&cfg.safety.emergency_hotkey);
        self.method = Some(cfg.wake.method);
        self.hold_ms = cfg.wake.hold_ms as u64;
        self.mouse_threshold_px = cfg.wake.mouse_threshold_px as f64;
        self.mouse_window_ms = cfg.wake.mouse_window_ms as u64;
        self.reset();
    }
    /// 每次息屏开始时设置（configure 不会改它，息屏中改设置也不影响本次）
    pub fn set_keep_input(&mut self, on: bool) { self.keep_input = on; self.reset(); }
    pub fn keep_input(&self) -> bool { self.keep_input }
    pub fn is_holding(&self) -> bool { self.hold_start_ms.is_some() }
    pub fn reset(&mut self) { self.acc_px = 0.0; self.window_start_ms = None; self.hold_start_ms = None; }

    fn hold_progress(&self, t_ms: u64) -> Option<f32> {
        let s = self.hold_start_ms?;
        Some(if self.hold_ms == 0 { 1.0 } else { (t_ms.saturating_sub(s) as f32 / self.hold_ms as f32).min(1.0) })
    }

    /// 定时器调用：按住唤醒键期间没有新的按键事件（无自动重复）时也能判定。
    pub fn tick(&mut self, t_ms: u64) -> Option<Verdict> {
        let p = self.hold_progress(t_ms)?;
        Some(if p >= 1.0 { self.reset(); Verdict::Wake } else { Verdict::Holding(p) })
    }

    pub fn feed(&mut self, input: Input) -> Verdict {
        use Verdict::*;
        if let Input::Key { key, mods, down: true, .. } = input {
            if self.emergency.as_ref().is_some_and(|h| h.matches(key, mods)) { self.reset(); self.by_emergency = true; return Wake; }
        }
        self.by_emergency = false;
        let method = self.method.unwrap_or(WakeMethod::Hotkey);
        let method = if self.keep_input { keep_input_method(method) } else { method };
        match (method, input) {
            // Password：任意键唤醒后由平台锁屏，交给系统密码框
            (WakeMethod::AnyKey | WakeMethod::Password, Input::Key { down: true, .. }) => Wake,
            (WakeMethod::Hotkey, Input::Key { key, mods, down, t_ms }) => {
                let is_combo = self.wake.as_ref().is_some_and(|h| h.matches(key, mods));
                if down && is_combo {
                    self.hold_start_ms.get_or_insert(t_ms);
                    match self.tick(t_ms) { Some(v) => v, None => Ignored }
                } else if self.hold_start_ms.is_none() {
                    Ignored
                } else if key == "Mod" && self.wake.as_ref().is_some_and(|h| h.mods == mods) {
                    // 修饰键事件但组合没变（例如重复的 flagsChanged），继续按住
                    self.hold_progress(t_ms).map_or(Ignored, Holding)
                } else {
                    // 松开主键、按了别的键或修饰键变了：放弃本次按住
                    self.hold_start_ms = None;
                    Ignored
                }
            }
            (WakeMethod::Click, Input::Click) => Wake,
            (WakeMethod::MouseMove, Input::MouseMove { dx, dy, t_ms }) => {
                let start = *self.window_start_ms.get_or_insert(t_ms);
                if t_ms.saturating_sub(start) > self.mouse_window_ms {
                    self.window_start_ms = Some(t_ms);
                    self.acc_px = 0.0;
                }
                self.acc_px += (dx * dx + dy * dy).sqrt();
                if self.acc_px >= self.mouse_threshold_px { self.reset(); Wake } else { Ignored }
            }
            _ => Ignored,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hotkey::{ALT, CTRL, SHIFT};

    fn cfg(m: WakeMethod) -> Config { let mut c = Config::default(); c.wake.method = m; c }
    fn key(k: &'static str, mods: u8, down: bool, t_ms: u64) -> Input { Input::Key { key: k, mods, down, t_ms } }

    #[test]
    fn jitter_filtered() {
        let mut p = WakePolicy::new(&cfg(WakeMethod::MouseMove));
        for i in 0..10 { assert_eq!(p.feed(Input::MouseMove { dx: 2.0, dy: 0.0, t_ms: i * 500 }), Verdict::Ignored); }
        let mut p = WakePolicy::new(&cfg(WakeMethod::MouseMove));
        assert_eq!(p.feed(Input::MouseMove { dx: 100.0, dy: 0.0, t_ms: 0 }), Verdict::Ignored);
        assert_eq!(p.feed(Input::MouseMove { dx: 60.0, dy: 0.0, t_ms: 300 }), Verdict::Wake);
    }

    #[test]
    fn emergency_always() {
        let mut p = WakePolicy::new(&cfg(WakeMethod::ScheduleOnly));
        assert_eq!(p.feed(key("A", 0, true, 0)), Verdict::Ignored);
        assert_eq!(p.feed(key("Esc", CTRL | ALT | SHIFT, true, 0)), Verdict::Wake);
    }

    #[test]
    fn hotkey_must_be_held() {
        let mut p = WakePolicy::new(&cfg(WakeMethod::Hotkey)); // Ctrl+Alt+W, 1000ms
        // 短按不算
        assert!(matches!(p.feed(key("W", CTRL | ALT, true, 0)), Verdict::Holding(_)));
        assert_eq!(p.feed(key("W", CTRL | ALT, false, 300)), Verdict::Ignored);
        assert_eq!(p.tick(2000), None);
        // 错的修饰键不算
        assert_eq!(p.feed(key("W", CTRL, true, 3000)), Verdict::Ignored);
        // 按住（自动重复）满 1 秒唤醒
        p.feed(key("W", CTRL | ALT, true, 5000));
        assert_eq!(p.feed(key("W", CTRL | ALT, true, 5500)), Verdict::Holding(0.5));
        assert_eq!(p.feed(key("W", CTRL | ALT, true, 6000)), Verdict::Wake);
        // 没有自动重复时由 tick 判定
        p.feed(key("W", CTRL | ALT, true, 9000));
        assert_eq!(p.tick(9999), Some(Verdict::Holding(0.999)));
        assert_eq!(p.tick(10000), Some(Verdict::Wake));
    }

    #[test]
    fn any_key_and_click() {
        assert_eq!(WakePolicy::new(&cfg(WakeMethod::AnyKey)).feed(key("Q", 0, true, 0)), Verdict::Wake);
        assert_eq!(WakePolicy::new(&cfg(WakeMethod::Click)).feed(Input::Click), Verdict::Wake);
        assert_eq!(WakePolicy::new(&cfg(WakeMethod::Click)).feed(key("Q", 0, true, 0)), Verdict::Ignored);
    }
    #[test]
    fn injected_routing() {
        assert_eq!(route(true, true), Route::PassThrough);
        assert_eq!(route(true, false), Route::Policy);
        assert_eq!(route(false, true), Route::Policy);
        assert_eq!(route(false, false), Route::Policy);
    }

    #[test]
    fn keep_input_policy_only_hotkey_wakes() {
        // 唤醒方式是「任意键」，但「键鼠照常用」时打字、动鼠标、点击都不能唤醒，只认按住唤醒键
        let mut p = WakePolicy::new(&cfg(WakeMethod::AnyKey));
        p.set_keep_input(true);
        assert_eq!(p.feed(key("Q", 0, true, 0)), Verdict::Ignored);
        assert_eq!(p.feed(Input::Click), Verdict::Ignored);
        assert_eq!(p.feed(Input::MouseMove { dx: 5000.0, dy: 0.0, t_ms: 10 }), Verdict::Ignored);
        assert!(matches!(p.feed(key("W", CTRL | ALT, true, 100)), Verdict::Holding(_)));
        assert_eq!(p.tick(1100), Some(Verdict::Wake));
        // 改设置（configure）不会把本次息屏的 keep_input 冲掉
        p.configure(&cfg(WakeMethod::MouseMove));
        assert!(p.keep_input());
        assert_eq!(p.feed(Input::MouseMove { dx: 5000.0, dy: 0.0, t_ms: 2000 }), Verdict::Ignored);
        // 仅按计划：唤醒键不唤醒，紧急键照样
        let mut p = WakePolicy::new(&cfg(WakeMethod::ScheduleOnly));
        p.set_keep_input(true);
        p.feed(key("W", CTRL | ALT, true, 0));
        assert_eq!(p.tick(5000), None);
        assert_eq!(p.feed(key("Esc", CTRL | ALT | SHIFT, true, 6000)), Verdict::Wake);
        // 关掉后恢复原来的唤醒方式
        let mut p = WakePolicy::new(&cfg(WakeMethod::AnyKey));
        p.set_keep_input(true);
        p.set_keep_input(false);
        assert_eq!(p.feed(key("Q", 0, true, 0)), Verdict::Wake);
    }

    #[test]
    fn keep_filter_swallows_only_the_combos() {
        let mut c = Config::default(); // 唤醒 Ctrl+Alt+W，紧急 Ctrl+Alt+Shift+Esc
        let mut f = KeepFilter::from_config(&c);
        // 普通打字、修饰键、别的组合：全部放行
        assert!(!f.key("A", 0, true) && !f.key("A", 0, false));
        assert!(!f.key("Mod", CTRL | ALT, true));
        assert!(!f.key("W", CTRL, true) && !f.key("W", CTRL, false));
        assert!(!f.key("W", 0, true) && !f.key("W", 0, false));
        // 唤醒组合：按下、自动重复、抬起都吞
        assert!(f.key("W", CTRL | ALT, true));
        assert!(f.key("W", CTRL | ALT, true));
        // 先松了 Alt，W 还在重复、然后抬起：继续吞，不漏一个 w 给前台 App
        assert!(!f.key("Mod", CTRL, false));
        assert!(f.key("W", CTRL, true));
        assert!(f.key("W", 0, false));
        assert!(!f.key("W", 0, true), "下一次普通的 w 放行");
        // 紧急键
        assert!(f.key("Esc", CTRL | ALT | SHIFT, true) && f.key("Esc", CTRL | ALT | SHIFT, false));
        assert!(!f.key("Esc", 0, true));
        // 「仅按计划」：唤醒组合不吞（它不唤醒，吞了只会让前台 App 收不到），紧急键照吞
        c.wake.method = WakeMethod::ScheduleOnly;
        let mut f = KeepFilter::from_config(&c);
        assert!(!f.key("W", CTRL | ALT, true));
        assert!(f.key("Esc", CTRL | ALT | SHIFT, true));
        assert_eq!(keep_input_method(WakeMethod::Password), WakeMethod::Hotkey);
    }
}
