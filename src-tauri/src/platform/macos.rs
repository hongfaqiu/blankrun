//! macOS 实现。见 RESEARCH.md §2。2026-10 在 MacBook Air M4 / macOS 26.5 上实测过关屏、防睡眠、背光探测。
//! 为了体积不引入 objc2 / core-foundation 等 crate，只手写用到的几个 C/ObjC 符号。
//! - 防睡眠：IOPMAssertion PreventUserIdleSystemSleep（= caffeinate -i）；ac_only 时用 PreventSystemSleep（只在接电源时生效）。
//! - 关屏：pmset displaysleepnow（IODisplayWrangler 的 IORequestIdle 在新系统上无效）。
//! - 恢复：IOPMAssertionDeclareUserActivity 点亮屏幕。
//! - 键盘背光：运行时 dlopen 私有的 CoreBrightness.framework，KeyboardBrightnessClient，每个方法先 respondsToSelector，缺任何一个就降级为"只关屏幕"。
//! - 输入：CGEventTap（会话级、主动过滤），需要「辅助功能」权限；回调吞掉所有事件并翻译后送进引擎。
//! - 崩溃：断言随进程释放；背光原值写进磁盘标记文件，下次启动还原；panic 钩子里用全局原子量立即还原。
#![allow(non_upper_case_globals, clippy::missing_safety_doc)]
use std::ffi::{c_char, c_void, CString};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering::Relaxed};
use std::thread::JoinHandle;

use blankrun_core::{hotkey, wake::Input, Config};

use super::{emit_input, now_ms, Cap, Platform, Result};

type Id = *mut c_void;
type CFStringRef = *const c_void;

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFStringCreateWithCString(alloc: *const c_void, s: *const c_char, enc: u32) -> CFStringRef;
    fn CFRelease(cf: *const c_void);
    static kCFRunLoopDefaultMode: CFStringRef;
    fn CFRunLoopGetCurrent() -> *mut c_void;
    fn CFRunLoopAddSource(rl: *mut c_void, source: *mut c_void, mode: CFStringRef);
    fn CFRunLoopRunInMode(mode: CFStringRef, seconds: f64, return_after_source: u8) -> i32;
    fn CFMachPortCreateRunLoopSource(alloc: *const c_void, port: *mut c_void, order: isize) -> *mut c_void;
    fn CFMachPortInvalidate(port: *mut c_void);
    fn CFLocaleCopyPreferredLanguages() -> *const c_void;
    fn CFArrayGetCount(a: *const c_void) -> isize;
    fn CFArrayGetValueAtIndex(a: *const c_void, i: isize) -> *const c_void;
    fn CFStringGetCString(s: CFStringRef, buf: *mut c_char, len: isize, enc: u32) -> u8;
    fn CFDictionaryGetValue(d: *const c_void, key: *const c_void) -> *const c_void;
    fn CFDictionaryGetCount(d: *const c_void) -> isize;
    fn CFDictionaryGetKeysAndValues(d: *const c_void, keys: *mut *const c_void, values: *mut *const c_void);
    fn CFNumberGetValue(n: *const c_void, ty: isize, out: *mut c_void) -> u8;
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Default)]
struct CGRect { x: f64, y: f64, w: f64, h: f64 }

#[link(name = "Carbon", kind = "framework")]
extern "C" {
    fn GetApplicationEventTarget() -> *mut c_void;
    fn InstallEventHandler(target: *mut c_void, h: HkHandler, n: u32, list: *const EventTypeSpec, user: *mut c_void, out: *mut *mut c_void) -> i32;
    fn RegisterEventHotKey(code: u32, mods: u32, id: EventHotKeyId, target: *mut c_void, opts: u32, out: *mut *mut c_void) -> i32;
    fn UnregisterEventHotKey(r: *mut c_void) -> i32;
    fn GetEventKind(ev: *mut c_void) -> u32;
    fn GetEventParameter(ev: *mut c_void, name: u32, ty: u32, out_ty: *mut u32, size: usize, out_size: *mut usize, data: *mut c_void) -> i32;
}
type HkHandler = extern "C" fn(next: *mut c_void, ev: *mut c_void, user: *mut c_void) -> i32;
#[repr(C)]
struct EventTypeSpec { class: u32, kind: u32 }
#[repr(C)]
struct EventHotKeyId { sig: u32, id: u32 }

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOPMAssertionCreateWithName(ty: CFStringRef, level: u32, name: CFStringRef, id: *mut u32) -> i32;
    fn IOPMAssertionRelease(id: u32) -> i32;
    fn IOPMAssertionDeclareUserActivity(name: CFStringRef, ty: u32, id: *mut u32) -> i32;
    fn IOPMCopyAssertionsByProcess(out: *mut *const c_void) -> i32;
}

type TapCb = extern "C" fn(proxy: *mut c_void, ty: u32, event: *mut c_void, user: *mut c_void) -> *mut c_void;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGMainDisplayID() -> u32;
    fn CGDisplayIsAsleep(display: u32) -> u32;
    fn CGEventTapCreate(tap: u32, place: u32, options: u32, mask: u64, cb: TapCb, user: *mut c_void) -> *mut c_void;
    fn CGEventTapEnable(tap: *mut c_void, enable: bool);
    fn CGEventGetIntegerValueField(event: *mut c_void, field: u32) -> i64;
    fn CGEventGetFlags(event: *mut c_void) -> u64;
    fn CGPreflightListenEventAccess() -> bool;
    fn CGWindowListCopyWindowInfo(opt: u32, rel: u32) -> *const c_void;
    fn CGRectMakeWithDictionaryRepresentation(d: *const c_void, r: *mut CGRect) -> bool;
    fn CGDisplayBounds(display: u32) -> CGRect;
    static kCGWindowLayer: CFStringRef;
    static kCGWindowBounds: CFStringRef;
    static kCGWindowOwnerPID: CFStringRef;
    fn CGRequestListenEventAccess() -> bool;
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
}

#[link(name = "objc")]
extern "C" {
    fn objc_getClass(name: *const c_char) -> Id;
    fn sel_registerName(name: *const c_char) -> Id;
    fn objc_msgSend();
}

const UTF8: u32 = 0x0800_0100;
const LEVEL_ON: u32 = 255;

fn cfstr(s: &str) -> CFStringRef {
    let c = CString::new(s).unwrap_or_default();
    unsafe { CFStringCreateWithCString(std::ptr::null(), c.as_ptr(), UTF8) }
}
fn sel(name: &str) -> Id { let c = CString::new(name).unwrap_or_default(); unsafe { sel_registerName(c.as_ptr()) } }

// objc_msgSend 必须按真实签名转型调用（arm64 上变参约定不同）
unsafe fn msg0(obj: Id, s: &str) -> Id {
    let f: extern "C" fn(Id, Id) -> Id = std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
    f(obj, sel(s))
}
unsafe fn responds(obj: Id, s: &str) -> bool {
    let f: extern "C" fn(Id, Id, Id) -> i8 = std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
    f(obj, sel("respondsToSelector:"), sel(s)) != 0
}

// ---------- 键盘背光（私有 CoreBrightness） ----------
static KB_CLIENT: AtomicUsize = AtomicUsize::new(0);
static KB_ID: AtomicU64 = AtomicU64::new(0);
/// 息屏前的亮度（f32 位）；u32::MAX = 没有待还原的值
static KB_SAVED: AtomicU32 = AtomicU32::new(u32::MAX);

const KB_METHODS: [&str; 3] = ["copyKeyboardBacklightIDs", "brightnessForKeyboard:", "setBrightness:forKeyboard:"];

/// 成功返回 (client, keyboard id)。只探测一次。
fn kb_probe() -> Option<(Id, u64)> {
    static PROBED: AtomicBool = AtomicBool::new(false);
    if !PROBED.swap(true, Relaxed) {
        unsafe {
            let path = c"/System/Library/PrivateFrameworks/CoreBrightness.framework/CoreBrightness";
            if !libc::dlopen(path.as_ptr(), libc::RTLD_LAZY).is_null() {
                let cls = objc_getClass(c"KeyboardBrightnessClient".as_ptr());
                if !cls.is_null() {
                    let obj = msg0(msg0(cls, "alloc"), "init");
                    if !obj.is_null() && KB_METHODS.iter().all(|m| responds(obj, m)) {
                        // ID 是 UInt64，必须先取；传 0 会无声失败（RESEARCH.md §2）
                        let ids = msg0(obj, "copyKeyboardBacklightIDs");
                        if !ids.is_null() && msg0(ids, "count") as usize > 0 {
                            let at: extern "C" fn(Id, Id, usize) -> Id = std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
                            let first = at(ids, sel("objectAtIndex:"), 0);
                            let val: extern "C" fn(Id, Id) -> u64 = std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
                            KB_ID.store(val(first, sel("unsignedLongLongValue")), Relaxed);
                            KB_CLIENT.store(obj as usize, Relaxed);
                        }
                    }
                }
            }
        }
    }
    let c = KB_CLIENT.load(Relaxed);
    (c != 0).then(|| (c as Id, KB_ID.load(Relaxed)))
}

fn kb_get() -> Option<f32> {
    let (c, id) = kb_probe()?;
    let f: extern "C" fn(Id, Id, u64) -> f32 = unsafe { std::mem::transmute(objc_msgSend as unsafe extern "C" fn()) };
    Some(f(c, sel("brightnessForKeyboard:"), id))
}
fn kb_set(v: f32) -> bool {
    let Some((c, id)) = kb_probe() else { return false };
    let f: extern "C" fn(Id, Id, f32, u64) -> i8 = unsafe { std::mem::transmute(objc_msgSend as unsafe extern "C" fn()) };
    f(c, sel("setBrightness:forKeyboard:"), v.clamp(0.0, 1.0), id) != 0
}

// ---------- 输入钩子（CGEventTap） ----------
static TAP: AtomicUsize = AtomicUsize::new(0);
static TAP_STOP: AtomicBool = AtomicBool::new(false);

fn keycode_name(code: i64) -> &'static str {
    match code {
        0 => "A", 1 => "S", 2 => "D", 3 => "F", 4 => "H", 5 => "G", 6 => "Z", 7 => "X", 8 => "C", 9 => "V",
        11 => "B", 12 => "Q", 13 => "W", 14 => "E", 15 => "R", 16 => "Y", 17 => "T", 18 => "1", 19 => "2",
        20 => "3", 21 => "4", 22 => "6", 23 => "5", 25 => "9", 26 => "7", 28 => "8", 29 => "0", 31 => "O",
        32 => "U", 34 => "I", 35 => "P", 37 => "L", 38 => "J", 40 => "K", 45 => "N", 46 => "M",
        36 => "Enter", 48 => "Tab", 49 => "Space", 51 => "Backspace", 53 => "Esc",
        122 => "F1", 120 => "F2", 99 => "F3", 118 => "F4", 96 => "F5", 97 => "F6", 98 => "F7", 100 => "F8",
        101 => "F9", 109 => "F10", 103 => "F11", 111 => "F12",
        _ => "?",
    }
}

fn flags_mods(f: u64) -> u8 {
    let mut m = 0;
    if f & 0x40000 != 0 { m |= hotkey::CTRL; }
    if f & 0x80000 != 0 { m |= hotkey::ALT; }
    if f & 0x20000 != 0 { m |= hotkey::SHIFT; }
    if f & 0x100000 != 0 { m |= hotkey::META; }
    m
}

extern "C" fn tap_cb(_proxy: *mut c_void, ty: u32, ev: *mut c_void, _user: *mut c_void) -> *mut c_void {
    unsafe {
        // kCGEventTapDisabledByTimeout / ByUserInput：系统把钩子停了，立即重新打开
        if ty == 0xFFFF_FFFE || ty == 0xFFFF_FFFF {
            let t = TAP.load(Relaxed);
            if t != 0 { CGEventTapEnable(t as *mut c_void, true); }
            return ev;
        }
        let t = now_ms();
        match ty {
            10 | 11 => emit_input(Input::Key {
                key: keycode_name(CGEventGetIntegerValueField(ev, 9)),
                mods: flags_mods(CGEventGetFlags(ev)), down: ty == 10, t_ms: t,
            }),
            12 => { let mods = flags_mods(CGEventGetFlags(ev)); emit_input(Input::Key { key: "Mod", mods, down: mods != 0, t_ms: t }) }
            5 | 6 | 7 | 27 => emit_input(Input::MouseMove {
                dx: CGEventGetIntegerValueField(ev, 4) as f64, dy: CGEventGetIntegerValueField(ev, 5) as f64, t_ms: t,
            }),
            1 | 3 | 25 => emit_input(Input::Click),
            14 | 22 => emit_input(Input::MouseMove { dx: 0.0, dy: 0.0, t_ms: t }),
            _ => {}
        }
    }
    std::ptr::null_mut() // 吞掉
}

fn tap_thread(ready: std::sync::mpsc::Sender<std::result::Result<(), String>>) {
    // 键盘按下/抬起/修饰键、鼠标按键/移动/拖动、滚轮、媒体键(NX_SYSDEFINED=14)
    let mask: u64 = [1u32, 2, 3, 4, 5, 6, 7, 10, 11, 12, 14, 22, 25, 26, 27].iter().fold(0, |m, t| m | (1u64 << t));
    unsafe {
        let tap = CGEventTapCreate(1 /* session */, 0 /* head */, 0 /* default=可吞 */, mask, tap_cb, std::ptr::null_mut());
        if tap.is_null() { let _ = ready.send(Err("CGEventTapCreate failed: allow BlankRun in Privacy & Security › Accessibility".into())); return; }
        TAP.store(tap as usize, Relaxed);
        let src = CFMachPortCreateRunLoopSource(std::ptr::null(), tap, 0);
        CFRunLoopAddSource(CFRunLoopGetCurrent(), src, kCFRunLoopDefaultMode);
        CGEventTapEnable(tap, true);
        let _ = ready.send(Ok(()));
        while !TAP_STOP.load(Relaxed) { CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.25, 0); }
        CGEventTapEnable(tap, false);
        TAP.store(0, Relaxed);
        CFMachPortInvalidate(tap);
        CFRelease(src);
        CFRelease(tap);
    }
}

// ---------- 平台实现 ----------
#[derive(Default)]
pub struct Native {
    assertion: Option<u32>,
    ac_only: bool,
    tap: Option<JoinHandle<()>>,
    tap_err: Option<String>,
    awake_err: Option<String>,
}

fn declare_user_activity() {
    unsafe {
        let name = cfstr("BlankRun wake");
        let mut id = 0u32;
        if IOPMAssertionDeclareUserActivity(name, 0 /* kIOPMUserActiveLocal */, &mut id) == 0 { IOPMAssertionRelease(id); }
        CFRelease(name);
    }
}

/// 关屏 = `pmset displaysleepnow`。
/// 实测 macOS 26 / M4：给 IODisplayWrangler 写 IORequestIdle 返回成功但屏幕不会睡，所以直接用系统命令。
fn display_sleep_now() -> Result {
    let st = std::process::Command::new("/usr/bin/pmset").arg("displaysleepnow").status().map_err(|e| e.to_string())?;
    if st.success() { Ok(()) } else { Err(format!("pmset displaysleepnow: {st}")) }
}

impl Platform for Native {
    fn configure(&mut self, cfg: &Config) { self.ac_only = cfg.power.ac_only; }

    fn keep_awake(&mut self, on: bool) -> Result {
        if let Some(id) = self.assertion.take() { unsafe { IOPMAssertionRelease(id); } }
        if !on { return Ok(()); }
        // 不用 PreventUserIdleDisplaySleep（-d），否则屏幕会被保持常亮
        let ty = cfstr(if self.ac_only { "PreventSystemSleep" } else { "PreventUserIdleSystemSleep" });
        let name = cfstr("BlankRun: screen off, keep running");
        let mut id = 0u32;
        let r = unsafe { IOPMAssertionCreateWithName(ty, LEVEL_ON, name, &mut id) };
        unsafe { CFRelease(ty); CFRelease(name); }
        if r != 0 { let e = format!("IOPMAssertionCreateWithName = {r:#x}"); self.awake_err = Some(e.clone()); return Err(e); }
        self.awake_err = None;
        self.assertion = Some(id);
        Ok(())
    }

    fn display_off(&mut self, off: bool) -> Result {
        if off { display_sleep_now() } else { declare_user_activity(); Ok(()) }
    }

    fn display_asleep(&self) -> Option<bool> { Some(unsafe { CGDisplayIsAsleep(CGMainDisplayID()) } != 0) }

    fn keyboard_backlight(&mut self, off: bool) -> Result {
        if off {
            let Some(cur) = kb_get() else { return Ok(()) }; // 不支持：降级，只关屏幕
            if KB_SAVED.load(Relaxed) == u32::MAX { KB_SAVED.store(cur.to_bits(), Relaxed); }
            if !kb_set(0.0) { return Err("setBrightness:forKeyboard: failed".into()); }
            crate::nblog!("keyboard backlight {cur} -> 0 (now {:?})", kb_get());
        } else {
            let bits = KB_SAVED.swap(u32::MAX, Relaxed);
            if bits != u32::MAX && !kb_set(f32::from_bits(bits)) { return Err("restoring keyboard backlight failed".into()); }
            if bits != u32::MAX { crate::nblog!("keyboard backlight restored to {} (now {:?})", f32::from_bits(bits), kb_get()); }
        }
        Ok(())
    }

    fn input_hook(&mut self, on: bool) -> Result {
        if on {
            if self.tap.is_some() { return Ok(()); }
            TAP_STOP.store(false, Relaxed);
            let (tx, rx) = std::sync::mpsc::channel();
            let h = std::thread::Builder::new().name("nb-tap".into()).spawn(move || tap_thread(tx)).map_err(|e| e.to_string())?;
            match rx.recv().map_err(|e| e.to_string()).and_then(|r| r) {
                Ok(()) => { self.tap = Some(h); self.tap_err = None; Ok(()) }
                Err(e) => { let _ = h.join(); self.tap_err = Some(e.clone()); Err(e) }
            }
        } else {
            if let Some(h) = self.tap.take() { TAP_STOP.store(true, Relaxed); let _ = h.join(); }
            Ok(())
        }
    }

    fn lock_screen(&mut self) -> Result {
        // 私有 login.framework 的 SACLockScreenImmediate；运行时解析，缺失就报错（不影响唤醒本身）
        unsafe {
            let h = libc::dlopen(c"/System/Library/PrivateFrameworks/login.framework/Versions/Current/login".as_ptr(), libc::RTLD_LAZY);
            if !h.is_null() {
                let f = libc::dlsym(h, c"SACLockScreenImmediate".as_ptr());
                if !f.is_null() {
                    let f: extern "C" fn() -> i32 = std::mem::transmute(f);
                    f();
                    return Ok(());
                }
            }
        }
        Err("SACLockScreenImmediate unavailable".into())
    }

    fn capabilities(&mut self) -> Vec<Cap> {
        let detail = "IOPMAssertion · PreventUserIdleSystemSleep";
        vec![
            match &self.awake_err {
                Some(e) => Cap::new("awake", "awake_bad", "bad", format!("{detail} · {e}")),
                None => Cap::new("awake", "awake_ok", "ok", detail),
            },
            if kb_probe().is_some() {
                Cap::new("kbd", "kbd_mac_ok", "ok", "CoreBrightness")
            } else {
                Cap::new("kbd", "kbd_mac_off", "off", "CoreBrightness")
            },
            // 还没装过钩子时按权限判断，免得没授权也显示「正常」
            match &self.tap_err {
                Some(e) => Cap::new("hook", "hook_bad", "bad", format!("CGEventTap · {e}")),
                None if !unsafe { AXIsProcessTrusted() } => Cap::new("hook", "hook_bad", "bad", "CGEventTap · Accessibility not granted"),
                None => Cap::new("hook", "hook_mac_ok", "ok", "CGEventTap"),
            },
            Cap::new("ddc", "ddc_todo", "off", "DDC/CI"),
            Cap::new("rgb", "rgb_todo", "off", "OpenRGB SDK"),
        ]
    }

    fn permissions(&self) -> Option<(bool, bool)> {
        Some(unsafe { (AXIsProcessTrusted(), CGPreflightListenEventAccess()) })
    }

    fn request_permission(&self, which: &str) {
        let pane = if which == "input" {
            unsafe { CGRequestListenEventAccess(); }
            "Privacy_ListenEvent"
        } else { "Privacy_Accessibility" };
        let _ = std::process::Command::new("/usr/bin/open")
            .arg(format!("x-apple.systempreferences:com.apple.preference.security?{pane}")).spawn();
    }

    fn persist(&self) -> Option<String> {
        let bits = KB_SAVED.load(Relaxed);
        Some(if bits == u32::MAX { "kbd=".into() } else { format!("kbd={}", f32::from_bits(bits)) })
    }

    fn set_launch_at_login(&mut self, on: bool) -> Result {
        let p = launch_agent().ok_or("no HOME")?;
        if !on { return match std::fs::remove_file(&p) { Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()), _ => Ok(()) }; }
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        // 在 .app 里时用 open -a 启动整个 App（LaunchServices），否则直接跑可执行文件
        let app = exe.ancestors().nth(3).filter(|a| a.extension().is_some_and(|x| x == "app"));
        let args: Vec<String> = match app {
            Some(a) => vec!["/usr/bin/open".into(), "-a".into(), a.display().to_string()],
            None => vec![exe.display().to_string()],
        };
        let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
        let items: String = args.iter().map(|a| format!("<string>{}</string>", esc(a))).collect();
        let plist = format!(concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
            "<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n",
            "<plist version=\"1.0\"><dict><key>Label</key><string>com.cosfoc.blankrun</string>",
            "<key>ProgramArguments</key><array>{}</array><key>RunAtLoad</key><true/></dict></plist>\n"), items);
        if let Some(d) = p.parent() { std::fs::create_dir_all(d).map_err(|e| e.to_string())?; }
        std::fs::write(&p, plist).map_err(|e| e.to_string())
    }

    fn launch_at_login(&self) -> Option<bool> { Some(launch_agent()?.exists()) }

    fn fullscreen_video(&self) -> bool { fullscreen_video() }

    fn restore_on_startup(&mut self, saved: Option<&str>) -> Result {
        let Some(s) = saved else { return Ok(()) };
        declare_user_activity();
        if let Some(v) = s.strip_prefix("kbd=").and_then(|v| v.trim().parse::<f32>().ok()) {
            if !kb_set(v) { return Err("restoring keyboard backlight failed".into()); }
        }
        Ok(())
    }
}

impl Drop for Native {
    fn drop(&mut self) { let _ = self.input_hook(false); let _ = self.keep_awake(false); }
}

/// panic 钩子里调用：不加锁
pub fn emergency_restore() {
    let bits = KB_SAVED.swap(u32::MAX, Relaxed);
    if bits != u32::MAX { kb_set(f32::from_bits(bits)); }
    TAP_STOP.store(true, Relaxed);
    declare_user_activity();
}

pub fn local_time() -> (u8, u16) {
    let mut t = std::mem::MaybeUninit::<libc::tm>::zeroed();
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        libc::localtime_r(&now, t.as_mut_ptr());
        let t = t.assume_init();
        (((t.tm_wday + 6) % 7) as u8, (t.tm_hour * 60 + t.tm_min) as u16)
    }
}

pub fn reveal(path: &std::path::Path) {
    let _ = std::process::Command::new("/usr/bin/open").arg("-R").arg(path).spawn();
}

/// 遮罩窗口盖过菜单栏和 Dock：NSScreenSaverWindowLevel + 所有桌面空间可见。必须在主线程调用。
pub fn raise_overlay(ns_window: *mut c_void) {
    if ns_window.is_null() { return; }
    unsafe {
        let set_level: extern "C" fn(Id, Id, isize) = std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        set_level(ns_window, sel("setLevel:"), 1000);
        let set_cb: extern "C" fn(Id, Id, usize) = std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        // canJoinAllSpaces | stationary | fullScreenAuxiliary
        set_cb(ns_window, sel("setCollectionBehavior:"), 1 | 16 | 256);
    }
}

/// 系统首选语言（BCP 47，例如 "zh-Hans-CN"、"en-US"）；取不到时返回空串
pub fn system_locale() -> String {
    unsafe {
        let a = CFLocaleCopyPreferredLanguages();
        if a.is_null() { return String::new(); }
        let mut out = String::new();
        if CFArrayGetCount(a) > 0 {
            let mut buf = [0 as c_char; 64];
            if CFStringGetCString(CFArrayGetValueAtIndex(a, 0), buf.as_mut_ptr(), buf.len() as isize, UTF8) != 0 {
                out = std::ffi::CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned();
            }
        }
        CFRelease(a);
        out
    }
}

/// 单实例：对 lock 文件加独占锁，进程退出时系统自动释放。已有实例在运行返回 false。
pub fn instance_lock(path: &std::path::Path) -> bool {
    let Ok(c) = CString::new(path.as_os_str().as_encoded_bytes()) else { return true };
    unsafe {
        let fd = libc::open(c.as_ptr(), libc::O_RDWR | libc::O_CREAT, 0o644);
        if fd < 0 { return true; }
        libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) == 0 // fd 故意不关，锁随进程存在
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct NSRect { x: f64, y: f64, w: f64, h: f64 }

/// 主屏刘海占用的横向范围（点）：(左, 右)。没有刘海或拿不到返回 None。
/// auxiliaryTopLeftArea / auxiliaryTopRightArea 是刘海两侧可用的菜单栏区域（macOS 12+）。
/// 只在 arm64 上调用：x86_64 上返回结构体要用 objc_msgSend_stret，而 Intel Mac 也没有刘海。
pub fn notch_span() -> Option<(f64, f64)> {
    if !cfg!(target_arch = "aarch64") { return None; }
    unsafe {
        let cls = objc_getClass(c"NSScreen".as_ptr());
        if cls.is_null() { return None; }
        let screen = msg0(cls, "mainScreen");
        if screen.is_null() || !responds(screen, "auxiliaryTopLeftArea") { return None; }
        let f: extern "C" fn(Id, Id) -> NSRect = std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        let (l, r) = (f(screen, sel("auxiliaryTopLeftArea")), f(screen, sel("auxiliaryTopRightArea")));
        (l.w > 0.0 && r.w > 0.0).then_some((l.x + l.w, r.x))
    }
}

/// 新装时菜单栏图标默认排在所有图标的最左边，菜单栏满时会被挤到刘海后面（MacBook 上很常见）。
/// 第一次运行时给它一个靠右的位置（离右边缘 260 点，在系统图标旁边）；用户 ⌘ 拖动后系统会自己改写这个值。
pub fn prefer_tray_position() {
    unsafe {
        let cls = objc_getClass(c"NSUserDefaults".as_ptr());
        if cls.is_null() { return; }
        let d = msg0(cls, "standardUserDefaults");
        if d.is_null() { return; }
        let key = cfstr("NSStatusItem Preferred Position Item-0");
        let get: extern "C" fn(Id, Id, CFStringRef) -> Id = std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        if get(d, sel("objectForKey:"), key).is_null() {
            let set: extern "C" fn(Id, Id, f64, CFStringRef) = std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
            set(d, sel("setDouble:forKey:"), 260.0, key);
            crate::nblog!("tray: first run, preferred position set near the right edge");
        }
        CFRelease(key);
    }
}

fn launch_agent() -> Option<std::path::PathBuf> {
    Some(std::path::PathBuf::from(std::env::var_os("HOME")?).join("Library/LaunchAgents/com.cosfoc.blankrun.plist"))
}

fn cf_string(s: *const c_void) -> String {
    if s.is_null() { return String::new(); }
    let mut buf = [0 as c_char; 256];
    unsafe {
        if CFStringGetCString(s, buf.as_mut_ptr(), buf.len() as isize, UTF8) == 0 { return String::new(); }
        std::ffi::CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned()
    }
}
fn cf_i64(n: *const c_void) -> Option<i64> {
    let mut v = 0i64;
    (!n.is_null() && unsafe { CFNumberGetValue(n, 4 /* kCFNumberSInt64Type */, &mut v as *mut _ as *mut c_void) } != 0).then_some(v)
}

/// 全屏看视频：主屏上有别的 App 的窗口铺满整屏，并且有程序正为播放视频阻止屏幕睡眠
/// （该全屏 App 自己持有 PreventUserIdleDisplaySleep，或断言名里带 video / media / playback，例如 Chrome 的 Video Wake Lock）。
/// 只看全屏不看断言会把全屏终端也算进去；只看断言会被远程桌面这类常驻断言误判。
pub fn fullscreen_video() -> bool {
    let me = std::process::id() as i64;
    let mut owners: Vec<i64> = Vec::new();
    unsafe {
        let screen = CGDisplayBounds(CGMainDisplayID());
        let list = CGWindowListCopyWindowInfo(1 | 16 /* on screen only, exclude desktop */, 0);
        if list.is_null() { return false; }
        for i in 0..CFArrayGetCount(list) {
            let w = CFArrayGetValueAtIndex(list, i);
            if cf_i64(CFDictionaryGetValue(w, kCGWindowLayer)) != Some(0) { continue; }
            let mut r = CGRect::default();
            if !CGRectMakeWithDictionaryRepresentation(CFDictionaryGetValue(w, kCGWindowBounds), &mut r) || r != screen { continue; }
            if let Some(pid) = cf_i64(CFDictionaryGetValue(w, kCGWindowOwnerPID)) { if pid != me { owners.push(pid); } }
        }
        CFRelease(list);
        if owners.is_empty() { return false; }

        let mut dict: *const c_void = std::ptr::null();
        if IOPMCopyAssertionsByProcess(&mut dict) != 0 || dict.is_null() { return false; }
        let n = CFDictionaryGetCount(dict) as usize;
        let (mut keys, mut vals) = (vec![std::ptr::null(); n], vec![std::ptr::null(); n]);
        CFDictionaryGetKeysAndValues(dict, keys.as_mut_ptr(), vals.as_mut_ptr());
        let (ty_key, name_key) = (cfstr("AssertType"), cfstr("AssertName"));
        let mut hit = false;
        'outer: for (k, v) in keys.iter().zip(&vals) {
            let Some(pid) = cf_i64(*k) else { continue };
            if pid == me { continue; }
            for j in 0..CFArrayGetCount(*v) {
                let a = CFArrayGetValueAtIndex(*v, j);
                let ty = cf_string(CFDictionaryGetValue(a, ty_key));
                if ty != "PreventUserIdleDisplaySleep" && ty != "NoDisplaySleepAssertion" { continue; }
                let name = cf_string(CFDictionaryGetValue(a, name_key)).to_ascii_lowercase();
                if owners.contains(&pid) || ["video", "media", "playback"].iter().any(|w| name.contains(w)) { hit = true; break 'outer; }
            }
        }
        CFRelease(ty_key); CFRelease(name_key); CFRelease(dict);
        hit
    }
}

// ---------- 「立即息屏」全局快捷键：Carbon RegisterEventHotKey（不需要任何权限） ----------
static HK_REF: AtomicUsize = AtomicUsize::new(0);
static HK_HANDLER: AtomicBool = AtomicBool::new(false);

/// 唤醒 / 紧急快捷键（没有辅助功能权限、装不上 CGEventTap 时的兜底），id 2.. ：(注册句柄, 键码, 修饰键位)
static WK: std::sync::Mutex<Vec<(usize, i64, u8)>> = std::sync::Mutex::new(Vec::new());

extern "C" fn hk_handler(_next: *mut c_void, ev: *mut c_void, _user: *mut c_void) -> i32 {
    unsafe {
        let mut id = EventHotKeyId { sig: 0, id: 0 };
        let st = GetEventParameter(ev, u32::from_be_bytes(*b"----"), u32::from_be_bytes(*b"hkid"), std::ptr::null_mut(),
            std::mem::size_of::<EventHotKeyId>(), std::ptr::null_mut(), &mut id as *mut _ as *mut c_void);
        let down = GetEventKind(ev) == 5;
        if st != 0 || id.id <= 1 { if down { super::emit_hotkey(); } return 0; }
        let k = WK.lock().ok().and_then(|g| g.get(id.id as usize - 2).copied());
        if let Some((_, code, mods)) = k {
            super::emit_input(Input::Key { key: keycode_name(code), mods, down, t_ms: super::now_ms() });
        }
    }
    0
}

fn carbon_mods(m: u8) -> u32 {
    let mut r = 0u32;
    if m & hotkey::META != 0 { r |= 0x100; }
    if m & hotkey::SHIFT != 0 { r |= 0x200; }
    if m & hotkey::ALT != 0 { r |= 0x800; }
    if m & hotkey::CTRL != 0 { r |= 0x1000; }
    r
}

unsafe fn install_hk_handler() {
    if !HK_HANDLER.swap(true, Relaxed) {
        let k = u32::from_be_bytes(*b"keyb");
        let specs = [EventTypeSpec { class: k, kind: 5 /* pressed */ }, EventTypeSpec { class: k, kind: 6 /* released */ }];
        InstallEventHandler(GetApplicationEventTarget(), hk_handler, 2, specs.as_ptr(), std::ptr::null_mut(), std::ptr::null_mut());
    }
}

/// 息屏期间临时注册唤醒键和紧急键（不需要任何权限），按下 / 松开会作为 Input::Key 送进唤醒策略。
/// 空列表 = 全部注销。返回成功注册的个数。必须在主线程调用。
pub fn set_wake_hotkeys(hs: &[hotkey::Hotkey]) -> std::result::Result<usize, String> {
    unsafe {
        let mut g = WK.lock().map_err(|e| e.to_string())?;
        for (r, ..) in g.drain(..) { if r != 0 { UnregisterEventHotKey(r as *mut c_void); } }
        if hs.is_empty() { return Ok(0); }
        install_hk_handler();
        let mut n = 0;
        for (i, h) in hs.iter().enumerate() {
            let code = (0..128).find(|c| keycode_name(*c) == h.key);
            let mut r: *mut c_void = std::ptr::null_mut();
            if let Some(code) = code {
                let st = RegisterEventHotKey(code as u32, carbon_mods(h.mods), EventHotKeyId { sig: u32::from_be_bytes(*b"BlRn"), id: 2 + i as u32 }, GetApplicationEventTarget(), 0, &mut r);
                if st == 0 { n += 1; } else { r = std::ptr::null_mut(); crate::nblog!("wake key {}: RegisterEventHotKey = {st}", h.key); }
            }
            g.push((r as usize, code.unwrap_or(-1), h.mods));
        }
        Ok(n)
    }
}

/// 必须在主线程调用
pub fn set_blank_hotkey(h: Option<&hotkey::Hotkey>) -> Result {
    unsafe {
        let old = HK_REF.swap(0, Relaxed);
        if old != 0 { UnregisterEventHotKey(old as *mut c_void); }
        let Some(h) = h else { return Ok(()) };
        let code = (0..128).find(|c| keycode_name(*c) == h.key).ok_or_else(|| format!("unsupported key {}", h.key))? as u32;
        install_hk_handler();
        let mods = carbon_mods(h.mods);
        let mut r: *mut c_void = std::ptr::null_mut();
        let st = RegisterEventHotKey(code, mods, EventHotKeyId { sig: u32::from_be_bytes(*b"BlRn"), id: 1 }, GetApplicationEventTarget(), 0, &mut r);
        match st {
            0 => { HK_REF.store(r as usize, Relaxed); Ok(()) }
            -9878 => Err("already used by another app".into()),
            e => Err(format!("RegisterEventHotKey = {e}")),
        }
    }
}
