//! Windows 实现。见 RESEARCH.md §3。
//! - 防睡眠：SetThreadExecutionState(ES_SYSTEM_REQUIRED)。注意它按线程生效，引擎保证所有动作都在同一个引擎线程里调用。
//! - 关屏：SC_MONITORPOWER=2；Modern Standby（AoAc）机器上关屏会进 S0 低功耗待机，自动模式下改用遮罩（上层 webview）。
//! - 屏幕状态：PowerSettingRegisterNotification(GUID_CONSOLE_DISPLAY_STATE) 回调，不需要窗口。
//! - 输入：WH_KEYBOARD_LL / WH_MOUSE_LL，跑在独立的消息循环线程里，吞掉所有非注入事件。
//! - 崩溃：执行状态、钩子、遮罩都随进程消失，系统会自己恢复；启动时再补发一次"开屏"。
use std::sync::atomic::{AtomicU8, Ordering::Relaxed};
use std::thread::JoinHandle;

use blankrun_core::{hotkey, wake::Input};
use windows_sys::Win32::{
    Foundation::{LPARAM, LRESULT, POINT, WPARAM},
    System::{
        LibraryLoader::GetModuleHandleW,
        Power::*,
        Shutdown::LockWorkStation,
        SystemInformation::GetLocalTime,
        SystemServices::GUID_CONSOLE_DISPLAY_STATE,
        Threading::GetCurrentThreadId,
    },
    UI::{
        Input::KeyboardAndMouse::{SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_MOVE, MOUSEINPUT},
        WindowsAndMessaging::*,
    },
};

use super::{emit_input, now_ms, Cap, Platform, Result};

/// 255 = 未知，0 = 关，1 = 开，2 = 变暗
static DISPLAY_STATE: AtomicU8 = AtomicU8::new(255);
static MODS: AtomicU8 = AtomicU8::new(0);

#[derive(Default)]
pub struct Native {
    hook: Option<(u32, JoinHandle<()>)>,
    hook_err: Option<String>,
    power_notify: HPOWERNOTIFY,
    modern_standby: Option<bool>,
    awake_ok: Option<bool>,
}

impl Native {
    fn modern_standby(&mut self) -> bool { *self.modern_standby.get_or_insert_with(query_aoac) }
    fn watch_display(&mut self) {
        if self.power_notify != 0 { return; }
        let mut params = DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS { Callback: Some(on_power_setting), Context: std::ptr::null_mut() };
        let mut h: *mut core::ffi::c_void = std::ptr::null_mut();
        let r = unsafe {
            PowerSettingRegisterNotification(&GUID_CONSOLE_DISPLAY_STATE, DEVICE_NOTIFY_CALLBACK,
                &mut params as *mut _ as *mut _, &mut h as *mut _ as *mut _)
        };
        if r == 0 { self.power_notify = h as HPOWERNOTIFY; }
    }
}

/// Modern Standby（S0 低功耗待机，AoAc）检测
fn query_aoac() -> bool {
    unsafe {
        let mut caps: SYSTEM_POWER_CAPABILITIES = std::mem::zeroed();
        let st = CallNtPowerInformation(SystemPowerCapabilities, std::ptr::null(), 0,
            &mut caps as *mut _ as *mut _, std::mem::size_of::<SYSTEM_POWER_CAPABILITIES>() as u32);
        st == 0 && caps.AoAc != 0
    }
}

unsafe extern "system" fn on_power_setting(_ctx: *const core::ffi::c_void, ty: u32, setting: *const core::ffi::c_void) -> u32 {
    if ty == PBT_POWERSETTINGCHANGE && !setting.is_null() {
        let s = &*(setting as *const POWERBROADCAST_SETTING);
        if s.DataLength >= 4 {
            let v = std::ptr::read_unaligned(s.Data.as_ptr() as *const u32);
            DISPLAY_STATE.store(v.min(2) as u8, Relaxed);
        }
    }
    0
}

impl Drop for Native {
    fn drop(&mut self) {
        let _ = self.input_hook(false);
        if self.power_notify != 0 { unsafe { PowerSettingUnregisterNotification(self.power_notify); } }
    }
}

fn monitor_power(state: isize) {
    unsafe { PostMessageW(HWND_BROADCAST, WM_SYSCOMMAND, SC_MONITORPOWER as WPARAM, state as LPARAM); }
}

fn display_on_now() {
    monitor_power(-1);
    unsafe {
        // 只重置一次显示器空闲计时（不带 ES_CONTINUOUS，不影响防睡眠的持续状态）
        SetThreadExecutionState(ES_DISPLAY_REQUIRED);
        // 0 位移的注入鼠标事件：部分驱动只认输入才点亮
        let mi = MOUSEINPUT { dx: 0, dy: 0, mouseData: 0, dwFlags: MOUSEEVENTF_MOVE, time: 0, dwExtraInfo: 0 };
        let inp = INPUT { r#type: INPUT_MOUSE, Anonymous: INPUT_0 { mi } };
        SendInput(1, &inp, std::mem::size_of::<INPUT>() as i32);
    }
}

impl Platform for Native {
    fn keep_awake(&mut self, on: bool) -> Result {
        // 只在息屏期间持有 ES_SYSTEM_REQUIRED（不加 DISPLAY_REQUIRED）。
        // TODO: 实测 Modern Standby 下能否挡住 S0 低功耗待机（RESEARCH.md §5 风险 2）。
        let flags = if on { ES_CONTINUOUS | ES_SYSTEM_REQUIRED } else { ES_CONTINUOUS };
        let ok = unsafe { SetThreadExecutionState(flags) } != 0;
        if on { self.awake_ok = Some(ok); }
        if !ok { return Err("SetThreadExecutionState failed".into()); }
        Ok(())
    }

    fn keep_display_on(&mut self, on: bool) -> Result {
        let flags = ES_CONTINUOUS | ES_SYSTEM_REQUIRED | if on { ES_DISPLAY_REQUIRED } else { 0 };
        if unsafe { SetThreadExecutionState(flags) } == 0 { return Err("SetThreadExecutionState failed".into()); }
        Ok(())
    }

    fn display_off(&mut self, off: bool) -> Result {
        self.watch_display();
        if off { monitor_power(2) } else { display_on_now() }
        Ok(())
    }

    fn display_asleep(&self) -> Option<bool> {
        match DISPLAY_STATE.load(Relaxed) { 0 => Some(true), 1 | 2 => Some(false), _ => None }
    }

    fn power_off_unsafe(&self) -> bool { self.modern_standby.unwrap_or_else(query_aoac) }

    fn keyboard_backlight(&mut self, _off: bool) -> Result {
        // TODO(M2/M4): OpenRGB SDK、Dynamic Lighting、厂商背光插件。见 RESEARCH.md §3「RGB」。
        Ok(())
    }

    fn input_hook(&mut self, on: bool) -> Result {
        if on {
            if self.hook.is_some() { return Ok(()); }
            MODS.store(0, Relaxed);
            let (tx, rx) = std::sync::mpsc::channel();
            let h = std::thread::Builder::new().name("nb-hook".into()).spawn(move || hook_thread(tx)).map_err(|e| e.to_string())?;
            match rx.recv() {
                Ok(Ok(tid)) => { self.hook = Some((tid, h)); self.hook_err = None; Ok(()) }
                Ok(Err(e)) => { let _ = h.join(); self.hook_err = Some(e.clone()); Err(e) }
                Err(e) => { self.hook_err = Some(e.to_string()); Err(e.to_string()) }
            }
        } else {
            if let Some((tid, h)) = self.hook.take() {
                unsafe { PostThreadMessageW(tid, WM_QUIT, 0, 0); }
                let _ = h.join();
            }
            Ok(())
        }
    }

    fn lock_screen(&mut self) -> Result {
        if unsafe { LockWorkStation() } == 0 { return Err("LockWorkStation failed".into()); }
        Ok(())
    }

    fn capabilities(&mut self) -> Vec<Cap> {
        let ms = self.modern_standby();
        vec![
            match self.awake_ok {
                Some(false) => Cap::new("awake", "防止系统睡眠", "SetThreadExecutionState · ES_SYSTEM_REQUIRED", "bad", "调用失败"),
                _ => Cap::new("awake", "防止系统睡眠", "SetThreadExecutionState · ES_SYSTEM_REQUIRED", "ok", "正常"),
            },
            match &self.hook_err {
                Some(e) => Cap::new("hook", "输入钩子", format!("WH_KEYBOARD_LL / WH_MOUSE_LL · {e}"), "bad", "安装失败"),
                None => Cap::new("hook", "输入钩子", "WH_KEYBOARD_LL / WH_MOUSE_LL · 无需管理员权限", "ok", "正常"),
            },
            if ms {
                Cap::new("s0", "现代待机（S0）", "这台电脑关屏后可能自动进入低功耗状态，自动模式下改用遮罩息屏", "off", "已规避")
            } else {
                Cap::new("s0", "显示器断电", "SC_MONITORPOWER · 传统睡眠模式，可以安全关屏", "ok", "可用")
            },
            Cap::new("kbd", "键盘背光", "未识别到这台电脑的背光接口，厂商插件会在后续版本提供", "off", "不支持"),
            Cap::new("ddc", "显示器断电（DDC/CI）", "后续版本提供", "off", "未实现"),
            Cap::new("rgb", "OpenRGB", "后续版本提供", "off", "未实现"),
        ]
    }

    fn restore_on_startup(&mut self, saved: Option<&str>) -> Result {
        if saved.is_some() { display_on_now(); }
        Ok(())
    }
}

/// panic 钩子里调用：不加锁，只做无状态的恢复
pub fn emergency_restore() { monitor_power(-1); }

fn hook_thread(ready: std::sync::mpsc::Sender<std::result::Result<u32, String>>) {
    unsafe {
        let hmod = GetModuleHandleW(std::ptr::null());
        let kb = SetWindowsHookExW(WH_KEYBOARD_LL, Some(kb_proc), hmod, 0);
        let ms = SetWindowsHookExW(WH_MOUSE_LL, Some(ms_proc), hmod, 0);
        if kb.is_null() || ms.is_null() {
            if !kb.is_null() { UnhookWindowsHookEx(kb); }
            if !ms.is_null() { UnhookWindowsHookEx(ms); }
            let _ = ready.send(Err("SetWindowsHookEx failed".into()));
            return;
        }
        let _ = ready.send(Ok(GetCurrentThreadId()));
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {}
        UnhookWindowsHookEx(kb);
        UnhookWindowsHookEx(ms);
    }
}

const LETTERS: [&str; 26] = ["A","B","C","D","E","F","G","H","I","J","K","L","M","N","O","P","Q","R","S","T","U","V","W","X","Y","Z"];
const DIGITS: [&str; 10] = ["0","1","2","3","4","5","6","7","8","9"];
const FKEYS: [&str; 12] = ["F1","F2","F3","F4","F5","F6","F7","F8","F9","F10","F11","F12"];

fn vk_name(vk: u32) -> &'static str {
    match vk {
        0x41..=0x5A => LETTERS[(vk - 0x41) as usize],
        0x30..=0x39 => DIGITS[(vk - 0x30) as usize],
        0x70..=0x7B => FKEYS[(vk - 0x70) as usize],
        0x1B => "Esc", 0x20 => "Space", 0x0D => "Enter", 0x09 => "Tab", 0x08 => "Backspace",
        _ => "?",
    }
}

fn vk_mod(vk: u32) -> u8 {
    match vk {
        0x11 | 0xA2 | 0xA3 => hotkey::CTRL,
        0x12 | 0xA4 | 0xA5 => hotkey::ALT,
        0x10 | 0xA0 | 0xA1 => hotkey::SHIFT,
        0x5B | 0x5C => hotkey::META,
        _ => 0,
    }
}

unsafe extern "system" fn kb_proc(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code < 0 { return CallNextHookEx(std::ptr::null_mut(), code, wp, lp); }
    let k = &*(lp as *const KBDLLHOOKSTRUCT);
    if k.flags & LLKHF_INJECTED != 0 { return CallNextHookEx(std::ptr::null_mut(), code, wp, lp); }
    let down = wp == WM_KEYDOWN as WPARAM || wp == WM_SYSKEYDOWN as WPARAM;
    let m = vk_mod(k.vkCode);
    let key = if m != 0 {
        if down { MODS.fetch_or(m, Relaxed); } else { MODS.fetch_and(!m, Relaxed); }
        "Mod"
    } else { vk_name(k.vkCode) };
    emit_input(Input::Key { key, mods: MODS.load(Relaxed), down, t_ms: now_ms() });
    1 // 吞掉
}

unsafe extern "system" fn ms_proc(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code < 0 { return CallNextHookEx(std::ptr::null_mut(), code, wp, lp); }
    let m = &*(lp as *const MSLLHOOKSTRUCT);
    if m.flags & LLMHF_INJECTED != 0 { return CallNextHookEx(std::ptr::null_mut(), code, wp, lp); }
    match wp as u32 {
        WM_MOUSEMOVE => {
            // 移动被吞掉后光标不动，所以 pt 与当前光标位置之差就是这一次的位移
            let mut cur = POINT { x: 0, y: 0 };
            GetCursorPos(&mut cur);
            emit_input(Input::MouseMove { dx: (m.pt.x - cur.x) as f64, dy: (m.pt.y - cur.y) as f64, t_ms: now_ms() });
        }
        WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN => emit_input(Input::Click),
        WM_MOUSEWHEEL | WM_MOUSEHWHEEL => emit_input(Input::MouseMove { dx: 0.0, dy: 0.0, t_ms: now_ms() }),
        _ => {}
    }
    1
}

pub fn local_time() -> (u8, u16) {
    let mut st = unsafe { std::mem::zeroed() };
    unsafe { GetLocalTime(&mut st) };
    let st: windows_sys::Win32::Foundation::SYSTEMTIME = st;
    (((st.wDayOfWeek + 6) % 7) as u8, st.wHour * 60 + st.wMinute)
}

pub fn reveal(path: &std::path::Path) {
    let _ = std::process::Command::new("explorer").arg(format!("/select,{}", path.display())).spawn();
}
