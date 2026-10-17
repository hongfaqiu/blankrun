//! 平台抽象。每个平台实现 `Platform`，核心状态机输出的 Action 由此执行。
//! 输入钩子在自己的线程里运行，通过 `emit_input` 把翻译好的事件送进引擎线程。
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::time::Instant;

use blankrun_core::{wake::Input, Config};
use serde::Serialize;

pub type Result<T = ()> = std::result::Result<T, String>;

/// 诊断页「这台电脑能做到什么」里的一行。
/// 文案不在这里：界面按 `key` 去 ui/i18n/<语言>.js 里查 `cap.<key>.name / .state / .note`；
/// `tech` 只放接口名和原始错误（不翻译）。
#[derive(Debug, Clone, Serialize)]
pub struct Cap { pub id: &'static str, pub key: &'static str, pub status: &'static str /* ok|warn|bad|off */, pub tech: String }

impl Cap {
    pub fn new(id: &'static str, key: &'static str, status: &'static str, tech: impl Into<String>) -> Self {
        Self { id, key, status, tech: tech.into() }
    }
}

/// 平台探测到的一块显示器的硬件信息（engine 再配上尺寸/主屏，交给 core::monitors::plan）
#[derive(Debug, Clone, Default)]
pub struct MonHw {
    pub label: Option<String>,
    pub virtual_: bool,
    /// "ddc" | "backlight" | None
    pub power: Option<&'static str>,
    /// 诊断用：显卡/驱动描述
    #[allow(dead_code)]
    pub detail: String,
}

pub trait Platform {
    /// 配置变化时调用（例如 Mac 的 ac_only）
    fn configure(&mut self, _cfg: &Config) {}
    /// 防系统睡眠但允许屏幕关闭
    fn keep_awake(&mut self, on: bool) -> Result;
    /// 在防睡眠的同时保持屏幕点亮（Windows Modern Standby 上只用遮罩时需要）
    fn keep_display_on(&mut self, _on: bool) -> Result { Ok(()) }
    /// 真正关屏（断电）/ 恢复。遮罩由上层的 webview 窗口负责。
    fn display_off(&mut self, off: bool) -> Result;
    /// 主屏当前是否处于睡眠；None = 无法判断
    fn display_asleep(&self) -> Option<bool> { None }
    /// 关屏会不会让系统进入低功耗待机（Windows Modern Standby）。true 时自动模式只用遮罩
    fn power_off_unsafe(&self) -> bool { false }
    /// 关键盘背光（off=false 时恢复原值）；不支持时返回 Ok 并在 capabilities 里标出降级
    fn keyboard_backlight(&mut self, off: bool) -> Result;
    /// 安装/卸载全局输入钩子（吞掉事件 + 通过 emit_input 喂给 WakePolicy）
    fn input_hook(&mut self, on: bool) -> Result;
    /// 唤醒后锁屏（可选设置项）
    fn lock_screen(&mut self) -> Result;
    /// 诊断信息
    fn capabilities(&mut self) -> Vec<Cap>;
    /// 权限状态 (辅助功能, 输入监控)；不需要权限的平台返回 None
    fn permissions(&self) -> Option<(bool, bool)> { None }
    fn request_permission(&self, _which: &str) {}
    /// 息屏前要保存到磁盘的状态（崩溃后下次启动还原），例如键盘背光原值
    fn persist(&self) -> Option<String> { None }
    /// 启动时调用：上次异常退出留下的状态（RESEARCH.md §5 风险 4）
    fn restore_on_startup(&mut self, _saved: Option<&str>) -> Result { Ok(()) }
    /// 开机（登录）时启动：Mac 写 LaunchAgent，Windows 写 HKCU\…\Run
    fn set_launch_at_login(&mut self, _on: bool) -> Result { Err("unsupported".into()) }
    /// 当前是否已设为开机启动；None = 不支持
    fn launch_at_login(&self) -> Option<bool> { None }
    /// 前台有程序在全屏播放视频（或演示、全屏游戏）
    fn fullscreen_video(&self) -> bool { false }
    /// 探测一块显示器。id = tauri Monitor::name()；(x, y) = 它左上角的逻辑坐标（Mac 按坐标找 CGDisplay）
    fn probe_monitor(&mut self, _id: &str, _x: f64, _y: f64) -> MonHw { MonHw::default() }
    /// 单独关/开一块显示器（DDC/CI 或内建屏背光）
    fn monitor_power(&mut self, _id: &str, _x: f64, _y: f64, _off: bool) -> Result { Err("unsupported".into()) }
    /// 单独关掉的屏是不是又亮了（DDC 读 D6）；None = 不知道
    fn monitor_lit(&mut self, _id: &str, _x: f64, _y: f64) -> Option<bool> { None }
    /// 当前是远程桌面（RDP）会话：不去动显示器和输入
    fn remote_desktop(&self) -> bool { false }
}

// ---------- 「立即息屏」全局快捷键 ----------
type HkSink = Box<dyn Fn() + Send + Sync>;
static HK_SINK: OnceLock<HkSink> = OnceLock::new();
static HK_ERR: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
pub fn set_hotkey_sink(f: HkSink) { let _ = HK_SINK.set(f); }
/// 快捷键回调里调用
#[cfg_attr(not(any(target_os = "macos", windows)), allow(dead_code))]
pub fn emit_hotkey() { if let Some(f) = HK_SINK.get() { f() } }
/// 最近一次注册快捷键的错误（给设置页显示）
pub fn hotkey_error() -> Option<String> { HK_ERR.lock().ok()?.clone() }
/// 注册（或注销，hk=None）「立即息屏」快捷键。Mac 上必须在主线程调用。
pub fn apply_blank_hotkey(spec: &str) {
    let r = match spec.trim() {
        "" => set_blank_hotkey(None),
        s => match blankrun_core::hotkey::Hotkey::parse(s) {
            Some(h) if h.usable_global() => set_blank_hotkey(Some(&h)),
            Some(_) => Err("needs Ctrl, Alt or Cmd/Win".into()),
            None => Err(format!("invalid shortcut {s:?}")),
        },
    };
    match &r { Ok(()) => crate::nblog!("blank-now hotkey {spec:?} registered"), Err(e) => crate::nblog!("blank-now hotkey {spec:?}: {e}") }
    if let Ok(mut g) = HK_ERR.lock() { *g = r.err(); }
}

// ---------- 没有输入钩子时的唤醒兜底（Mac 未授权辅助功能） ----------
static WAKE_KEYS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// 唤醒快捷键已通过系统热键注册：屏幕被误触点亮时可以放心重新息屏
pub fn wake_keys_ok() -> bool { WAKE_KEYS.load(std::sync::atomic::Ordering::Relaxed) }
/// 注册（wake=Some）或注销唤醒键 + 紧急键。Mac 上必须在主线程调用。
pub fn apply_wake_keys(keys: Option<(String, String)>) {
    use blankrun_core::hotkey::Hotkey;
    let ok = match &keys {
        None => { let _ = set_wake_hotkeys(&[]); false }
        Some((wake, em)) => match Hotkey::parse(wake) {
            Some(w) => {
                let list: Vec<Hotkey> = std::iter::once(w).chain(Hotkey::parse(em)).collect();
                match set_wake_hotkeys(&list) {
                    // 第 1 个是唤醒键，必须成功
                    Ok(n) if n >= 1 => { crate::nblog!("no input hook: wake keys registered as system hot keys ({n}); accidental touches will re-blank"); true }
                    Ok(_) => { crate::nblog!("no input hook: could not register wake key {wake:?}"); false }
                    Err(e) => { crate::nblog!("no input hook: wake keys unavailable: {e}"); false }
                }
            }
            None => false,
        },
    };
    if ok { WAKE_KEYS.store(true, std::sync::atomic::Ordering::Relaxed); } else { WAKE_KEYS.store(false, std::sync::atomic::Ordering::Relaxed); if keys.is_some() { let _ = set_wake_hotkeys(&[]); } }
}

type Sink = Box<dyn Fn(Input) + Send + Sync>;
static SINK: OnceLock<Sink> = OnceLock::new();
static T0: OnceLock<Instant> = OnceLock::new();

pub fn set_input_sink(f: Sink) { let _ = SINK.set(f); now_ms(); }
/// 钩子回调里调用，必须很快返回
#[cfg_attr(not(any(target_os = "macos", windows)), allow(dead_code))]
pub fn emit_input(i: Input) { if let Some(f) = SINK.get() { f(i) } }
pub fn now_ms() -> u64 { T0.get_or_init(Instant::now).elapsed().as_millis() as u64 }

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::{set_blank_hotkey, set_wake_hotkeys, emergency_restore, instance_lock, local_time, notch_span, prefer_tray_position, raise_overlay, reveal, system_locale, Native};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
mod win_monitors;
#[cfg(windows)]
pub use self::windows::{set_blank_hotkey, set_wake_hotkeys, emergency_restore, instance_lock, local_time, reveal, system_locale, taskbar_light, Native};

#[cfg(not(any(target_os = "macos", windows)))]
mod stub;
#[cfg(not(any(target_os = "macos", windows)))]
pub use stub::{set_blank_hotkey, set_wake_hotkeys, emergency_restore, instance_lock, local_time, reveal, system_locale, Native};

pub const OS: &str = if cfg!(target_os = "macos") { "mac" } else if cfg!(windows) { "win" } else { "linux" };

// ---------- 唤醒后排空键盘（见 blankrun_core::drain） ----------
/// 排空中：键盘钩子只按 DRAIN 决定吞不吞，不再上报；鼠标直接放行
#[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
pub static DRAINING: AtomicBool = AtomicBool::new(false);
#[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
static DRAIN: std::sync::Mutex<Option<blankrun_core::drain::Drain>> = std::sync::Mutex::new(None);

/// 钩子回调里调用：None = 不在排空；Some(true) = 吞掉
#[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
pub fn drain_key(code: u32, down: bool) -> Option<bool> {
    if !DRAINING.load(Relaxed) { return None; }
    let mut g = DRAIN.lock().unwrap_or_else(|p| p.into_inner());
    Some(g.as_mut().is_some_and(|d| d.key(code, down, now_ms())))
}

/// 卸钩子前调用：唤醒时还按着键，就让钩子再活一会儿，排空后由后台线程调用 `finish` 卸掉。
/// 没有按着的键（或线程起不来）就当场调用 `finish`，返回 None。
#[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
pub fn start_drain(held: Vec<u32>, finish: Box<dyn FnOnce() + Send>) -> Option<std::thread::JoinHandle<()>> {
    let d = blankrun_core::drain::Drain::new(held.iter().copied(), now_ms());
    if !d.needed() { finish(); return None; }
    crate::nblog!("input: draining {} held key(s) before releasing the keyboard: {held:?}", held.len());
    *DRAIN.lock().unwrap_or_else(|p| p.into_inner()) = Some(d);
    DRAINING.store(true, Relaxed);
    let slot = std::sync::Arc::new(std::sync::Mutex::new(Some(finish)));
    let slot2 = slot.clone();
    let end = || {
        DRAINING.store(false, Relaxed);
        *DRAIN.lock().unwrap_or_else(|p| p.into_inner()) = None;
    };
    let h = std::thread::Builder::new().name("nb-drain".into()).spawn(move || {
        let t0 = now_ms();
        loop {
            std::thread::sleep(std::time::Duration::from_millis(20));
            let done = DRAIN.lock().unwrap_or_else(|p| p.into_inner()).as_ref().is_none_or(|d| d.done(now_ms()));
            if done { break; }
        }
        end();
        crate::nblog!("input: keyboard released after {} ms", now_ms() - t0);
        if let Some(f) = slot2.lock().unwrap_or_else(|p| p.into_inner()).take() { f(); }
    });
    match h {
        Ok(h) => Some(h),
        Err(_) => { end(); if let Some(f) = slot.lock().unwrap_or_else(|p| p.into_inner()).take() { f(); } None }
    }
}
