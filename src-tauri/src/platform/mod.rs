//! 平台抽象。每个平台实现 `Platform`，核心状态机输出的 Action 由此执行。
//! 输入钩子在自己的线程里运行，通过 `emit_input` 把翻译好的事件送进引擎线程。
use std::sync::OnceLock;
use std::time::Instant;

use blankrun_core::{wake::Input, Config};
use serde::Serialize;

pub type Result<T = ()> = std::result::Result<T, String>;

/// 诊断页「这台电脑能做到什么」里的一行
#[derive(Debug, Clone, Serialize)]
pub struct Cap { pub id: &'static str, pub label: &'static str, pub detail: String, pub status: &'static str /* ok|warn|bad|off */, pub text: String }

impl Cap {
    pub fn new(id: &'static str, label: &'static str, detail: impl Into<String>, status: &'static str, text: impl Into<String>) -> Self {
        Self { id, label, detail: detail.into(), status, text: text.into() }
    }
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
pub use macos::{emergency_restore, local_time, raise_overlay, reveal, Native};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use self::windows::{emergency_restore, local_time, reveal, Native};

#[cfg(not(any(target_os = "macos", windows)))]
mod stub;
#[cfg(not(any(target_os = "macos", windows)))]
pub use stub::{emergency_restore, local_time, reveal, Native};

pub const OS: &str = if cfg!(target_os = "macos") { "mac" } else if cfg!(windows) { "win" } else { "linux" };
