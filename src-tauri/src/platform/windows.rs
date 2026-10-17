//! Windows 实现。见 RESEARCH.md §3。
//! - 防睡眠：SetThreadExecutionState(ES_SYSTEM_REQUIRED)。注意它按线程生效，引擎保证所有动作都在同一个引擎线程里调用。
//! - 关屏：SC_MONITORPOWER=2（只发给自己的隐藏窗口，不广播）；Modern Standby（AoAc）机器上关屏会进 S0 低功耗待机，自动模式下改用遮罩（上层 webview）。
//! - 屏幕状态：PowerSettingRegisterNotification(GUID_CONSOLE_DISPLAY_STATE) 回调，不需要窗口。
//! - 输入：WH_KEYBOARD_LL / WH_MOUSE_LL，跑在独立的消息循环线程里，吞掉所有非注入事件。
//! - 崩溃：执行状态、钩子、遮罩都随进程消失，系统会自己恢复；启动时再补发一次"开屏"。
use std::sync::atomic::{AtomicU8, Ordering::Relaxed};
use std::thread::JoinHandle;

use blankrun_core::{hotkey, wake::Input};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM},
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
/// 「放行远程控制」：注入的键鼠事件不拦、不上报
static ALLOW_INJECTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[derive(Default)]
pub struct Native {
    hook: Option<(u32, JoinHandle<()>)>,
    hook_err: Option<String>,
    power_notify: HPOWERNOTIFY,
    modern_standby: Option<bool>,
    awake_ok: Option<bool>,
    /// DDC 探测结果缓存（每块屏几十到几百毫秒，探一次就记住；显示器拔插后 engine 会清掉）
    mon_cache: std::collections::HashMap<String, super::MonHw>,
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

/// 关屏/开屏请求只发给我们自己的一个隐藏窗口，由它的 DefWindowProc 执行。
///
/// 不能 PostMessage(HWND_BROADCAST)：广播会塞进每个顶层窗口的消息队列，被挂起的 UWP 应用
/// （设置、ShellExperienceHost 等）当时不处理，等用户几分钟后切回它们才把积压的
/// SC_MONITORPOWER=2 交给 DefWindowProc，屏幕就在空闲时自己黑掉、还会黑亮来回闪。
fn monitor_power(state: isize) {
    let hwnd = power_window();
    if hwnd.is_null() { return; }
    unsafe { PostMessageW(hwnd, WM_SYSCOMMAND, SC_MONITORPOWER as WPARAM, state as LPARAM); }
}

/// 专用线程上的隐藏顶层窗口（不显示、不进任务栏），自带消息循环，所以不会被系统当成无响应，
/// 也能正常收别的广播。懒创建一次，任何线程都可以往里 PostMessage。
fn power_window() -> HWND {
    use std::sync::OnceLock;
    static HWND_CELL: OnceLock<usize> = OnceLock::new();
    *HWND_CELL.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<usize>();
        let spawned = std::thread::Builder::new().name("blankrun-power".into()).spawn(move || unsafe {
            let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
            let title: Vec<u16> = "BlankRun power\0".encode_utf16().collect();
            let hwnd = CreateWindowExW(WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE, class.as_ptr(), title.as_ptr(), WS_POPUP,
                0, 0, 0, 0, std::ptr::null_mut(), std::ptr::null_mut(), GetModuleHandleW(std::ptr::null()), std::ptr::null());
            let _ = tx.send(hwnd as usize);
            if hwnd.is_null() { return; }
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        });
        if spawned.is_err() { return 0; }
        rx.recv().unwrap_or(0)
    }) as HWND
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
            hook_reset();
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
                Some(false) => Cap::new("awake", "awake_bad", "bad", "SetThreadExecutionState · ES_SYSTEM_REQUIRED"),
                _ => Cap::new("awake", "awake_ok", "ok", "SetThreadExecutionState · ES_SYSTEM_REQUIRED"),
            },
            match &self.hook_err {
                Some(e) => Cap::new("hook", "hook_bad", "bad", format!("WH_KEYBOARD_LL / WH_MOUSE_LL · {e}")),
                None => Cap::new("hook", "hook_win_ok", "ok", "WH_KEYBOARD_LL / WH_MOUSE_LL"),
            },
            if ms {
                Cap::new("s0", "s0_standby", "off", "AoAc / Modern Standby")
            } else {
                Cap::new("s0", "s0_ok", "ok", "SC_MONITORPOWER")
            },
            Cap::new("kbd", "kbd_win_off", "off", "—"),
            Cap::new("ddc", "ddc_todo", "off", "DDC/CI"),
            Cap::new("rgb", "rgb_todo", "off", "OpenRGB SDK"),
        ]
    }

    fn restore_on_startup(&mut self, saved: Option<&str>) -> Result {
        if saved.is_some() { super::win_monitors::wake_all(); display_on_now(); }
        Ok(())
    }

    fn set_launch_at_login(&mut self, on: bool) -> Result {
        use windows_sys::Win32::System::Registry::{RegDeleteKeyValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ};
        let (key, name) = (wide(RUN_KEY), wide("BlankRun"));
        let r = if on {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            let v = wide(&format!("\"{}\" --autostart", exe.display()));
            unsafe { RegSetKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), REG_SZ, v.as_ptr() as *const _, (v.len() * 2) as u32) }
        } else {
            match unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr()) } { 2 => 0, r => r } // 2 = 本来就没有
        };
        if r == 0 { Ok(()) } else { Err(format!("registry error {r}")) }
    }

    fn launch_at_login(&self) -> Option<bool> {
        use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ};
        let (key, name) = (wide(RUN_KEY), wide("BlankRun"));
        let mut n = 0u32;
        let r = unsafe { RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), std::ptr::null_mut(), &mut n) };
        Some(r == 0)
    }

    fn fullscreen_video(&self) -> bool {
        use windows_sys::Win32::UI::Shell::{SHQueryUserNotificationState, QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN};
        let mut st = 0;
        // BUSY = 有程序全屏（视频、浏览器全屏播放）；D3D 全屏 = 游戏 / 播放器独占全屏；演示模式
        unsafe { SHQueryUserNotificationState(&mut st) == 0 && matches!(st, QUNS_BUSY | QUNS_RUNNING_D3D_FULL_SCREEN | QUNS_PRESENTATION_MODE) }
    }

    fn configure(&mut self, cfg: &blankrun_core::Config) {
        ALLOW_INJECTED.store(cfg.wake.allow_remote, Relaxed);
        // 显示器可能换过（设置页会重新探测）
        self.mon_cache.clear();
    }

    fn probe_monitor(&mut self, id: &str, _x: f64, _y: f64) -> super::MonHw {
        if let Some(h) = self.mon_cache.get(id) { return h.clone(); }
        let names = super::win_monitors::friendly_names();
        let h = super::win_monitors::probe(id, &names);
        crate::nblog!("monitor {id}: label={:?} adapter={:?} virtual={} power={:?}", h.label, h.detail, h.virtual_, h.power);
        self.mon_cache.insert(id.to_string(), h.clone());
        h
    }

    fn monitor_power(&mut self, id: &str, _x: f64, _y: f64, off: bool) -> Result {
        super::win_monitors::set_ddc_power(id, if off { 4 } else { 1 })
    }

    fn monitor_lit(&mut self, id: &str, _x: f64, _y: f64) -> Option<bool> {
        super::win_monitors::ddc_power(id).map(|v| v == 1)
    }

    fn remote_desktop(&self) -> bool {
        unsafe { GetSystemMetrics(SM_REMOTESESSION) != 0 }
    }
}

const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
fn wide(s: &str) -> Vec<u16> { s.encode_utf16().chain([0]).collect() }

// ---------- 「立即息屏」全局快捷键：RegisterHotKey，跑在自己的消息循环线程里 ----------
static HK_THREAD: std::sync::Mutex<Option<(u32, JoinHandle<()>)>> = std::sync::Mutex::new(None);

fn hk_vk(key: &str) -> Option<u32> {
    if let Some(i) = LETTERS.iter().position(|k| *k == key) { return Some(0x41 + i as u32); }
    if let Some(i) = DIGITS.iter().position(|k| *k == key) { return Some(0x30 + i as u32); }
    if let Some(i) = FKEYS.iter().position(|k| *k == key) { return Some(0x70 + i as u32); }
    Some(match key { "Esc" => 0x1B, "Space" => 0x20, "Enter" => 0x0D, "Tab" => 0x09, "Backspace" => 0x08, _ => return None })
}

pub fn set_blank_hotkey(h: Option<&hotkey::Hotkey>) -> Result {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN};
    let mut g = HK_THREAD.lock().map_err(|e| e.to_string())?;
    if let Some((tid, j)) = g.take() { unsafe { PostThreadMessageW(tid, WM_QUIT, 0, 0); } let _ = j.join(); }
    let Some(h) = h else { return Ok(()) };
    let vk = hk_vk(&h.key).ok_or_else(|| format!("unsupported key {}", h.key))?;
    let mut mods = MOD_NOREPEAT;
    if h.mods & hotkey::CTRL != 0 { mods |= MOD_CONTROL; }
    if h.mods & hotkey::ALT != 0 { mods |= MOD_ALT; }
    if h.mods & hotkey::SHIFT != 0 { mods |= MOD_SHIFT; }
    if h.mods & hotkey::META != 0 { mods |= MOD_WIN; }
    let (tx, rx) = std::sync::mpsc::channel();
    let j = std::thread::Builder::new().name("nb-hotkey".into()).spawn(move || unsafe {
        if RegisterHotKey(std::ptr::null_mut(), 1, mods, vk) == 0 {
            let _ = tx.send(Err("already used by another app".to_string()));
            return;
        }
        let _ = tx.send(Ok(GetCurrentThreadId()));
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            if msg.message == WM_HOTKEY { super::emit_hotkey(); }
        }
        UnregisterHotKey(std::ptr::null_mut(), 1);
    }).map_err(|e| e.to_string())?;
    match rx.recv().map_err(|e| e.to_string())? {
        Ok(tid) => { *g = Some((tid, j)); Ok(()) }
        Err(e) => { let _ = j.join(); Err(e) }
    }
}

/// panic 钩子里调用：不加锁，只做无状态的恢复
pub fn emergency_restore() { monitor_power(-1); super::win_monitors::wake_all(); }

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

/// 钩子装上以后按下过的键（vk 位图）。装钩子之前就按着的键（比如刚按的 Ctrl+Alt+B），
/// 它们的抬起要放行，否则别的程序会以为 Ctrl/Alt 一直按着。
static SEEN_DOWN: [std::sync::atomic::AtomicU64; 4] = [const { std::sync::atomic::AtomicU64::new(0) }; 4];
static BTN_SEEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub(crate) fn hook_reset() { for w in &SEEN_DOWN { w.store(0, Relaxed); } MODS.store(0, Relaxed); BTN_SEEN.store(false, Relaxed); }

unsafe extern "system" fn kb_proc(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code < 0 { return CallNextHookEx(std::ptr::null_mut(), code, wp, lp); }
    let k = &*(lp as *const KBDLLHOOKSTRUCT);
    // 远程控制软件、屏幕键盘发来的是「注入」事件：开了「放行远程控制」就原样放过去，不算误触
    if blankrun_core::wake::route(k.flags & LLKHF_INJECTED != 0, ALLOW_INJECTED.load(Relaxed)) == blankrun_core::wake::Route::PassThrough {
        return CallNextHookEx(std::ptr::null_mut(), code, wp, lp);
    }
    let down = wp == WM_KEYDOWN as WPARAM || wp == WM_SYSKEYDOWN as WPARAM;
    let (w, bit) = (&SEEN_DOWN[(k.vkCode as usize >> 6) & 3], 1u64 << (k.vkCode & 63));
    if down { w.fetch_or(bit, Relaxed); }
    else if w.fetch_and(!bit, Relaxed) & bit == 0 { return CallNextHookEx(std::ptr::null_mut(), code, wp, lp); }
    // 远程控制软件、屏幕键盘发来的是「注入」事件：一样拦下，也一样能用来唤醒
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
    if blankrun_core::wake::route(m.flags & LLMHF_INJECTED != 0, ALLOW_INJECTED.load(Relaxed)) == blankrun_core::wake::Route::PassThrough {
        return CallNextHookEx(std::ptr::null_mut(), code, wp, lp);
    }
    // 注入的（远程控制等）也拦下；只有装钩子之前按下的鼠标键，抬起时放行
    if matches!(wp as u32, WM_LBUTTONUP | WM_RBUTTONUP | WM_MBUTTONUP | WM_XBUTTONUP) && !BTN_SEEN.swap(false, Relaxed) {
        return CallNextHookEx(std::ptr::null_mut(), code, wp, lp);
    }
    if matches!(wp as u32, WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN) { BTN_SEEN.store(true, Relaxed); }
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

/// 用户界面语言（例如 "zh-CN"、"en-US"）；取不到时返回空串
pub fn system_locale() -> String {
    use windows_sys::Win32::Globalization::{GetUserPreferredUILanguages, MUI_LANGUAGE_NAME};
    let (mut num, mut len) = (0u32, 0u32);
    unsafe {
        if GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut num, std::ptr::null_mut(), &mut len) == 0 || len == 0 { return String::new(); }
        let mut buf = vec![0u16; len as usize];
        if GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut num, buf.as_mut_ptr(), &mut len) == 0 { return String::new(); }
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..end])
    }
}

/// 任务栏是不是浅色主题（决定托盘图标用深色还是浅色）；读不到按深色任务栏处理
pub fn taskbar_light() -> bool {
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
    let w = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<u16>>();
    let (key, val) = (w("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"), w("SystemUsesLightTheme"));
    let (mut v, mut n) = (0u32, 4u32);
    let r = unsafe { RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), val.as_ptr(), RRF_RT_REG_DWORD, std::ptr::null_mut(), &mut v as *mut _ as *mut _, &mut n) };
    r == 0 && v != 0
}

/// 单实例：命名互斥量（随进程释放）。已有实例在运行返回 false。
pub fn instance_lock(_path: &std::path::Path) -> bool {
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
    use windows_sys::Win32::System::Threading::CreateMutexW;
    let name: Vec<u16> = "Local\\com.cosfoc.blankrun".encode_utf16().chain([0]).collect();
    let h = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
    !h.is_null() && unsafe { GetLastError() } != ERROR_ALREADY_EXISTS // 句柄故意不关
}

/// Windows 的低级键鼠钩子不需要权限，用不到这个兜底
pub fn set_wake_hotkeys(_hs: &[blankrun_core::hotkey::Hotkey]) -> std::result::Result<usize, String> { Err("unsupported".into()) }
