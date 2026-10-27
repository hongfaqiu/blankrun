//! 引擎：唯一调用平台代码的线程。状态机 + 倒计时 + 定时计划 + 唤醒策略 + 遮罩窗口 + 意外点亮后重新息屏。
//! Windows 的 SetThreadExecutionState 按线程生效，所以所有平台动作都必须在这个线程里执行；
//! Tauri 命令只把请求丢进通道。
use std::path::PathBuf;
use std::sync::{mpsc::{Receiver, RecvTimeoutError}, Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use blankrun_core::history::History;
use blankrun_core::monitors::{self, Method};
use blankrun_core::{
    config::WakeMethod,
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
pub struct Session { pub start_unix: u64, pub end_unix: Option<u64>, pub lit: u32, pub touches: u32, pub by_schedule: bool, pub trigger: &'static str, pub power_off: bool }

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SchedStatus {
    /// start / end / duration_min：正在进行或下一段（多组定时重叠、相接时合并后的）时段
    pub enabled: bool, pub start: String, pub end: String, pub in_window: bool, pub until_min: Option<u32>, pub duration_min: Option<u32>, pub skipped: bool,
    /// 到点后在等什么："postponed"（推迟中）/ "fullscreen"（全屏视频）
    pub waiting: Option<&'static str>,
    /// 推迟还剩几分钟
    pub postponed_min: Option<u32>,
    /// 这段时段开始那天（0=周一）和名称（取开始它的那组定时）
    pub weekday: Option<u8>,
    pub label: Option<String>,
    /// 今天哪些分钟在时段内 [(开始, 结束)]，画时间轴用
    pub today: Vec<(u16, u16)>,
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
    /// 自动更新状态
    pub update: crate::update::UpdState,
    /// 现在是否在阻止系统睡眠
    pub keep_awake: bool,
}

pub struct Engine {
    pub sm: StateMachine,
    pub plat: Native,
    pub cfg: Config,
    pub cfg_path: PathBuf,
    pub cfg_mtime: Option<SystemTime>,
    /// 已经应用到窗口的外观（general.theme）
    theme_applied: Option<String>,
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
    /// 倒计时 / 息屏中：按设置决定要不要防睡眠
    awake_wanted: bool,
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
    /// 这次息屏是谁触发的：manual / hotkey / schedule
    trigger: &'static str,
    pub history: History,
    last_heartbeat: Option<Instant>,
    /// 本次息屏要盖遮罩的屏（show_overlays 的编号）
    overlay_idx: Vec<usize>,
    /// 本次息屏要单独断电的屏 (id, x, y)
    mon_off_plan: Vec<(String, f64, f64)>,
    /// 已经单独断电的屏
    mon_off: Vec<(String, f64, f64)>,
    last_mon_reassert: Option<Instant>,
    /// 本次息屏里每块屏被重新点亮的次数
    mon_relit: std::collections::HashMap<String, u32>,
    /// 本次息屏里内建屏没能断开的原因码（core::monitors::notice / "came_back" / "disconnect_failed"），设置页显示
    mon_notes: std::collections::HashMap<String, &'static str>,
    /// 内建屏被 macOS 重新打开的时间记录（10 秒内两次就不再对抗）
    came_back: std::collections::HashMap<String, monitors::CameBack>,
    /// 已经放弃断开的屏：id → 当时的显示器组合签名（组合变了就再试一次）
    gave_up: std::collections::HashMap<String, String>,
    /// 本次息屏 id → 显示名（no_ddc / prefs 按显示名记）
    mon_labels: std::collections::HashMap<String, String>,
    /// 本次息屏 id → show_overlays 编号
    mon_idx: std::collections::HashMap<String, usize>,
    /// 远程模式下已确认断电、撤掉了遮罩的屏
    uncovered: std::collections::HashSet<String>,
    /// 本次息屏 id → 单独断电方式（"ddc" | "backlight"）
    mon_power: std::collections::HashMap<String, &'static str>,
    /// 本次要「断开」的内建屏（Mac）：不盖遮罩（断开后遮罩窗口会被系统挪到别的屏上）；断开失败才补遮罩
    disconnecting: std::collections::HashSet<String>,
    /// 本次息屏要关的屏（plan 里不是 Skip 的）id：断开内建屏后屏幕坐标会变，光标围栏按 id 重算
    blank_ids: std::collections::HashSet<String>,
    /// 本次息屏是「键鼠照常用」（Mac Mini 模式）：键鼠放行、不算误触，只认唤醒键 / 紧急键 / 菜单 / 计划结束
    keep_input: bool,
    rdp_logged: bool,
}

fn unix_now() -> u64 { SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) }

impl Engine {
    pub fn new(cfg: Config, cfg_path: PathBuf, mut plat: Native) -> Self {
        plat.configure(&cfg);
        crate::update::set_enabled(cfg.general.check_updates);
        let kbd_available = plat.capabilities().iter().any(|c| c.id == "kbd" && c.status == "ok");
        let mtime = std::fs::metadata(&cfg_path).and_then(|m| m.modified()).ok();
        let mut e = Self {
            sm: StateMachine::new(cfg.wake.lock_on_wake), policy: WakePolicy::new(&cfg), plat, cfg, cfg_path,
            cfg_mtime: mtime, theme_applied: None, last_session: None, tracker: Tracker::default(), reminder: Reminder::default(), remind_open: false, gate: Gate::default(), fullscreen: (None, false),
            hotkey_dirty: true, fullscreen_logged: false, login: None, countdown: None, power_off: false,
            overlay_on: false, power_off_at: None, reblank_at: None, hook_ok: true, wake_keys: false, awake_wanted: false, session: None, last_hint: None,
            last_sched_check: None, last_cfg_check: None, last_status: None, kbd_available,
            displays: 1, last_mon_check: None, keep_display: false, last_asleep: None, settle_until: None,
            trigger: "manual", history: History::default(), last_heartbeat: None,
            overlay_idx: vec![], mon_off_plan: vec![], mon_off: vec![], last_mon_reassert: None, mon_relit: Default::default(), mon_notes: Default::default(), came_back: Default::default(), gave_up: Default::default(), mon_labels: Default::default(), mon_idx: Default::default(), uncovered: Default::default(), mon_power: Default::default(), disconnecting: Default::default(), blank_ids: Default::default(), keep_input: false, rdp_logged: false,
        };
        // 息屏记录；上次息屏中途崩溃/强退的那一条记为 crash
        e.history = History::from_json(&std::fs::read_to_string(e.history_path()).unwrap_or_default());
        if e.history.recover() { nblog!("history: last session marked crash-recovered"); e.save_history(); }
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
    fn history_path(&self) -> PathBuf { self.cfg_path.with_file_name("history.json") }
    pub fn save_history(&self) {
        if let Err(e) = std::fs::write(self.history_path(), self.history.to_json()) { nblog!("history: write failed: {e}"); }
    }
    pub fn clear_history(&mut self) { self.history.clear(); self.save_history(); nblog!("history cleared"); }
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
        if a.entries != b.entries || a.enabled != b.enabled { self.gate.clear_postpone(); }
        // 息屏中改了「防睡眠」：马上生效
        let awake_changed = cfg.power != self.cfg.power;
        let login_changed = cfg.general.launch_at_login != self.cfg.general.launch_at_login;
        if cfg.general.blank_now_hotkey != self.cfg.general.blank_now_hotkey { self.hotkey_dirty = true; }
        self.plat.configure(&cfg);
        self.policy.configure(&cfg);
        crate::update::set_enabled(cfg.general.check_updates);
        self.cfg = cfg;
        if login_changed { self.sync_login(); }
        if awake_changed && self.awake_wanted {
            let on = self.cfg.power.keep_awake;
            match self.plat.keep_awake(on) { Ok(()) => nblog!("keep awake = {on} (changed while blank)"), Err(e) => nblog!("keep awake = {on} failed: {e}") }
        }
    }

    pub fn save_config(&mut self, cfg: Config) -> Result<(), String> {
        let mut cfg = cfg;
        // 设置页拿的是打开时的配置：别把息屏期间自动记下的「不响应断电」冲掉（除非用户把那台改成了「断电」）
        for l in &self.cfg.display.no_ddc {
            if !cfg.display.no_ddc.contains(l) && cfg.display.prefs.get(l).map(String::as_str) != Some("off") { cfg.display.no_ddc.push(l.clone()); }
        }
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
                Action::StartKeepAwake => { self.awake_wanted = true; self.plat.keep_awake(self.cfg.power.keep_awake) }
                Action::StopKeepAwake => { self.awake_wanted = false; self.plat.keep_awake(false) }
                Action::DisplayOff => {
                    if !self.overlay_on {
                        self.overlay_on = true;
                        show_overlays(app, &self.overlay_idx, self.keep_input);
                        // 等遮罩窗口出来再断电，免得新窗口把屏幕又点亮
                        if self.power_off || !self.mon_off_plan.is_empty() {
                            // 只有要断开的屏、没有遮罩要等：马上断开
                            let ms = if self.overlay_idx.is_empty() && !self.power_off { 50 } else { 600 };
                            self.power_off_at = Some(Instant::now() + Duration::from_millis(ms));
                        }
                        Ok(())
                    } else if self.power_off {
                        self.settle_until = Some(Instant::now() + Duration::from_secs(3));
                        self.plat.display_off(true)
                    } else { self.monitors_off(app); Ok(()) }
                }
                Action::DisplayRestore => {
                    self.overlay_on = false;
                    self.power_off_at = None;
                    self.reblank_at = None;
                    hide_overlays(app);
                    for (id, x, y) in std::mem::take(&mut self.mon_off) {
                        match self.plat.monitor_power(&id, x, y, false) { Ok(()) => nblog!("monitor {id}: power on ok"), Err(e) => nblog!("monitor {id}: restoring power failed: {e}") }
                    }
                    self.plat.display_off(false)
                }
                // 「键鼠照常用」时键盘还在用，不关键盘背光
                Action::BacklightOff if self.cfg.lights.keyboard_backlight && !self.keep_input => self.plat.keyboard_backlight(true),
                Action::BacklightOff => Ok(()),
                Action::BacklightRestore => self.plat.keyboard_backlight(false),
                Action::InputHookOn => {
                    let r = self.plat.input_hook(true);
                    self.hook_ok = r.is_ok();
                    // 钩子装不上（Mac 没给辅助功能权限）：唤醒键改用系统热键注册，误触点亮后仍能重新息屏。
                    // 「键鼠照常用」时没有钩子键鼠本来就放行，一样用系统热键认唤醒键（光标围栏则没有）
                    let method = if self.keep_input { blankrun_core::wake::keep_input_method(self.cfg.wake.method) } else { self.cfg.wake.method };
                    if r.is_err() && method == WakeMethod::Hotkey {
                        let keys = (self.cfg.wake.hotkey.clone(), self.cfg.safety.emergency_hotkey.clone());
                        let _ = app.run_on_main_thread(move || platform::apply_wake_keys(Some(keys)));
                        self.wake_keys = true;
                    }
                    r
                }
                Action::InputHookOff => {
                    if std::mem::take(&mut self.wake_keys) { let _ = app.run_on_main_thread(|| platform::apply_wake_keys(None)); }
                    let r = self.plat.input_hook(false);
                    // 排空（如果需要）已经开始，它在 KEEP_INPUT 之前判断，可以放心关掉
                    platform::set_keep_input(None, Default::default());
                    r
                }
                Action::LockScreen => self.plat.lock_screen(),
            };
            match r { Ok(()) => nblog!("{a:?} ok"), Err(e) => nblog!("{a:?} failed: {e}") }
        }
    }

    fn arm(&mut self, app: &AppHandle, delay_s: u32, trigger: &'static str) {
        // 远程桌面（RDP）会话里不息屏：遮罩和输入拦截会落到远程用户的画面上
        if self.plat.remote_desktop() {
            if !self.rdp_logged { self.rdp_logged = true; nblog!("remote desktop session: not blanking ({trigger})"); }
            return;
        }
        match self.sm.state {
            State::Idle => self.apply(app, Event::Arm),
            State::Armed => {}
            _ => return,
        }
        self.trigger = trigger;
        if delay_s == 0 { self.countdown = None; self.do_blank(app); }
        else { self.countdown = Some((Instant::now() + Duration::from_secs(delay_s as u64), delay_s)); }
    }

    fn do_blank(&mut self, app: &AppHandle) {
        let (trigger, by_schedule) = (self.trigger, self.trigger == "schedule");
        hide_reminder(app);
        // 每块屏怎么关：系统级断电 / 单独断电（DDC/CI、内建屏背光）/ 遮罩 / 不管（虚拟屏、没选中的）
        let mons = self.monitor_list(app);
        let list: Vec<_> = mons.iter().map(|(m, _, _)| m.clone()).collect();
        let mut plan = monitors::plan(&self.cfg.display, &list, !self.plat.power_off_unsafe(), self.cfg.wake.allow_remote);
        let notes = self.plan_notes(&list, &mut plan);
        // 「键鼠照常用」：只关所选、还有屏亮着。绝不用系统级关屏（会把亮着的外接屏也关掉）
        self.keep_input = monitors::keep_input_active(&self.cfg.display, &list);
        self.power_off = plan.global_off && !self.keep_input;
        self.mon_power = list.iter().filter_map(|m| Some((m.id.clone(), m.power?))).collect();
        self.mon_relit.clear();
        self.came_back.clear();
        self.mon_notes = notes;
        for (m, how) in list.iter().zip(&plan.per) {
            if let Some(code) = self.mon_notes.get(&m.id) { nblog!("notice: {} ({}) {how:?}: {}", m.id, m.label, monitors::notice_text(code)); }
        }
        self.mon_labels = mons.iter().map(|(m, _, _)| (m.id.clone(), m.label.clone())).collect();
        self.mon_idx = mons.iter().enumerate().map(|(i, (m, _, _))| (m.id.clone(), i)).collect();
        self.uncovered.clear();
        self.disconnecting = list.iter().zip(&plan.per).filter(|(m, how)| **how == Method::Off && monitors::disconnect_ok(m, &list)).map(|(m, _)| m.id.clone()).collect();
        self.blank_ids = list.iter().zip(&plan.per).filter(|(_, how)| **how != Method::Skip).map(|(m, _)| m.id.clone()).collect();
        self.overlay_idx = plan.per.iter().zip(&list).enumerate().filter(|(_, (how, m))| **how != Method::Skip && !self.disconnecting.contains(&m.id)).map(|(i, _)| i).collect();
        self.mon_off_plan = plan.per.iter().zip(&mons).filter(|(m, _)| **m == Method::Off).map(|(_, (m, x, y))| (m.id.clone(), *x, *y)).collect();
        for ((m, _, _), how) in mons.iter().zip(&plan.per) { nblog!("plan: {} ({}) virtual={} power={:?} -> {how:?}", m.id, m.label, m.virtual_, m.power); }
        self.sm.lock_on_wake = self.cfg.wake.lock_on_wake || self.cfg.wake.method == WakeMethod::Password;
        self.policy.set_keep_input(self.keep_input);
        if self.keep_input {
            let fence = self.fence_rects(app);
            nblog!("keep input: local keyboard/mouse pass through; only wake / emergency keys are caught; cursor fence {} blocked / {} allowed", fence.0.len(), fence.1.len());
            platform::set_keep_input(Some(blankrun_core::wake::KeepFilter::from_config(&self.cfg)), fence);
        } else {
            platform::set_keep_input(None, Default::default());
        }
        let c = &self.cfg;
        nblog!("blank: power_off={} per_monitor_off={} method={:?} mode={} wake={:?} lock={} remote={} keep_input={}", self.power_off, self.mon_off_plan.len(), c.display.method, c.display.mode, c.wake.method, self.sm.lock_on_wake, c.wake.allow_remote, self.keep_input);
        self.apply(app, Event::Blank);
        // Modern Standby 上只用遮罩时，屏幕要保持点亮（黑遮罩），否则系统空闲关屏后会进入 S0 低功耗待机
        self.keep_display = !self.power_off && self.plat.power_off_unsafe();
        if self.keep_display { let _ = self.plat.keep_display_on(true); }
        // 崩溃标记：下次启动据此还原背光等
        let _ = std::fs::write(self.marker_path(), self.plat.persist().unwrap_or_default());
        let now = unix_now();
        self.session = Some(Session { start_unix: now, end_unix: None, lit: 0, touches: 0, by_schedule, trigger, power_off: self.power_off });
        self.history.begin(now, trigger);
        self.last_heartbeat = Some(Instant::now());
        self.save_history();
    }

    /// 光标围栏：(关掉的屏, 不关的屏) 在输入钩子坐标系里的矩形（Mac 全局点坐标，Windows 物理像素）
    /// 按 id 匹配当前的显示器（断开内建屏后编号和坐标都会变）；断开了的屏不在列表里，光标本来也去不了
    fn fence_rects(&mut self, app: &AppHandle) -> (Vec<platform::Rect>, Vec<platform::Rect>) {
        let mons = app.available_monitors().unwrap_or_default();
        let (mut blocked, mut allowed) = (vec![], vec![]);
        for (i, m) in mons.iter().enumerate() {
            let skip = !self.blank_ids.contains(&monitor_name(i, m));
            let (p, sz, sc) = (m.position(), m.size(), m.scale_factor().max(0.1));
            // Windows 的低级鼠标钩子和 SetCursorPos 用物理像素；Mac 换算成点，再让平台层按 CGDisplayBounds 校正
            let k = if cfg!(windows) { 1.0 } else { sc };
            let r = platform::Rect { x: p.x as f64 / k, y: p.y as f64 / k, w: sz.width as f64 / k, h: sz.height as f64 / k };
            let r = self.plat.display_rect(p.x as f64 / sc, p.y as f64 / sc, r);
            if skip { allowed.push(r) } else { blocked.push(r) }
        }
        (blocked, allowed)
    }

    /// 当前所有显示器 + 平台探测结果；(x, y) 是逻辑坐标
    pub fn monitor_list(&mut self, app: &AppHandle) -> Vec<(monitors::Monitor, f64, f64)> {
        let primary = app.primary_monitor().ok().flatten().map(|m| *m.position());
        app.available_monitors().unwrap_or_default().iter().enumerate().map(|(i, m)| {
            let id = monitor_name(i, m);
            let sc = m.scale_factor().max(0.1);
            let (x, y) = (m.position().x as f64 / sc, m.position().y as f64 / sc);
            let hw = self.plat.probe_monitor(&id, x, y);
            (monitors::Monitor {
                label: hw.label.clone().unwrap_or_else(|| id.clone()), id, width: m.size().width, height: m.size().height,
                primary: primary == Some(*m.position()), virtual_: hw.virtual_, power: hw.power, note: hw.note,
            }, x, y)
        }).collect()
    }

    /// 显示器组合的签名（id + 是不是虚拟屏）：组合变了，之前「放弃对抗」的结论就不算数了
    fn mon_sig(list: &[monitors::Monitor]) -> String {
        list.iter().map(|m| format!("{}:{}", m.id, m.virtual_ as u8)).collect::<Vec<_>>().join("|")
    }

    /// 在 `plan()` 的结果上补两件事：之前被 macOS 反复接回、已经放弃断开的内建屏改成只盖遮罩；
    /// 给每块要处理的屏配上「为什么没法断开」的说明码（没有就不在表里）
    fn plan_notes(&self, list: &[monitors::Monitor], plan: &mut monitors::Plan) -> std::collections::HashMap<String, &'static str> {
        let sig = Self::mon_sig(list);
        let mut notes = std::collections::HashMap::new();
        for (m, how) in list.iter().zip(plan.per.iter_mut()) {
            let gave_up = m.power == Some("disconnect") && self.gave_up.get(&m.id) == Some(&sig);
            if *how == Method::Off && gave_up { *how = Method::Overlay; }
            if *how == Method::Skip { continue; }
            if let Some(code) = if gave_up { Some("came_back") } else { monitors::notice(m, list) } { notes.insert(m.id.clone(), code); }
        }
        notes
    }

    /// 设置页用：每块屏和它息屏时会怎么处理
    pub fn monitor_report(&mut self, app: &AppHandle) -> Vec<serde_json::Value> {
        let mons = self.monitor_list(app);
        let list: Vec<_> = mons.iter().map(|(m, _, _)| m.clone()).collect();
        let mut plan = monitors::plan(&self.cfg.display, &list, !self.plat.power_off_unsafe(), self.cfg.wake.allow_remote);
        let mut notes = self.plan_notes(&list, &mut plan);
        // 这次息屏中途才发现的原因（断开被系统拒绝等）
        if self.sm.state == State::ScreenOff { for (id, code) in &self.mon_notes { notes.insert(id.clone(), code); } }
        list.iter().zip(&plan.per).map(|(m, how)| {
            let how = match how {
                Method::Off => m.power.unwrap_or("overlay"), Method::Overlay if plan.global_off => "power", Method::Overlay => "overlay", Method::Skip => "skip" };
            serde_json::json!({ "id": m.id, "label": m.label, "width": m.width, "height": m.height, "primary": m.primary, "virtual": m.virtual_, "power": m.power, "how": how, "note": notes.get(&m.id),
                "pref": monitors::pref(&self.cfg.display, m), "unresponsive": monitors::unresponsive(&self.cfg.display, m) })
        }).collect()
    }

    /// 记住这台屏不响应 DDC/CI 断电，写进配置
    fn mark_unresponsive(&mut self, id: &str) {
        let label = self.mon_labels.get(id).cloned().unwrap_or_else(|| id.to_string());
        nblog!("monitor {id} ({label}): reads on after power-off twice, treating as overlay-only from now on");
        if self.cfg.display.no_ddc.contains(&label) { return; }
        let mut c = self.cfg.clone();
        c.display.no_ddc.push(label);
        c.display.prefs.retain(|k, v| !(c.display.no_ddc.contains(k) && v == "off"));
        if let Err(e) = self.save_config(c) { nblog!("saving no_ddc failed: {e}"); }
    }

    /// 单独断电计划里的屏（重复调用安全：已关的再关一次）
    fn monitors_off(&mut self, app: &AppHandle) {
        self.last_mon_reassert = Some(Instant::now());
        for (id, x, y) in self.mon_off_plan.clone() {
            match self.plat.monitor_power(&id, x, y, true) {
                Ok(()) => { nblog!("monitor {id}: power off ok"); if !self.mon_off.iter().any(|(i, _, _)| *i == id) { self.mon_off.push((id, x, y)); } }
                // 断开失败（亮度原样没动，绝不自动退到背光 0）：这块屏没盖遮罩，现在补上。没有断开过，编号还是息屏时的
                Err(e) if self.disconnecting.remove(&id) => {
                    nblog!("monitor {id}: disconnect failed: {e} (brightness untouched, adding overlay)");
                    self.mon_notes.insert(id.clone(), "disconnect_failed");
                    if self.overlay_on { if let Some(&i) = self.mon_idx.get(&id) { self.overlay_idx.push(i); show_overlays(app, &[i], self.keep_input); } }
                }
                Err(e) => nblog!("monitor {id}: power off failed: {e} (overlay stays)"),
            }
        }
        self.refence(app);
        // 内建屏原亮度要进崩溃恢复标记
        if !self.mon_off.is_empty() && self.sm.state == State::ScreenOff { let _ = std::fs::write(self.marker_path(), self.plat.persist().unwrap_or_default()); }
    }

    /// 「键鼠照常用」：按当前显示器重算光标围栏（断开内建屏后坐标会变）
    fn refence(&mut self, app: &AppHandle) {
        if !self.keep_input || self.sm.state != State::ScreenOff { return; }
        let fence = self.fence_rects(app);
        platform::set_keep_input(Some(blankrun_core::wake::KeepFilter::from_config(&self.cfg)), fence);
    }

    fn cancel(&mut self, app: &AppHandle) {
        if self.sm.state == State::Armed { self.countdown = None; self.apply(app, Event::Cancel); }
    }

    fn wake(&mut self, app: &AppHandle, why: &str) {
        let ended = match why {
            _ if self.policy.by_emergency && why == "input matched wake policy" => "emergency",
            "input matched wake policy" | "wake policy timer" => "wake",
            "schedule ended" => "schedule",
            "quit" => "quit",
            "requested" => "manual",
            _ => "no_hook",
        };
        self.policy.by_emergency = false;
        if self.sm.state == State::Armed { return self.cancel(app); }
        if self.sm.state != State::ScreenOff { return; }
        nblog!("wake: {why}");
        self.apply(app, Event::WakeRequested);
        self.keep_input = false;
        self.policy.set_keep_input(false);
        if self.keep_display { self.keep_display = false; let _ = self.plat.keep_display_on(false); }
        self.apply(app, Event::RestoreDone);
        let _ = std::fs::remove_file(self.marker_path());
        if let Some(mut s) = self.session.take() {
            // 时段内被手动唤醒：本时段不再自动息屏
            let (wd, min) = platform::local_time();
            if schedule::in_window(&self.cfg.schedule, wd, min) { self.skip(); }
            s.end_unix = Some(unix_now());
            self.history.finish(unix_now(), ended, s.touches, s.lit);
            self.save_history();
            self.last_session = Some(s);
        }
    }

    fn hint(&mut self, app: &AppHandle, mut payload: serde_json::Value, throttle: bool) {
        let now = Instant::now();
        if throttle && self.last_hint.is_some_and(|t| now - t < Duration::from_millis(400)) { return; }
        self.last_hint = Some(now);
        // 只在光标所在的那块屏上显示（光标被拦住不动，就是用户刚才看的那块）
        payload["target"] = cursor_overlay(app).map_or(serde_json::Value::Null, serde_json::Value::from);
        let _ = app.emit("nb://overlay", payload);
    }

    /// 误触后多久重新息屏：至少留够时间把提示看清楚
    fn reblank_delay(&self) -> u32 { self.cfg.wake.reblank_after_s.max(HINT_MIN_S) }

    /// 被碰到：显示提示；再碰会延长，而不是让提示闪烁
    fn touched(&mut self, app: &AppHandle) {
        if let Some(s) = self.session.as_mut() { s.touches += 1; }
        let s = self.reblank_delay();
        self.hint(app, serde_json::json!({ "kind": "hint", "reblank_s": s }), true);
        if self.power_off { self.reblank_at = Some(Instant::now() + Duration::from_secs(s as u64)); }
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
                // 「键鼠照常用」：正常使用的输入，不是误触
                Verdict::Ignored if self.keep_input => {}
                Verdict::Ignored => self.touched(app),
            },
            Msg::Input(_) => {}
            // 「键鼠照常用」时别的屏照常能用（菜单栏 / 托盘可以唤醒），点到遮罩不算唤醒
            Msg::OverlayInput if !self.hook_ok && !platform::wake_keys_ok() && !self.keep_input => self.wake(app, "overlay input (no hook)"),
            Msg::OverlayInput => {}
            Msg::Blank { delay_s } => self.arm(app, delay_s, "manual"),
            Msg::Cancel => self.cancel(app),
            Msg::Wake => self.wake(app, "requested"),
            Msg::SkipTonight => { self.skip(); hide_reminder(app); }
            Msg::Unskip => self.unskip(),
            Msg::Postpone { min } => {
                self.gate.postpone(platform::now_ms(), min as u64 * 60_000);
                nblog!("schedule: postponed {min} min");
                hide_reminder(app);
            }
            Msg::Hotkey if matches!(self.sm.state, State::Idle | State::Armed) => { nblog!("blank-now hotkey"); self.arm(app, 0, "hotkey") }
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
        // 息屏中每分钟记一次心跳：崩溃后下次启动能算出大概时长
        if self.sm.state == State::ScreenOff && self.last_heartbeat.map_or(true, |t| t.elapsed() >= Duration::from_secs(60)) {
            self.last_heartbeat = Some(Instant::now());
            if let Some(s) = self.session.as_ref() { let (t, l) = (s.touches, s.lit); self.history.touch(unix_now(), t, l); self.save_history(); }
        }
        let now = Instant::now();
        if let Some((at, _)) = self.countdown {
            if now >= at { self.countdown = None; self.do_blank(app); }
        }
        if let Some(at) = self.power_off_at {
            if now >= at {
                self.power_off_at = None;
                self.settle_until = Some(now + Duration::from_secs(3));
                if self.power_off {
                    match self.plat.display_off(true) { Ok(()) => nblog!("display power off ok"), Err(e) => nblog!("display power off failed: {e}") }
                }
                self.monitors_off(app);
            }
        }
        // 断开了内建屏、外接屏又被拔掉：macOS 不会自己把内建屏接回来，我们接回并唤醒
        if self.sm.state == State::ScreenOff && self.plat.display_guard() { self.wake(app, "no other display left"); return; }
        if self.sm.state == State::ScreenOff && self.last_mon_reassert.is_none_or(|t| now - t >= Duration::from_secs(5)) {
            self.last_mon_reassert = Some(now);
            self.refence(app);
            if self.plat.remote_desktop() { self.wake(app, "remote desktop session started"); return; }
            // 单独断电的屏被远程会话 / 信号变化重新点亮：再关一次
            if !self.policy.is_holding() {
                for (id, x, y) in self.mon_off.clone() {
                    if self.mon_relit.get(&id).copied().unwrap_or(0) >= 2 { continue; }
                    let lit = self.plat.monitor_lit(&id, x, y);
                    let idx = self.mon_idx.get(&id).copied();
                    // 断开的内建屏被系统接了回来（休眠唤醒、开合盖）：再断开一次，不计入「不响应断电」。
                    // 显示器编号这时已经变过，不碰遮罩。
                    // 10 秒内被接回来第二次：macOS 不让断开（比如只剩虚拟屏），不再对抗——内建屏保持原样、亮度不动，盖上遮罩并提示
                    if self.disconnecting.contains(&id) {
                        if lit == Some(true) {
                            if self.came_back.entry(id.clone()).or_default().feed(platform::now_ms()) {
                                nblog!("monitor {id}: macOS re-enabled the display {} times within {}s, not fighting it any more (brightness untouched, overlay only)", monitors::RELIT_MAX, monitors::RELIT_WINDOW_MS / 1000);
                                // 让平台层忘掉「已断开」（屏本来就亮着；重连报 1001 也算成功）
                                if let Err(e) = self.plat.monitor_power(&id, x, y, false) { nblog!("monitor {id}: releasing the disconnect failed: {e}"); }
                                self.disconnecting.remove(&id);
                                self.mon_off.retain(|(i, _, _)| *i != id);
                                self.mon_off_plan.retain(|(i, _, _)| *i != id);
                                self.mon_notes.insert(id.clone(), "came_back");
                                let sig = Self::mon_sig(&self.monitor_list(app).into_iter().map(|(m, _, _)| m).collect::<Vec<_>>());
                                self.gave_up.insert(id.clone(), sig);
                                if let Some(i) = idx { if !self.overlay_idx.contains(&i) { self.overlay_idx.push(i); } show_overlays(app, &[i], self.keep_input); }
                                let _ = app.emit("nb://config", ());
                            } else {
                                nblog!("monitor {id}: display came back, disconnecting it again");
                                if let Err(e) = self.plat.monitor_power(&id, x, y, true) {
                                    // 亮度没动；屏又回来了，编号也就回到了息屏时的样子，补上遮罩
                                    nblog!("monitor {id}: disconnect again failed: {e} (brightness untouched, adding overlay)");
                                    self.disconnecting.remove(&id);
                                    self.mon_off.retain(|(i, _, _)| *i != id);
                                    self.mon_notes.insert(id.clone(), "disconnect_failed");
                                    if let Some(i) = idx { show_overlays(app, &[i], self.keep_input); }
                                }
                            }
                        }
                        continue;
                    }
                    // 内建屏背光被亮度键 / 自动亮度调了上来：直接再调回 0，不计入「不响应断电」
                    if lit == Some(true) && self.mon_power.get(&id) == Some(&"backlight") {
                        nblog!("monitor {id}: backlight came back, turning it off again");
                        let _ = self.plat.monitor_power(&id, x, y, true);
                        continue;
                    }
                    // 远程模式：确认这块屏真的断电了，就撤掉它的遮罩——远程软件抓的是显卡画面，遮罩在它眼里也是黑的
                    // （「键鼠照常用」时留着遮罩：背光万一漏光也还是黑的）
                    if lit == Some(false) && self.cfg.wake.allow_remote && !self.keep_input && !self.uncovered.contains(&id) {
                        if let Some(i) = idx {
                            if let Some(w) = app.get_webview_window(&format!("overlay-{i}")) { let _ = w.destroy(); }
                            self.uncovered.insert(id.clone());
                            nblog!("monitor {id}: confirmed off, overlay removed so a remote view shows the desktop");
                        }
                    }
                    if lit == Some(true) {
                        // 被点亮了：先把遮罩盖回去
                        if self.uncovered.remove(&id) { if let Some(i) = idx { show_overlays(app, &[i], self.keep_input); } }
                        let n = self.mon_relit.entry(id.clone()).or_insert(0);
                        *n += 1;
                        if *n >= 2 {
                            // 接受了 D6=4 却没真关（或有信号就自己醒）：记下来，以后只用遮罩，也不再发断电指令（免得闪）
                            self.mark_unresponsive(&id);
                            let _ = app.emit("nb://config", ());
                            continue;
                        }
                        nblog!("monitor {id}: lit again, powering off");
                        let _ = self.plat.monitor_power(&id, x, y, true);
                    }
                }
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
                if !self.hook_ok { nblog!("screen lit without the wake key (no input hook): re-blank in {}s", self.reblank_delay()); }
                // 没有钩子时这里是唯一知道「被碰到」的地方：一样显示提示
                if !self.hook_ok { self.touched(app); } else { self.reblank_at = Some(now + Duration::from_secs(self.reblank_delay() as u64)); }
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
        if self.theme_applied.as_deref() != Some(self.cfg.general.theme.as_str()) {
            // 原生窗口外观（Mac 上面板的毛玻璃底色、Windows 标题栏和 WebView 的配色）跟着设置走；
            // 页面里的颜色由 common.js 设 <html data-theme> 控制
            let t = self.cfg.general.theme.clone();
            app.set_theme(match t.as_str() { "light" => Some(tauri::Theme::Light), "dark" => Some(tauri::Theme::Dark), _ => None });
            nblog!("theme = {t}");
            self.theme_applied = Some(t);
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
                if self.sm.state == State::Idle && schedule::in_window(&self.cfg.schedule, wd, min) { nblog!("schedule: start"); self.arm(app, 0, "schedule"); }
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
                    else { std::fs::read_to_string(&self.cfg_path).ok().and_then(|s| Config::from_toml(&s).map_err(|e| nblog!("config.toml ignored: {e}")).ok()) };
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
        let focus = schedule::focus(s, wd, min);
        let in_win = focus.is_some_and(|(_, d)| d == 0);
        let until_min = focus.and_then(|(_, d)| (d > 0).then_some(d));
        let hm = |m: u16| format!("{:02}:{:02}", m / 60, m % 60);
        // 没有可用的时段（计划关着、都没选星期）：显示第一组的时间
        let (start, end) = match focus {
            Some((sp, _)) => (hm(sp.start_hm()), hm(sp.end_hm())),
            None => s.entries.iter().find(|e| e.enabled).or(s.entries.first())
                .map_or(("--:--".into(), "--:--".into()), |e| (e.start.clone(), e.end.clone())),
        };
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
                enabled: s.enabled, start, end,
                in_window: in_win,
                until_min,
                duration_min: focus.map(|(sp, _)| sp.len()),
                weekday: focus.map(|(sp, _)| sp.weekday()),
                label: focus.and_then(|(sp, _)| schedule::label_of(s, &sp)).map(str::to_string),
                // 计划关着也画出来（界面上变灰），让人看得到设的是什么
                today: schedule::day_segments(&blankrun_core::config::ScheduleConfig { enabled: true, ..s.clone() }, wd),
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
            update: crate::update::state(),
            keep_awake: self.awake_wanted && self.cfg.power.keep_awake,
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

/// 误触提示至少显示几秒
const HINT_MIN_S: u32 = 5;

/// 光标所在显示器对应的遮罩窗口（和 show_overlays 用同一个编号）
fn cursor_overlay(app: &AppHandle) -> Option<String> {
    let c = app.cursor_position().ok()?;
    let mons = app.available_monitors().ok()?;
    let i = mons.iter().position(|m| {
        let (p, s) = (m.position(), m.size());
        c.x >= p.x as f64 && c.y >= p.y as f64 && c.x < (p.x + s.width as i32) as f64 && c.y < (p.y + s.height as i32) as f64
    })?;
    Some(format!("overlay-{i}"))
}

/// `passive`：「键鼠照常用」时遮罩不能抢键盘焦点、也不接鼠标（点击穿透），免得打字落到遮罩上
fn show_overlays(app: &AppHandle, only: &[usize], passive: bool) {
    let Ok(monitors) = app.available_monitors() else { return };
    for (i, m) in monitors.into_iter().enumerate() {
        if !only.contains(&i) { continue; }
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
                .focusable(!passive)
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
            if passive { let _ = w.set_ignore_cursor_events(true); }
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
