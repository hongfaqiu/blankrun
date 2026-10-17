//! 引擎：唯一调用平台代码的线程。状态机 + 倒计时 + 定时计划 + 唤醒策略 + 遮罩窗口 + 意外点亮后重新息屏。
//! Windows 的 SetThreadExecutionState 按线程生效，所以所有平台动作都必须在这个线程里执行；
//! Tauri 命令只把请求丢进通道。
use std::path::PathBuf;
use std::sync::{mpsc::{Receiver, RecvTimeoutError}, Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use nightblank_core::{
    config::{DisplayMethod, WakeMethod},
    schedule::{self, Edge, Tracker},
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
    Quit,
}

#[derive(Debug, Clone, Serialize)]
pub struct Session { pub start_unix: u64, pub end_unix: Option<u64>, pub lit: u32, pub by_schedule: bool, pub power_off: bool }

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SchedStatus { pub enabled: bool, pub start: String, pub end: String, pub in_window: bool, pub until_min: Option<u32>, pub duration_min: Option<u32>, pub skipped: bool }

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
    countdown: Option<(Instant, u32)>,
    power_off: bool,
    overlay_on: bool,
    power_off_at: Option<Instant>,
    reblank_at: Option<Instant>,
    hook_ok: bool,
    session: Option<Session>,
    last_hint: Option<Instant>,
    last_sched_check: Option<Instant>,
    last_cfg_check: Option<Instant>,
    last_status: Option<Status>,
    kbd_available: bool,
    displays: usize,
    last_mon_check: Option<Instant>,
    keep_display: bool,
}

fn unix_now() -> u64 { SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) }

impl Engine {
    pub fn new(cfg: Config, cfg_path: PathBuf, mut plat: Native) -> Self {
        plat.configure(&cfg);
        let kbd_available = plat.capabilities().iter().any(|c| c.id == "kbd" && c.status == "ok");
        let mtime = std::fs::metadata(&cfg_path).and_then(|m| m.modified()).ok();
        Self {
            sm: StateMachine::new(cfg.wake.lock_on_wake), policy: WakePolicy::new(&cfg), plat, cfg, cfg_path,
            cfg_mtime: mtime, last_session: None, tracker: Tracker::default(), countdown: None, power_off: false,
            overlay_on: false, power_off_at: None, reblank_at: None, hook_ok: true, session: None, last_hint: None,
            last_sched_check: None, last_cfg_check: None, last_status: None, kbd_available,
            displays: 1, last_mon_check: None, keep_display: false,
        }
    }

    pub fn marker_path(&self) -> PathBuf { self.cfg_path.with_file_name("blanking.state") }

    /// 配置变化（设置页保存或外部编辑 config.toml）
    pub fn set_config(&mut self, cfg: Config) {
        self.plat.configure(&cfg);
        self.policy.configure(&cfg);
        self.cfg = cfg;
    }

    pub fn save_config(&mut self, cfg: Config) -> Result<(), String> {
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
                    r
                }
                Action::InputHookOff => self.plat.input_hook(false),
                Action::LockScreen => self.plat.lock_screen(),
            };
            if let Err(e) = r { eprintln!("[nightblank] {a:?} failed: {e}"); }
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
        let c = &self.cfg;
        // 「只关所选」时只能用遮罩（系统级关屏会把所有屏一起关掉）
        self.power_off = c.display.mode != "selected" && match c.display.method {
            DisplayMethod::PowerOff => true,
            DisplayMethod::Overlay => false,
            DisplayMethod::Auto => !self.plat.power_off_unsafe(),
        };
        self.sm.lock_on_wake = c.wake.lock_on_wake || c.wake.method == WakeMethod::Password;
        self.policy.reset();
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

    fn wake(&mut self, app: &AppHandle) {
        if self.sm.state == State::Armed { return self.cancel(app); }
        if self.sm.state != State::ScreenOff { return; }
        self.apply(app, Event::WakeRequested);
        if self.keep_display { self.keep_display = false; let _ = self.plat.keep_display_on(false); }
        self.apply(app, Event::RestoreDone);
        let _ = std::fs::remove_file(self.marker_path());
        if let Some(mut s) = self.session.take() {
            // 时段内被手动唤醒：本时段不再自动息屏
            let (wd, min) = platform::local_time();
            if schedule::in_window(&self.cfg.schedule, wd, min) { self.tracker.suppress(); }
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
                Verdict::Wake => self.wake(app),
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
            Msg::OverlayInput if !self.hook_ok => self.wake(app),
            Msg::OverlayInput => {}
            Msg::Blank { delay_s } => self.arm(app, delay_s, false),
            Msg::Cancel => self.cancel(app),
            Msg::Wake => self.wake(app),
            Msg::SkipTonight => self.tracker.suppress(),
            Msg::Quit => {
                self.shutdown(app);
                app.exit(0);
            }
        }
    }

    /// 退出前恢复一切
    pub fn shutdown(&mut self, app: &AppHandle) {
        self.wake(app);
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
            if now >= at { self.power_off_at = None; let _ = self.plat.display_off(true); }
        }
        if self.sm.state == State::ScreenOff {
            match self.policy.tick(platform::now_ms()) {
                Some(Verdict::Wake) => self.wake(app),
                Some(Verdict::Holding(p)) => {
                    self.reblank_at = None;
                    self.hint(app, serde_json::json!({ "kind": "hold", "progress": p }), false)
                }
                _ => {}
            }
        }
        if self.sm.state == State::ScreenOff && self.power_off && self.power_off_at.is_none() {
            let lit = self.plat.display_asleep() == Some(false);
            if lit && !self.hook_ok {
                // 没有输入钩子（例如 Mac 没给辅助功能权限）：无法分辨误触，屏幕亮了就算唤醒
                self.wake(app);
            } else if lit && self.reblank_at.is_none() && !self.policy.is_holding() {
                self.reblank_at = Some(now + Duration::from_secs(self.cfg.wake.reblank_after_s.max(1) as u64));
            }
            if let Some(at) = self.reblank_at {
                if now >= at {
                    self.reblank_at = None;
                    if self.plat.display_asleep() != Some(true) {
                        if let Some(s) = self.session.as_mut() { s.lit += 1; }
                        self.apply(app, Event::ScreenLitUnexpectedly);
                    }
                }
            }
        }
        if self.last_sched_check.is_none_or(|t| now - t >= Duration::from_secs(1)) {
            self.last_sched_check = Some(now);
            let (wd, min) = platform::local_time();
            match self.tracker.update(&self.cfg.schedule, wd, min) {
                // TODO: skip_when_fullscreen_video 需要平台检测全屏视频；remind_before_min 需要系统通知
                Some(Edge::Start) if self.sm.state == State::Idle => self.arm(app, 0, true),
                Some(Edge::End) if self.session.as_ref().is_some_and(|s| s.by_schedule) => self.wake(app),
                _ => {}
            }
        }
        if self.last_cfg_check.is_none_or(|t| now - t >= Duration::from_secs(2)) {
            self.last_cfg_check = Some(now);
            let m = std::fs::metadata(&self.cfg_path).and_then(|m| m.modified()).ok();
            if m.is_some() && m != self.cfg_mtime {
                self.cfg_mtime = m;
                if let Some(c) = std::fs::read_to_string(&self.cfg_path).ok().and_then(|s| Config::from_toml(&s).ok()) {
                    self.set_config(c);
                    let _ = app.emit("nb://config", ());
                }
            }
        }
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

    pub fn status(&self) -> Status {
        let (wd, min) = platform::local_time();
        let s = &self.cfg.schedule;
        let (left, total) = self.countdown.map_or((0, 0), |(at, t)| {
            (at.saturating_duration_since(Instant::now()).as_secs_f32().ceil() as u32, t)
        });
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
                in_window: schedule::in_window(s, wd, min),
                until_min: schedule::minutes_until_start(s, wd, min),
                duration_min: schedule::duration_min(s),
                skipped: self.tracker.suppressed(),
            },
            displays: self.displays,
            keyboard: self.kbd_available && self.cfg.lights.keyboard_backlight,
            power_off: self.power_off,
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
                .title("NightBlank")
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
    m.name().cloned().filter(|n| !n.is_empty()).unwrap_or_else(|| format!("显示器 {}", i + 1))
}

fn hide_overlays(app: &AppHandle) {
    for (label, w) in app.webview_windows() {
        if label.starts_with("overlay-") { let _ = w.destroy(); }
    }
}
