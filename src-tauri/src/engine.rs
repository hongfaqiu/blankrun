//! 引擎：唯一调用平台代码的线程。状态机 + 倒计时 + 定时计划 + 唤醒策略 + 遮罩窗口 + 意外点亮后重新息屏。
//! Windows 的 SetThreadExecutionState 按线程生效，所以所有平台动作都必须在这个线程里执行；
//! Tauri 命令只把请求丢进通道。
use std::path::PathBuf;
use std::sync::{mpsc::{Receiver, RecvTimeoutError}, Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use blankrun_core::{
    config::{DisplayMethod, WakeMethod},
    schedule::{self, Edge, Gate, Reminder, Tracker, Waiting},
    state::Action,
    wake::{Input, Verdict, WakePolicy},
    Config, Event, State, StateMachine,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::platform::{self, Native, Platform};

pub enum Msg {
    Input(Input),
    /// 遮罩页面自己收到的键鼠（输入钩子装不上时的兜底）
    OverlayInput,
    Blank { delay_s: u32 },
    Cancel,
    Wake,
    SkipTonight,
    /// 撤销「跳过今晚」
    Unskip,
    /// 提醒里点了「推迟」
    Postpone { min: u32 },
    /// 「立即息屏」全局快捷键
    Hotkey,
    Quit,
}

#[derive(Debug, Clone, Serialize)]
pub struct Session { pub start_unix: u64, pub end_unix: Option<u64>, pub lit: u32, pub by_schedule: bool, pub power_off: bool }

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SchedStatus {
    pub enabled: bool, pub start: String, pub end: String, pub in_window: bool, pub until_min: Option<u32>, pub duration_min: Option<u32>, pub skipped: bool,
    /// 到点后在等什么："postponed"（推迟中）/ "fullscreen"（全屏视频）
    pub waiting: Option<&'static str>,
    /// 推迟还剩几分钟
    pub postponed_min: Option<u32>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Status {
    pub state: &'static str,
    pub countdown_left: u32,
    pub countdown_total: u32,
    pub now_min: u16,
    pub schedule: SchedStatus,
    pub displays: usize,
    pub keyboard: bool,
    pub power_off: bool,
    /// 「立即息屏」快捷键注册失败的原因
    pub hotkey_error: Option<String>,
    /// 是否已设为开机启动（None = 不支持）
    pub login: Option<bool>,
    /// 误触防护："ok" 有输入钩子；"keys" 没权限，但唤醒键走系统热键、误触后会重新息屏；"none" 没权限且碰一下就会唤醒
    pub input_guard: &'static str,
}

pub struct Engine {
    pub sm: StateMachine,
    pub plat: Native,
    pub cfg: Config,
    pub cfg_path: PathBuf,
    pub cfg_mtime: Option<SystemTime>,
    pub last_session: Option<Session>,
    policy: WakePolicy,
    tracker: Tracker,
    reminder: Reminder,
    remind_open: bool,
    gate: Gate,
    /// 全屏视频检测结果的缓存（每 5 秒查一次）
    fullscreen: (Option<Instant>, bool),
    hotkey_dirty: bool,
    fullscreen_logged: bool,
    login: Option<bool>,
    countdown: Option<(Instant, u32)>,
    power_off: bool,
    overlay_on: bool,
    power_off_at: Option<Instant>,
    reblank_at: Option<Instant>,
    hook_ok: bool,
    /// 本次息屏注册了兜底唤醒键（要在结束时注销）
    wake_keys: bool,
    session: Option<Session>,
    last_hint: Option<Instant>,
    last_sched_check: Option<Instant>,
    last_cfg_check: Option<Instant>,
    last_status: Option<Status>,
    kbd_available: bool,
    displays: usize,
    last_mon_check: Option<Instant>,
    keep_display: bool,
    last_asleep: Option<bool>,
    /// 刚发出关屏命令后的几秒里屏幕还没真正睡下，这段时间不算「被点亮」
    settle_until: Option<Instant>,
}

fn unix_now() -> u64 { SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) }

impl Engine {
    pub fn new(cfg: Config, cfg_path: PathBuf, mut plat: Native) -> Self {
        plat.configure(&cfg);
        let kbd_available = plat.capabilities().iter().any(|c| c.id == "kbd" && c.status == "ok");
        let mtime = std::fs::metadata(&cfg_path).and_then(|m| m.modified()).ok();
        let mut e = Self {
            sm: StateMachine::new(cfg.wake.lock_on_wake), policy: WakePolicy::new(&cfg), plat, cfg, cfg_path,
            cfg_mtime: mtime, last_session: None, tracker: Tracker::default(), reminder: Reminder::default(), remind_open: false, gate: Gate::default(), fullscreen: (None, false),
            hotkey_dirty: true, fullscreen_logged: false, login: None, countdown: None, power_off: false,
            overlay_on: false, power_off_at: None, reblank_at: None, hook_ok: true, wake_keys: false, session: None, last_hint: None,
            last_sched_check: None, last_cfg_check: None, last_status: None, kbd_available,
            displays: 1, last_mon_check: None, keep_display: false, last_asleep: None, settle_until: None,
        };
        e.login = e.plat.launch_at_login();
        if e.cfg.general.launch_at_login || e.login == Some(true) { e.sync_login(); } // 每次启动重写一遍（App 位置可能变了）
        let until = std::fs::read_to_string(e.skip_path()).ok().and_then(|s| s.trim().parse::<u64>().ok());
        match until {
            Some(u) if u > unix_now() => { e.tracker.suppress(); nblog!("schedule: still skipped ({}s left)", u - unix_now()); }
            Some(_) => { let _ = std::fs::remove_file(e.skip_path()); }
            None => {}
        }
        e
    }

    pub fn marker_path(&self) -> PathBuf { self.cfg_path.with_file_name("blanking.state") }
    /// 「跳过今晚」的过期时间（unix 秒），重启后照样有效
    fn skip_path(&self) -> PathBuf { self.cfg_path.with_file_name("skip.until") }

    fn sync_login(&mut self) {
        let on = self.cfg.general.launch_at_login;
        match self.plat.set_launch_at_login(on) { Ok(()) => nblog!("launch at login = {on}"), Err(e) => nblog!("launch at login = {on} failed: {e}") }
        self.login = self.plat.launch_at_login();
    }

    fn skip(&mut self) {
        self.tracker.suppress();
        self.gate.reset();
        let (wd, min) = platform::local_time();
        if let Some(m) = schedule::minutes_until_end(&self.cfg.schedule, wd, min) {
            let _ = std::fs::write(self.skip_path(), (unix_now() + m as u64 * 60).to_string());
        }
        nblog!("schedule: skipped until the current/next window ends");
    }
    fn unskip(&mut self) {
        self.tracker.unsuppress();
        let _ = std::fs::remove_file(self.skip_path());
        nblog!("schedule: skip undone");
    }

    /// 配置变化（设置页保存或外部编辑 config.toml）
    pub fn set_config(&mut self, cfg: Config) {
        // 计划从关到开：之前的「跳过」作废
        if cfg.schedule.enabled && !self.cfg.schedule.enabled && self.tracker.suppressed() { self.unskip(); }
        let (a, b) = (&cfg.schedule, &self.cfg.schedule);
        if a.start != b.start || a.end != b.end || a.days != b.days || a.enabled != b.enabled { self.gate.clear_postpone(); }
        let login_changed = cfg.general.launch_at_login != self.cfg.general.launch_at_login;
        if cfg.general.blank_now_hotkey != self.cfg.general.blank_now_hotkey { self.hotkey_dirty = true; }
        self.plat.configure(&cfg);
        self.policy.configure(&cfg);
        self.cfg = cfg;
        if login_changed { self.sync_login(); }
    }

    pub fn save_config(&mut self, cfg: Config) -> Result<(), String> {
        let mut cfg = cfg;
        cfg.normalize()?;
        if let Some(dir) = self.cfg_path.parent() { std::fs::create_dir_all(dir).map_err(|e| e.to_string())?; }
        std::fs::write(&self.cfg_path, cfg.to_toml()).map_err(|e| e.to_string())?;
        self.cfg_mtime = std::fs::metadata(&self.cfg_path).and_then(|m| m.modified()).ok();
        self.set_config(cfg);
        Ok(())
    }

    fn apply(&mut self, app: &AppHandle, ev: Event) {
        for a in self.sm.handle(ev) {
            let r = match a {
                Action::StartKeepAwake => self.plat.keep_awake(true),
                Action::StopKeepAwake => self.plat.keep_awake(false),
                Action::DisplayOff => {
                    if !self.overlay_on {
                        self.overlay_on = true;
                        show_overlays(app, &self.cfg);
                        // 等遮罩窗口出来再断电，免得新窗口把屏幕又点亮
                        if self.power_off { self.power_off_at = Some(Instant::now() + Duration::from_millis(600)); }
                        Ok(())
                    } else if self.power_off {
                        self.settle_until = Some(Instant::now() + Duration::from_secs(3));
                        self.plat.display_off(true)
                    } else { Ok(()) }
                }
                Action::DisplayRestore => {
                    self.overlay_on = false;
                    self.power_off_at = None;
                    self.reblank_at = None;
                    hide_overlays(app);
                    self.plat.display_off(false)
                }
                Action::BacklightOff if self.cfg.lights.keyboard_backlight => self.plat.keyboard_backlight(true),
                Action::BacklightOff => Ok(()),
                Action::BacklightRestore => self.plat.keyboard_backlight(false),
                Action::InputHookOn => {
                    let r = self.plat.input_hook(true);
                    self.hook_ok = r.is_ok();
                    // 钩子装不上（Mac 没给辅助功能权限）：唤醒键改用系统热键注册，误触点亮后仍能重新息屏
                    if r.is_err() && self.cfg.wake.method == WakeMethod::Hotkey {
                        let keys = (self.cfg.wake.hotkey.clone(), self.cfg.safety.emergency_hotkey.clone());
                        let _ = app.run_on_main_thread(move || platform::apply_wake_keys(Some(keys)));
                        self.wake_keys = true;
                    }
                    r
                }
                Action::InputHookOff => {
                    if std::mem::take(&mut self.wake_keys) { let _ = app.run_on_main_thread(|| platform::apply_wake_keys(None)); }
                    self.plat.input_hook(false)
                }
                Action::LockScreen => self.plat.lock_screen(),
            };
            match r { Ok(()) => nblog!("{a:?} ok"), Err(e) => nblog!("{a:?} failed: {e}") }
        }
    }

    fn arm(&mut self, app: &AppHandle, delay_s: u32, by_schedule: bool) {
        match self.sm.state {
            State::Idle => self.apply(app, Event::Arm),
            State::Armed => {}
            _ => return,
        }
        if delay_s == 0 { self.countdown = None; self.do_blank(app, by_schedule); }
        else { self.countdown = Some((Instant::now() + Duration::from_secs(delay_s as u64), delay_s)); }
    }

    fn do_blank(&mut self, app: &AppHandle, by_schedule: bool) {
        hide_reminder(app);
        let c = &self.cfg;
        // 「只关所选」时只能用遮罩（系统级关屏会把所有屏一起关掉）
        self.power_off = c.display.mode != "selected" && match c.display.method {
            DisplayMethod::PowerOff => true,
            DisplayMethod::Overlay => false,
            DisplayMethod::Auto => !self.plat.power_off_unsafe(),
        };
        self.sm.lock_on_wake = c.wake.lock_on_wake || c.wake.method == WakeMethod::Password;
        self.policy.reset();
        nblog!("blank: power_off={} method={:?} mode={} wake={:?} lock={}", self.power_off, c.display.method, c.display.mode, c.wake.method, self.sm.lock_on_wake);
        self.apply(app, Event::Blank);
        // Modern Standby 上只用遮罩时，屏幕要保持点亮（黑遮罩），否则系统空闲关屏后会进入 S0 低功耗待机
        self.keep_display = !self.power_off && self.plat.power_off_unsafe();
        if self.keep_display { let _ = self.plat.keep_display_on(true); }
        // 崩溃标记：下次启动据此还原背光等
        let _ = std::fs::write(self.marker_path(), self.plat.persist().unwrap_or_default());
        self.session = Some(Session { start_unix: unix_now(), end_unix: None, lit: 0, by_schedule, power_off: self.power_off });
    }

    fn cancel(&mut self, app: &AppHandle) {
        if self.sm.state == State::Armed { self.countdown = None; self.apply(app, Event::Cancel); }
    }

    fn wake(&mut self, app: &AppHandle, why: &str) {
        if self.sm.state == State::Armed { return self.cancel(app); }
        if self.sm.state != State::ScreenOff { return; }
        nblog!("wake: {why}");
        self.apply(app, Event::WakeRequested);
        if self.keep_display { self.keep_display = false; let _ = self.plat.keep_display_on(false); }
        self.apply(app, Event::RestoreDone);
        let _ = std::fs::remove_file(self.marker_path());
        if let Some(mut s) = self.session.take() {
            // 时段内被手动唤醒：本时段不再自动息屏
            let (wd, min) = platform::local_time();
            if schedule::in_window(&self.cfg.schedule, wd, min) { self.skip(); }
            s.end_unix = Some(unix_now());
            self.last_session = Some(s);
        }
    }

    fn hint(&mut self, app: &AppHandle, payload: serde_json::Value, throttle: bool) {
        let now = Instant::now();
        if throttle && self.last_hint.is_some_and(|t| now - t < Duration::from_millis(400)) { return; }
        self.last_hint = Some(now);
        let _ = app.emit("nb://overlay", payload);
    }

    pub fn handle(&mut self, app: &AppHandle, m: Msg) {
        match m {
            Msg::Input(i) if self.sm.state == State::ScreenOff => match self.policy.feed(i) {
                Verdict::Wake => self.wake(app, "input matched wake policy"),
                Verdict::Holding(p) => {
                    // 正在按住唤醒键：别在这时把屏幕又关掉
                    self.reblank_at = None;
                    self.hint(app, serde_json::json!({ "kind": "hold", "progress": p }), false)
                }
                Verdict::Ignored => {
                    let s = self.cfg.wake.reblank_after_s;
                    self.hint(app, serde_json::json!({ "kind": "hint", "reblank_s": s }), true);
                    if self.power_off && self.reblank_at.is_none() {
                        self.reblank_at = Some(Instant::now() + Duration::from_secs(s.max(1) as u64));
                    }
                }
            },
            Msg::Input(_) => {}
            Msg::OverlayInput if !self.hook_ok && !platform::wake_keys_ok() => self.wake(app, "overlay input (no hook)"),
            Msg::OverlayInput => {}
            Msg::Blank { delay_s } => self.arm(app, delay_s, false),
            Msg::Cancel => self.cancel(app),
            Msg::Wake => self.wake(app, "requested"),
            Msg::SkipTonight => { self.skip(); hide_reminder(app); }
            Msg::Unskip => self.unskip(),
            Msg::Postpone { min } => {
                self.gate.postpone(platform::now_ms(), min as u64 * 60_000);
                nblog!("schedule: postponed {min} min");
                hide_reminder(app);
            }
            Msg::Hotkey if matches!(self.sm.state, State::Idle | State::Armed) => { nblog!("blank-now hotkey"); self.arm(app, 0, false) }
            Msg::Hotkey => {}
            Msg::Quit => {
                nblog!("quit");
                self.shutdown(app);
                app.exit(0);
            }
        }
    }

    /// 退出前恢复一切
    pub fn shutdown(&mut self, app: &AppHandle) {
        self.wake(app, "quit");
        self.cancel(app);
        let _ = self.plat.keep_awake(false);
        let _ = std::fs::remove_file(self.marker_path());
    }

    pub fn tick(&mut self, app: &AppHandle) {
        let now = Instant::now();
        if let Some((at, _)) = self.countdown {
            if now >= at { self.countdown = None; self.do_blank(app, false); }
        }
        if let Some(at) = self.power_off_at {
            if now >= at {
                self.power_off_at = None;
                self.settle_until = Some(now + Duration::from_secs(3));
                match self.plat.display_off(true) { Ok(()) => nblog!("display power off ok"), Err(e) => nblog!("display power off failed: {e}") }
            }
        }
        if self.sm.state == State::ScreenOff {
            match self.policy.tick(platform::now_ms()) {
                Some(Verdict::Wake) => self.wake(app, "wake policy timer"),
                Some(Verdict::Holding(p)) => {
                    self.reblank_at = None;
                    self.hint(app, serde_json::json!({ "kind": "hold", "progress": p }), false)
                }
                _ => {}
            }
        }
        let asleep = self.plat.display_asleep();
        if asleep != self.last_asleep { nblog!("display asleep: {:?} -> {asleep:?}", self.last_asleep); self.last_asleep = asleep; }
        let settling = self.settle_until.is_some_and(|t| now < t);
        if self.sm.state == State::ScreenOff && self.power_off && self.power_off_at.is_none() && !settling {
            let lit = asleep == Some(false);
            if lit && !self.hook_ok && !platform::wake_keys_ok() {
                // 没有输入钩子、唤醒键也没注册上：无法分辨误触，屏幕亮了就算唤醒（免得把人锁在外面）
                self.wake(app, "screen lit and no input hook");
            } else if lit && self.reblank_at.is_none() && !self.policy.is_holding() {
                if !self.hook_ok { nblog!("screen lit without the wake key (no input hook): re-blank in {}s", self.cfg.wake.reblank_after_s.max(1)); }
                self.reblank_at = Some(now + Duration::from_secs(self.cfg.wake.reblank_after_s.max(1) as u64));
            }
            if let Some(at) = self.reblank_at {
                if now >= at {
                    self.reblank_at = None;
                    if self.plat.display_asleep() != Some(true) {
                        if let Some(s) = self.session.as_mut() { s.lit += 1; }
                        nblog!("screen lit unexpectedly: blanking again");
                        self.apply(app, Event::ScreenLitUnexpectedly);
                    }
                }
            }
        }
        if self.last_sched_check.is_none_or(|t| now - t >= Duration::from_secs(1)) {
            self.last_sched_check = Some(now);
            let (wd, min) = platform::local_time();
            let was_skipped = self.tracker.suppressed();
            let edge = self.tracker.update(&self.cfg.schedule, wd, min);
            if was_skipped && !self.tracker.suppressed() { let _ = std::fs::remove_file(self.skip_path()); }
            match edge {
                // TODO: skip_when_fullscreen_video 需要平台检测全屏视频；remind_before_min 需要系统通知
                // 到点不直接息屏，交给 Gate：被推迟或在全屏看视频时先等着
                Some(Edge::Start) if self.sm.state == State::Idle => self.gate.on_start(),
                Some(Edge::End) => {
                    self.gate.reset();
                    if self.session.as_ref().is_some_and(|s| s.by_schedule) { self.wake(app, "schedule ended"); }
                }
                _ => {}
            }
            let fs = self.fullscreen_now(now);
            let waiting = self.gate.waiting(platform::now_ms(), fs);
            if self.gate.poll(platform::now_ms(), fs) {
                if self.sm.state == State::Idle && schedule::in_window(&self.cfg.schedule, wd, min) { nblog!("schedule: start"); self.arm(app, 0, true); }
            } else if waiting == Some(Waiting::Fullscreen) && !self.fullscreen_logged {
                nblog!("schedule: waiting, a video is playing full screen");
            }
            self.fullscreen_logged = waiting == Some(Waiting::Fullscreen);
            if self.sm.state == State::Idle && !self.tracker.suppressed() {
                if let Some(m) = self.reminder.update(&self.cfg.schedule, wd, min) { nblog!("schedule: reminder, {m} min left"); show_reminder(app); self.remind_open = true; }
            }
            // 计划被关掉或改到更晚：已经弹出的提醒不再成立
            if self.remind_open && !self.reminder.fired() && !schedule::in_window(&self.cfg.schedule, wd, min) { hide_reminder(app); self.remind_open = false; }
        }
        if self.last_cfg_check.is_none_or(|t| now - t >= Duration::from_secs(2)) {
            self.last_cfg_check = Some(now);
            let m = std::fs::metadata(&self.cfg_path).and_then(|m| m.modified()).ok();
            if m != self.cfg_mtime {
                self.cfg_mtime = m;
                // 文件被删掉 = 回到默认设置（和第一次启动一样）
                let c = if m.is_none() { nblog!("config file removed, using defaults"); Some(Config::default()) }
                    else { std::fs::read_to_string(&self.cfg_path).ok().and_then(|s| Config::from_toml(&s).ok()) };
                if let Some(c) = c {
                    self.set_config(c);
                    let _ = app.emit("nb://config", ());
                }
            }
        }
        if self.hotkey_dirty {
            self.hotkey_dirty = false;
            let spec = self.cfg.general.blank_now_hotkey.clone();
            // Mac 的 Carbon 快捷键必须在主线程注册；不等结果，错误在 status.hotkey_error 里
            let _ = app.run_on_main_thread(move || platform::apply_blank_hotkey(&spec));
        }
        // 再次打开 App（第二个实例）留下的请求：把设置窗口拿到前面
        let req = self.cfg_path.with_file_name("show.request");
        if std::fs::remove_file(&req).is_ok() { nblog!("second launch: showing settings"); crate::show_settings(app, None); }
        if self.last_mon_check.is_none_or(|t| now - t >= Duration::from_secs(5)) {
            self.last_mon_check = Some(now);
            self.displays = app.available_monitors().map(|m| m.len().max(1)).unwrap_or(1);
        }
        let st = self.status();
        if self.last_status.as_ref() != Some(&st) {
            let _ = app.emit("nb://status", &st);
            self.last_status = Some(st);
        }
    }

    /// 计划在等着开始、且开了「全屏看视频时跳过」时才去查（每 5 秒一次）
    fn fullscreen_now(&mut self, now: Instant) -> bool {
        if !self.cfg.schedule.skip_when_fullscreen_video || !self.gate_pending() { self.fullscreen.1 = false; return false; }
        if self.fullscreen.0.is_none_or(|t| now - t >= Duration::from_secs(5)) {
            self.fullscreen = (Some(now), self.plat.fullscreen_video());
        }
        self.fullscreen.1
    }
    fn gate_pending(&self) -> bool { self.gate.waiting(platform::now_ms(), true).is_some() }

    pub fn status(&self) -> Status {
        let (wd, min) = platform::local_time();
        let s = &self.cfg.schedule;
        let (left, total) = self.countdown.map_or((0, 0), |(at, t)| {
            (at.saturating_duration_since(Instant::now()).as_secs_f32().ceil() as u32, t)
        });
        let in_win = schedule::in_window(s, wd, min);
        let until_min = schedule::minutes_until_start(s, wd, min);
        let now = platform::now_ms();
        let postponed_min = self.gate.postponed_until(now).map(|t| ((t - now) as f64 / 60_000.0).ceil() as u32);
        Status {
            state: match self.sm.state {
                State::Idle => "idle",
                State::Armed => "countdown",
                State::ScreenOff => "blank",
                State::Waking => "waking",
            },
            countdown_left: left,
            countdown_total: total,
            now_min: min,
            schedule: SchedStatus {
                enabled: s.enabled, start: s.start.clone(), end: s.end.clone(),
                in_window: in_win,
                until_min,
                duration_min: schedule::duration_min(s),
                skipped: self.tracker.suppressed(),
                waiting: match self.gate.waiting(platform::now_ms(), self.fullscreen.1) {
                    // 还没到开始时间：推迟只在比开始更晚时才值得显示
                    Some(Waiting::Postponed) if in_win || postponed_min > until_min => Some("postponed"),
                    Some(Waiting::Postponed) => None,
                    Some(Waiting::Fullscreen) => Some("fullscreen"),
                    None => None,
                },
                postponed_min,
            },
            displays: self.displays,
            keyboard: self.kbd_available && self.cfg.lights.keyboard_backlight,
            power_off: self.power_off,
            hotkey_error: platform::hotkey_error(),
            login: self.login,
            input_guard: match self.plat.permissions() {
                Some((false, _)) if self.cfg.wake.method == WakeMethod::Hotkey => "keys",
                Some((false, _)) => "none",
                _ => "ok",
            },
        }
    }
}

pub fn run(app: AppHandle, eng: Arc<Mutex<Engine>>, rx: Receiver<Msg>) {
    loop {
        let msg = rx.recv_timeout(Duration::from_millis(200));
        let mut e = eng.lock().unwrap_or_else(|p| p.into_inner());
        match msg {
            Ok(m) => e.handle(&app, m),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        e.tick(&app);
    }
}

fn show_overlays(app: &AppHandle, cfg: &Config) {
    let Ok(monitors) = app.available_monitors() else { return };
    let selected = cfg.display.mode == "selected";
    for (i, m) in monitors.into_iter().enumerate() {
        let name = monitor_name(i, &m);
        if selected && !cfg.display.targets.contains(&name) { continue; }
        let app2 = app.clone();
        let _ = app.run_on_main_thread(move || {
            let label = format!("overlay-{i}");
            if app2.get_webview_window(&label).is_some() { return; }
            let Ok(w) = WebviewWindowBuilder::new(&app2, &label, WebviewUrl::App("overlay.html".into()))
                .title("BlankRun")
                .decorations(false)
                .resizable(false)
                .always_on_top(true)
                .skip_taskbar(true)
                .focused(false)
                .shadow(false)
                .visible_on_all_workspaces(true)
                .background_color(tauri::window::Color(0, 0, 0, 255))
                .visible(false)
                .build() else { return };
            let _ = w.set_position(tauri::Position::Physical(*m.position()));
            let _ = w.set_size(tauri::Size::Physical(*m.size()));
            #[cfg(target_os = "macos")]
            if let Ok(ns) = w.ns_window() { platform::raise_overlay(ns); }
            let _ = w.set_cursor_visible(false);
            let _ = w.show();
        });
    }
}

/// 显示器名（设置页的「只关所选」按这个匹配）
pub fn monitor_name(i: usize, m: &tauri::Monitor) -> String {
    m.name().cloned().filter(|n| !n.is_empty()).unwrap_or_else(|| format!("Display {}", i + 1))
}

fn hide_overlays(app: &AppHandle) {
    for (label, w) in app.webview_windows() {
        if label.starts_with("overlay-") { let _ = w.destroy(); }
    }
}

/// 开始前提醒：屏幕右上角（Mac）/ 右下角（Windows）的小窗口，带「跳过今晚」「推迟 30 分钟」
fn show_reminder(app: &AppHandle) {
    let app2 = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(w) = app2.get_webview_window("remind") { let _ = w.show(); let _ = w.emit("nb://shown", ()); return; }
        let Ok(w) = WebviewWindowBuilder::new(&app2, "remind", WebviewUrl::App("remind.html".into()))
            .title("BlankRun").inner_size(340.0, 132.0).decorations(false).resizable(false).always_on_top(true)
            .skip_taskbar(true).focused(false).visible(false).build() else { return };
        if let Ok(Some(m)) = w.primary_monitor() {
            let (wa, sc) = (m.work_area(), m.scale_factor());
            let (ww, wh) = ((340.0 * sc) as i32, (132.0 * sc) as i32);
            let gap = (12.0 * sc) as i32;
            let x = wa.position.x + wa.size.width as i32 - ww - gap;
            let y = if cfg!(target_os = "macos") { wa.position.y + gap } else { wa.position.y + wa.size.height as i32 - wh - gap };
            let _ = w.set_position(tauri::PhysicalPosition::new(x, y));
        }
        let _ = w.show();
    });
}

fn hide_reminder(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("remind") { let _ = w.hide(); }
}
