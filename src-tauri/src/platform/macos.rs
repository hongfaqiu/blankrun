//! macOS 实现。见 RESEARCH.md §2。2026-10 在 MacBook Air M4 / macOS 26.5 上实测过关屏、防睡眠、背光探测。
//! 为了体积不引入 objc2 / core-foundation 等 crate，只手写用到的几个 C/ObjC 符号。
//! - 防睡眠：IOPMAssertion PreventUserIdleSystemSleep（= caffeinate -i）；ac_only 时用 PreventSystemSleep（只在接电源时生效）。
//! - 关屏：pmset displaysleepnow（IODisplayWrangler 的 IORequestIdle 在新系统上无效）。
//! - 恢复：IOPMAssertionDeclareUserActivity 点亮屏幕。
//! - 内建屏单独关闭：Apple 芯片 + macOS 13 起用 SkyLight 私有 `SLSConfigureDisplayEnabled(false)`「断开」，
//!   和合盖、BetterDisplay「断开显示器」、Lunar BlackOut 同一机制：屏幕从桌面布局里消失，面板进待机。
//!   运行时 dlsym，拿不到、被拒绝或只剩这一块屏时退回 DisplayServices 背光调 0 + 遮罩。
//! - 键盘背光：运行时 dlopen 私有的 CoreBrightness.framework，KeyboardBrightnessClient，每个方法先 respondsToSelector，缺任何一个就降级为"只关屏幕"。
//! - 输入：CGEventTap（会话级、主动过滤），需要「辅助功能」权限；回调吞掉所有事件并翻译后送进引擎。
//!   「键鼠照常用」（Mac Mini 模式）时只吞唤醒键 / 紧急键，别的事件原样返回（= 放行），光标被挡在关掉的屏外面。
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
    fn CFDictionaryCreate(alloc: *const c_void, keys: *const *const c_void, values: *const *const c_void, n: isize, kcb: *const c_void, vcb: *const c_void) -> *const c_void;
    static kCFTypeDictionaryKeyCallBacks: [usize; 6];
    static kCFTypeDictionaryValueCallBacks: [usize; 5];
    static kCFBooleanTrue: *const c_void;
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Default)]
struct CGRect { x: f64, y: f64, w: f64, h: f64 }
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Default)]
struct CGPoint { x: f64, y: f64 }

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
    fn CGEventSourceFlagsState(state: i32) -> u64;
    fn CGEventSourceKeyState(state: i32, key: u16) -> bool;
    fn CGEventTapCreate(tap: u32, place: u32, options: u32, mask: u64, cb: TapCb, user: *mut c_void) -> *mut c_void;
    fn CGEventTapEnable(tap: *mut c_void, enable: bool);
    fn CGEventGetIntegerValueField(event: *mut c_void, field: u32) -> i64;
    fn CGEventGetFlags(event: *mut c_void) -> u64;
    fn CGPreflightListenEventAccess() -> bool;
    fn CGWindowListCopyWindowInfo(opt: u32, rel: u32) -> *const c_void;
    fn CGRectMakeWithDictionaryRepresentation(d: *const c_void, r: *mut CGRect) -> bool;
    fn CGDisplayBounds(display: u32) -> CGRect;
    fn CGGetActiveDisplayList(max: u32, displays: *mut u32, count: *mut u32) -> i32;
    fn CGGetOnlineDisplayList(max: u32, displays: *mut u32, count: *mut u32) -> i32;
    fn CGDisplayIsBuiltin(display: u32) -> u32;
    fn CGDisplayPixelsWide(display: u32) -> usize;
    static kCGWindowLayer: CFStringRef;
    static kCGWindowBounds: CFStringRef;
    static kCGWindowOwnerPID: CFStringRef;
    fn CGRequestListenEventAccess() -> bool;
    fn CGEventGetLocation(event: *mut c_void) -> CGPoint;
    fn CGEventSetLocation(event: *mut c_void, p: CGPoint);
    fn CGWarpMouseCursorPosition(p: CGPoint) -> i32;
    fn CGAssociateMouseAndMouseCursorPosition(connected: u32) -> i32;
    fn CGBeginDisplayConfiguration(config: *mut *mut c_void) -> i32;
    fn CGCompleteDisplayConfiguration(config: *mut c_void, option: u32) -> i32;
    fn CGCancelDisplayConfiguration(config: *mut c_void) -> i32;
    fn CGDisplayIsActive(display: u32) -> u32;
    fn CGDisplayVendorNumber(display: u32) -> u32;
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: *const c_void) -> bool;
    static kAXTrustedCheckOptionPrompt: CFStringRef;
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

// ---------- 内建屏背光（DisplayServices 私有框架，运行时探测） ----------
type DsGet = unsafe extern "C" fn(u32, *mut f32) -> i32;
type DsSet = unsafe extern "C" fn(u32, f32) -> i32;
fn display_services() -> Option<(DsGet, DsSet)> {
    static F: std::sync::OnceLock<Option<(usize, usize)>> = std::sync::OnceLock::new();
    let (g, s) = (*F.get_or_init(|| unsafe {
        let h = libc::dlopen(c"/System/Library/PrivateFrameworks/DisplayServices.framework/DisplayServices".as_ptr(), libc::RTLD_LAZY);
        if h.is_null() { return None; }
        let (g, s) = (libc::dlsym(h, c"DisplayServicesGetBrightness".as_ptr()), libc::dlsym(h, c"DisplayServicesSetBrightness".as_ptr()));
        (!g.is_null() && !s.is_null()).then_some((g as usize, s as usize))
    }))?;
    Some(unsafe { (std::mem::transmute::<usize, DsGet>(g), std::mem::transmute::<usize, DsSet>(s)) })
}
/// 关掉前的内建屏亮度（display id, 值）；崩溃后启动时据此还原
static BL_SAVED: std::sync::Mutex<Option<(u32, f32)>> = std::sync::Mutex::new(None);

/// 左上角在 (x, y)（点）的那块屏
fn display_at(x: f64, y: f64) -> Option<u32> {
    let mut ids = [0u32; 16];
    let mut n = 0u32;
    if unsafe { CGGetActiveDisplayList(16, ids.as_mut_ptr(), &mut n) } != 0 { return None; }
    ids[..n as usize].iter().copied().min_by(|a, b| {
        let d = |id: u32| { let r = unsafe { CGDisplayBounds(id) }; (r.x - x).abs() + (r.y - y).abs() };
        d(*a).partial_cmp(&d(*b)).unwrap_or(std::cmp::Ordering::Equal)
    })
}

fn set_builtin_off(id: u32, off: bool) -> Result {
    let (get, set) = display_services().ok_or("DisplayServices unavailable")?;
    let mut g = BL_SAVED.lock().map_err(|e| e.to_string())?;
    if off {
        let mut v = 0f32;
        if unsafe { get(id, &mut v) } != 0 { return Err("DisplayServicesGetBrightness failed".into()); }
        if g.is_none() { *g = Some((id, v)); }
        if unsafe { set(id, 0.0) } != 0 { return Err("DisplayServicesSetBrightness failed".into()); }
        crate::nblog!("built-in display {id}: brightness {v} -> 0");
    } else if let Some((sid, v)) = g.take() {
        unsafe { set(sid, v); }
        crate::nblog!("built-in display {sid}: brightness restored to {v}");
    }
    Ok(())
}

// ---------- 内建屏「断开」（SkyLight SLSConfigureDisplayEnabled） ----------
type CfgEnabled = unsafe extern "C" fn(*mut c_void, u32, bool) -> i32;
/// 当前被我们断开的屏（CGDirectDisplayID，0 = 没有）。崩溃标记里也记一份，下次启动接回
static DISCONNECTED: AtomicU32 = AtomicU32::new(0);
/// 本次息屏断开失败过：之后只调背光（遮罩已经盖上，再断开会把遮罩窗口挪到外接屏上）
static DISCONNECT_FAILED: AtomicBool = AtomicBool::new(false);
/// 设置里的 display.builtin_off
static BUILTIN_OFF: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());
/// 设置里的 display.builtin_test（内建屏断开检测的结果）
static BUILTIN_TEST: std::sync::Mutex<Option<blankrun_core::config::BuiltinTest>> = std::sync::Mutex::new(None);
/// 正在检测、被检测断开的内建屏（0 = 没在检测）；panic 时也要接回
static TESTING: AtomicU32 = AtomicU32::new(0);

fn sysctl_str(name: &std::ffi::CStr) -> String {
    let mut buf = [0u8; 128];
    let mut len = buf.len();
    let r = unsafe { libc::sysctlbyname(name.as_ptr(), buf.as_mut_ptr() as *mut c_void, &mut len, std::ptr::null_mut(), 0) };
    if r != 0 || len == 0 { return String::new(); }
    String::from_utf8_lossy(&buf[..len.min(buf.len())]).trim_end_matches('\0').trim().to_string()
}

/// SLSConfigureDisplayEnabled（老名字 CGSConfigureDisplayEnabled），只探测一次
fn display_enable_fn() -> Option<CfgEnabled> {
    static F: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    let p = *F.get_or_init(|| unsafe {
        let sl = libc::dlopen(c"/System/Library/PrivateFrameworks/SkyLight.framework/SkyLight".as_ptr(), libc::RTLD_LAZY);
        for h in [sl, libc::RTLD_DEFAULT] {
            if h.is_null() { continue; }
            for name in [c"SLSConfigureDisplayEnabled", c"CGSConfigureDisplayEnabled"] {
                let f = libc::dlsym(h, name.as_ptr());
                if !f.is_null() { return f as usize; }
            }
        }
        0
    });
    (p != 0).then(|| unsafe { std::mem::transmute::<usize, CfgEnabled>(p) })
}

/// (hw.model, 芯片, 系统版本, 系统主版本)
fn machine() -> (String, String, String, u32) {
    let os = sysctl_str(c"kern.osproductversion");
    let major = os.split('.').next().and_then(|v| v.parse().ok()).unwrap_or(0);
    (sysctl_str(c"hw.model"), sysctl_str(c"machdep.cpu.brand_string"), os, major)
}

/// 这台机器上测过的结果（换了机型 / 芯片 / 系统主版本就不算数）
fn tested_verdict() -> Option<bool> {
    let (model, chip, _, major) = machine();
    BUILTIN_TEST.lock().ok()?.as_ref()?.verdict(&chip, &model, major)
}

/// 按设置和机器决定内建屏能不能用「断开」
fn disconnect_supported() -> bool {
    let setting = BUILTIN_OFF.lock().map(|g| g.clone()).unwrap_or_default();
    let (model, chip, _, os_major) = machine();
    let how = blankrun_core::monitors::builtin_off(if setting.is_empty() { "auto" } else { &setting },
        &chip, &model, os_major, cfg!(target_arch = "aarch64"), tested_verdict());
    how == blankrun_core::monitors::BuiltinOff::Disconnect && display_enable_fn().is_some()
}

// ---------- 内建屏断开检测（设置 › 息屏 › 内建屏 › 检测） ----------
fn display_list(online: bool) -> Vec<u32> {
    let mut ids = [0u32; 16];
    let mut n = 0u32;
    let e = unsafe { if online { CGGetOnlineDisplayList(16, ids.as_mut_ptr(), &mut n) } else { CGGetActiveDisplayList(16, ids.as_mut_ptr(), &mut n) } };
    if e != 0 { return vec![]; }
    ids[..(n as usize).min(16)].to_vec()
}
/// 内建屏在不在系统当前使用的显示器里（活动列表里有、而且 active）
fn builtin_present(id: u32) -> bool {
    display_list(false).contains(&id) && unsafe { CGDisplayIsActive(id) } != 0
}
fn wait_until(ms: u64, f: impl Fn() -> bool) -> bool {
    let t = std::time::Instant::now();
    loop {
        if f() { return true; }
        if t.elapsed() >= std::time::Duration::from_millis(ms) { return false; }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

/// 检测内建屏能不能真正断开再接回：断开 → 确认消失 → 等约 3 秒 → 接回 → 确认回来。
/// `marker`：崩溃标记文件，断开前写入 dis=<id>，下次启动据此接回；接回成功后删掉。
/// 调用前上层要确认没在息屏。这里会阻塞几秒到十几秒，别在主线程 / 持着引擎锁时调用。
pub fn builtin_test(marker: &std::path::Path) -> super::BuiltinTestOutcome {
    let (model, chip, os, major) = machine();
    let out = |result: &str, detail: String| {
        crate::nblog!("built-in display test: {result} ({detail}) on {model} / {chip} / macOS {os}");
        super::BuiltinTestOutcome { result: result.into(), detail, model: model.clone(), chip: chip.clone(), os: os.clone() }
    };
    // 前提：有内建屏、有别的物理屏亮着
    let Some(id) = display_list(true).into_iter().find(|&d| unsafe { CGDisplayIsBuiltin(d) } != 0) else {
        return out("no_builtin", "no built-in display online (lid closed?)".into());
    };
    if !builtin_present(id) { return out("no_builtin", format!("built-in display {id} is not active (lid closed?)")); }
    if other_real_displays(id) == 0 { return out("need_external", "no other physical display".into()); }
    if DISCONNECTED.load(Relaxed) != 0 { return out("busy", "built-in display is already disconnected by BlankRun".into()); }
    if !cfg!(target_arch = "aarch64") { return out("unsupported", "Intel Mac: SLSConfigureDisplayEnabled does not power the panel down".into()); }
    if major < 13 { return out("unsupported", format!("macOS {os}: needs macOS 13 or later")); }
    if display_enable_fn().is_none() { return out("unsupported", "SLSConfigureDisplayEnabled unavailable".into()); }

    // 断开前写崩溃标记：检测中途崩溃 / 被强退，下次启动会接回
    let _ = std::fs::write(marker, format!("dis={id}"));
    TESTING.store(id, Relaxed);
    let finish = |ok: bool| { TESTING.store(0, Relaxed); if ok { let _ = std::fs::remove_file(marker); } };

    if let Err(e) = set_display_enabled(id, false) {
        // set_display_enabled 失败时已经尝试接回
        let back = wait_until(3000, || builtin_present(id));
        finish(back);
        return if back { out("unsupported", format!("disconnect had no effect: {e}")) } else { out("stuck", format!("disconnect failed and the display did not come back: {e}")) };
    }
    // 确认真的消失了
    let gone = wait_until(3000, || !builtin_present(id));
    if gone { std::thread::sleep(std::time::Duration::from_secs(3)); }
    // 接回：最多 3 次，每次等 4 秒
    let mut back = false;
    let mut err = String::new();
    for i in 0..3 {
        if let Err(e) = set_display_enabled(id, true) { err = e; }
        if wait_until(4000, || builtin_present(id)) { back = true; break; }
        crate::nblog!("built-in display test: display {id} not back yet (attempt {})", i + 1);
    }
    finish(back);
    match (gone, back) {
        (true, true) => out("ok", format!("display {id} disconnected and reconnected")),
        (false, true) => out("unsupported", format!("display {id} stayed active after SLSConfigureDisplayEnabled")),
        (_, false) => out("stuck", format!("display {id} did not come back {err}").trim().to_string()),
    }
}

/// 诊断页里的一行：内建屏断开检测
fn builtin_cap() -> Cap {
    let (model, chip, os, _) = machine();
    let tech = format!("SLSConfigureDisplayEnabled · {model} · {chip} · macOS {os}");
    let t = BUILTIN_TEST.lock().ok().and_then(|g| g.clone());
    match (t.as_ref().map(|t| t.result.as_str()), tested_verdict()) {
        (Some("ok"), Some(_)) => Cap::new("builtin", "builtin_ok", "ok", tech),
        (Some("stuck"), Some(_)) => Cap::new("builtin", "builtin_stuck", "bad", tech),
        (Some(_), Some(_)) => Cap::new("builtin", "builtin_unsupported", "off", tech),
        _ => Cap::new("builtin", "builtin_untested", "off", tech),
    }
}

/// 开/关一块屏。关：先用 kCGConfigureForSession（就算我们没机会接回，注销或重启后系统也会还原），
/// 1 秒内没生效再用 kCGConfigurePermanently（Crisp 的经验：有的系统上只有它真正生效）。
/// 开：用 kCGConfigurePermanently——「开着」本来就是默认状态，永久保存也无害，还能盖掉之前永久保存的「关」。
fn set_display_enabled(id: u32, enabled: bool) -> Result {
    if enabled { return configure_display(id, true, 2); }
    configure_display(id, false, 1)?;
    for _ in 0..10 {
        if unsafe { CGDisplayIsActive(id) } == 0 { return Ok(()); }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    crate::nblog!("built-in display {id}: still active after a session-scoped disconnect, retrying permanently");
    configure_display(id, false, 2)?;
    for _ in 0..10 {
        if unsafe { CGDisplayIsActive(id) } == 0 { return Ok(()); }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    // 没断开：别留个永久的「关」在那里
    let _ = configure_display(id, true, 2);
    Err("display stayed active after SLSConfigureDisplayEnabled".into())
}

/// 一个显示配置事务：Begin → SLSConfigureDisplayEnabled → Complete(option)
fn configure_display(id: u32, enabled: bool, option: u32) -> Result {
    let f = display_enable_fn().ok_or("SLSConfigureDisplayEnabled unavailable")?;
    unsafe {
        let mut cfg: *mut c_void = std::ptr::null_mut();
        let e = CGBeginDisplayConfiguration(&mut cfg);
        if e != 0 || cfg.is_null() { return Err(format!("CGBeginDisplayConfiguration = {e}")); }
        let e = f(cfg, id, enabled);
        if e != 0 { CGCancelDisplayConfiguration(cfg); return Err(format!("SLSConfigureDisplayEnabled({id}, {enabled}) = {e}")); }
        let e = CGCompleteDisplayConfiguration(cfg, option);
        if e != 0 { return Err(format!("CGCompleteDisplayConfiguration = {e}")); }
    }
    Ok(())
}

/// 除 `except` 以外，系统当前还在用的真实显示器数。
/// 别的屏都没了时 macOS 会放一个厂商号为 0 / 'unkn' / 'virt' 的占位显示器，不算。
fn other_real_displays(except: u32) -> usize {
    let mut ids = [0u32; 16];
    let mut n = 0u32;
    if unsafe { CGGetActiveDisplayList(16, ids.as_mut_ptr(), &mut n) } != 0 { return 0; }
    let fake = [0, u32::MAX, u32::from_be_bytes(*b"unkn"), u32::from_be_bytes(*b"virt")];
    ids[..(n as usize).min(16)].iter().filter(|&&d| d != except && unsafe { CGDisplayIsActive(d) } != 0 && !fake.contains(&unsafe { CGDisplayVendorNumber(d) })).count()
}

/// 接回我们断开的屏（没有就什么都不做）
fn reconnect_builtin(why: &str) -> Result {
    let d = DISCONNECTED.swap(0, Relaxed);
    if d == 0 { return Ok(()); }
    let r = set_display_enabled(d, true);
    match &r {
        Ok(()) => crate::nblog!("built-in display {d}: reconnected ({why})"),
        // 失败也别丢了这个 id：崩溃标记 / 下次启动还会再试
        Err(e) => { DISCONNECTED.store(d, Relaxed); crate::nblog!("built-in display {d}: reconnect failed ({why}): {e}") }
    }
    r
}

/// 关内建屏：能断开就断开，不行退回背光 0（返回 Err 让引擎补上遮罩）
fn builtin_off(d: u32) -> Result {
    // 正在「检测」时别同时去断开（检测会自己接回）
    if DISCONNECT_FAILED.load(Relaxed) || TESTING.load(Relaxed) != 0 || !disconnect_supported() { return set_builtin_off(d, true); }
    if other_real_displays(d) == 0 {
        set_builtin_off(d, true)?;
        return Err("built-in is the only display: not disconnecting it, backlight 0 instead".into());
    }
    match set_display_enabled(d, false) {
        Ok(()) => {
            DISCONNECTED.store(d, Relaxed);
            crate::nblog!("built-in display {d}: disconnected (SLSConfigureDisplayEnabled), now active={}", unsafe { CGDisplayIsActive(d) } != 0);
            Ok(())
        }
        Err(e) => {
            DISCONNECT_FAILED.store(true, Relaxed);
            set_builtin_off(d, true)?;
            Err(format!("{e}; backlight 0 instead"))
        }
    }
}

static ALLOW_INJECTED: AtomicBool = AtomicBool::new(false);

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

/// 修饰键的 keycode → hotkey 位
fn mod_bit(code: u32) -> u8 {
    match code { 59 | 62 => hotkey::CTRL, 58 | 61 => hotkey::ALT, 56 | 60 => hotkey::SHIFT, 55 | 54 => hotkey::META, _ => 0 }
}

/// 唤醒那一刻还按着、且按下事件被我们吞掉的键
fn held_keys() -> Vec<u32> {
    let mut v: Vec<u32> = (0..128u32).filter(|c| KEY_SEEN[(*c as usize) >> 6].load(Relaxed) & (1u64 << (c & 63)) != 0).collect();
    let seen = MOD_SEEN.load(Relaxed);
    for c in [54u32, 55, 56, 58, 59, 60, 61, 62] {
        if seen & mod_bit(c) != 0 && unsafe { CGEventSourceKeyState(1, c as u16) } { v.push(c); }
    }
    v
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
        // 「放行远程控制」：别的进程合成的事件（远程控制、屏幕键盘；kCGEventSourceUnixProcessID ≠ 0）原样放过
        if ALLOW_INJECTED.load(Relaxed) && blankrun_core::wake::route(CGEventGetIntegerValueField(ev, 41) != 0, true) == blankrun_core::wake::Route::PassThrough {
            return ev;
        }
        // 唤醒后排空：只吞唤醒时按着的键（含修饰键）的重复和抬起，鼠标直接放行
        if super::DRAINING.load(Relaxed) {
            let code = CGEventGetIntegerValueField(ev, 9) as u32;
            let swallow = match ty {
                10 | 11 => super::drain_key(code, ty == 10).unwrap_or(false),
                12 => super::drain_key(code, flags_mods(CGEventGetFlags(ev)) & mod_bit(code) != 0).unwrap_or(false),
                _ => false,
            };
            return if swallow { std::ptr::null_mut() } else { ev };
        }
        let t = now_ms();
        // 「键鼠照常用」：键鼠照常送给前台 App；按键仍然上报，用来认唤醒键 / 紧急键（只吞这两个组合）
        if super::KEEP_INPUT.load(Relaxed) { return keep_input_event(ty, ev, t); }
        // 装 tap 之前就按着的键（比如刚按的 ⌃⌥B），抬起要放行，否则别的 App 会以为修饰键一直按着
        match ty {
            10 | 11 => {
                let code = CGEventGetIntegerValueField(ev, 9) as usize & 127;
                let (w, bit) = (&KEY_SEEN[code >> 6], 1u64 << (code & 63));
                if ty == 10 { w.fetch_or(bit, Relaxed); } else if w.fetch_and(!bit, Relaxed) & bit == 0 { return ev; }
            }
            12 => {
                let (now_m, prev) = (flags_mods(CGEventGetFlags(ev)), MOD_PREV.swap(flags_mods(CGEventGetFlags(ev)), Relaxed));
                let (pressed, released) = (now_m & !prev, prev & !now_m);
                if pressed != 0 { MOD_SEEN.fetch_or(pressed, Relaxed); }
                if released != 0 && MOD_SEEN.fetch_and(!released, Relaxed) & released == 0 { return ev; }
            }
            _ => {}
        }
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

/// 「键鼠照常用」时的 tap 回调：返回 ev = 放行，null = 吞掉
unsafe fn keep_input_event(ty: u32, ev: *mut c_void, t: u64) -> *mut c_void {
    match ty {
        10 | 11 => {
            let code = CGEventGetIntegerValueField(ev, 9);
            let (key, mods, down) = (keycode_name(code), flags_mods(CGEventGetFlags(ev)), ty == 10);
            let swallow = super::keep_key(key, mods, down);
            // 只记下被吞掉的键：唤醒时它还按着的话，排空会把它的重复和抬起也吞掉；放行的键不用管
            let c = code as usize & 127;
            let (w, bit) = (&KEY_SEEN[c >> 6], 1u64 << (c & 63));
            if swallow && down { w.fetch_or(bit, Relaxed); } else if !down { w.fetch_and(!bit, Relaxed); }
            emit_input(Input::Key { key, mods, down, t_ms: t });
            if swallow { std::ptr::null_mut() } else { ev }
        }
        12 => {
            // 修饰键永远放行（不记进 MOD_SEEN，唤醒后也不排空它们）
            let mods = flags_mods(CGEventGetFlags(ev));
            MOD_PREV.store(mods, Relaxed);
            emit_input(Input::Key { key: "Mod", mods, down: mods != 0, t_ms: t });
            ev
        }
        // 鼠标移动 / 拖动：要进关掉的屏就挡在边上
        5 | 6 | 7 | 27 => {
            let p = CGEventGetLocation(ev);
            if let Some((x, y)) = super::fence_point(p.x, p.y) {
                let q = CGPoint { x, y };
                CGEventSetLocation(ev, q);
                CGWarpMouseCursorPosition(q);
                // warp 后系统默认会冻结本地鼠标约 0.25 秒；马上重新关联，光标不卡顿
                CGAssociateMouseAndMouseCursorPosition(1);
            }
            ev
        }
        _ => ev,
    }
}

static KEY_SEEN: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];
static MOD_SEEN: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
static MOD_PREV: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

fn tap_thread(ready: std::sync::mpsc::Sender<std::result::Result<(), String>>) {
    for w in &KEY_SEEN { w.store(0, Relaxed); }
    MOD_SEEN.store(0, Relaxed);
    MOD_PREV.store(flags_mods(unsafe { CGEventSourceFlagsState(1 /* HID system state */) }), Relaxed);
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
    /// 唤醒后排空键盘的后台线程
    drain: Option<JoinHandle<()>>,
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
    fn configure(&mut self, cfg: &Config) {
        self.ac_only = cfg.power.ac_only;
        ALLOW_INJECTED.store(cfg.wake.allow_remote, Relaxed);
        if let Ok(mut g) = BUILTIN_OFF.lock() { *g = cfg.display.builtin_off.clone(); }
        if let Ok(mut g) = BUILTIN_TEST.lock() { *g = cfg.display.builtin_test.clone(); }
    }

    fn probe_monitor(&mut self, id: &str, x: f64, y: f64) -> super::MonHw {
        let did = display_at(x, y);
        let builtin = did.is_some_and(|d| unsafe { CGDisplayIsBuiltin(d) } != 0);
        let virtual_ = blankrun_core::monitors::looks_virtual(id);
        let power = if builtin && disconnect_supported() { Some("disconnect") } else { (builtin && display_services().is_some()).then_some("backlight") };
        super::MonHw { label: None, virtual_, power, detail: did.map_or(String::new(), |d| format!("CGDisplay {d} {}px{}", unsafe { CGDisplayPixelsWide(d) }, if builtin { " built-in" } else { "" })) }
    }

    fn monitor_power(&mut self, _id: &str, x: f64, y: f64, off: bool) -> Result {
        if !off {
            DISCONNECT_FAILED.store(false, Relaxed);
            let r = reconnect_builtin("wake");
            return r.and(set_builtin_off(0, false));
        }
        // 已经断开了（断开的屏不在活动列表里，按坐标找不到它）：被系统接回来了就再断开一次
        let dis = DISCONNECTED.load(Relaxed);
        if dis != 0 {
            if unsafe { CGDisplayIsActive(dis) } == 0 { return Ok(()); }
            DISCONNECTED.store(0, Relaxed);
            return builtin_off(dis);
        }
        let d = display_at(x, y).ok_or("display not found")?;
        if unsafe { CGDisplayIsBuiltin(d) } == 0 { return Err("external display: no DDC on macOS yet".into()); }
        builtin_off(d)
    }

    fn display_guard(&mut self) -> bool {
        static RESCUE: std::sync::Mutex<blankrun_core::monitors::Rescue> = std::sync::Mutex::new(blankrun_core::monitors::Rescue::new());
        let d = DISCONNECTED.load(Relaxed);
        let mut r = RESCUE.lock().unwrap_or_else(|p| p.into_inner());
        if d == 0 { r.reset(); return false; }
        if !r.feed(other_real_displays(d), super::now_ms()) { return false; }
        r.reset();
        crate::nblog!("no other display left for {}s: reconnecting the built-in display", blankrun_core::monitors::RESCUE_MS / 1000);
        let _ = reconnect_builtin("no other display");
        true
    }

    /// 关掉的内建屏是不是又亮了（亮度键、自动亮度）：读 DisplayServices 亮度
    fn monitor_lit(&mut self, _id: &str, x: f64, y: f64) -> Option<bool> {
        // 断开的内建屏：被系统接回来（休眠唤醒、开合盖）就算「又亮了」
        let dis = DISCONNECTED.load(Relaxed);
        if dis != 0 { return Some(unsafe { CGDisplayIsActive(dis) } != 0); }
        let d = display_at(x, y)?;
        if unsafe { CGDisplayIsBuiltin(d) } == 0 { return None; }
        let (get, _) = display_services()?;
        let mut v = 0f32;
        (unsafe { get(d, &mut v) } == 0).then_some(v > 0.001)
    }

    fn display_rect(&mut self, x: f64, y: f64, fallback: super::Rect) -> super::Rect {
        match display_at(x, y) {
            Some(d) => { let r = unsafe { CGDisplayBounds(d) }; super::Rect { x: r.x, y: r.y, w: r.w, h: r.h } }
            None => fallback,
        }
    }

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
            if bits != u32::MAX {
                let target = f32::from_bits(bits);
                crate::nblog!("keyboard backlight restored to {target} (now {:?})", kb_get());
                // 屏幕还没完全亮起来时 macOS 会把背光自己压回 0：屏幕亮了以后再补几次
                if target > 0.0 {
                    let _ = std::thread::Builder::new().name("nb-kbd".into()).spawn(move || {
                        for i in 0..10 {
                            std::thread::sleep(std::time::Duration::from_millis(500));
                            if KB_SAVED.load(Relaxed) != u32::MAX { return; } // 又开始息屏了
                            match kb_get() {
                                Some(v) if v + 0.001 >= target => { if i > 0 { crate::nblog!("keyboard backlight back at {v}"); } return; }
                                _ => { kb_set(target); }
                            }
                        }
                        crate::nblog!("keyboard backlight still {:?} after retries (target {target})", kb_get());
                    });
                }
            }
        }
        Ok(())
    }

    fn input_hook(&mut self, on: bool) -> Result {
        if on {
            if self.tap.is_some() { return Ok(()); }
            if let Some(j) = self.drain.take() { let _ = j.join(); }
            TAP_STOP.store(false, Relaxed);
            let (tx, rx) = std::sync::mpsc::channel();
            let h = std::thread::Builder::new().name("nb-tap".into()).spawn(move || tap_thread(tx)).map_err(|e| e.to_string())?;
            match rx.recv().map_err(|e| e.to_string()).and_then(|r| r) {
                Ok(()) => { self.tap = Some(h); self.tap_err = None; Ok(()) }
                Err(e) => { let _ = h.join(); self.tap_err = Some(e.clone()); Err(e) }
            }
        } else {
            if let Some(h) = self.tap.take() {
                // 唤醒时还按着的键：tap 继续吞到它们全部抬起，免得自动重复漏给前台 App
                self.drain = super::start_drain(held_keys(), Box::new(move || { TAP_STOP.store(true, Relaxed); let _ = h.join(); }));
            }
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
            Cap::new("ddc", "ddc_mac", if display_services().is_some() { "ok" } else { "off" }, "DisplayServices"),
            builtin_cap(),
            Cap::new("rgb", "rgb_todo", "off", "OpenRGB SDK"),
        ]
    }

    fn permissions(&self) -> Option<(bool, bool)> {
        Some(unsafe { (AXIsProcessTrusted(), CGPreflightListenEventAccess()) })
    }

    fn request_permission(&self, which: &str) {
        // 先发起系统授权请求（没有记录时弹系统授权框），再打开对应的设置页（已有记录时系统不会再弹框）
        let pane = if which == "input" {
            unsafe { CGRequestListenEventAccess(); }
            "Privacy_ListenEvent"
        } else {
            prompt_accessibility();
            "Privacy_Accessibility"
        };
        let _ = std::process::Command::new("/usr/bin/open")
            .arg(format!("x-apple.systempreferences:com.apple.preference.security?{pane}")).spawn();
    }

    fn prompt_permissions(&self) {
        static DONE: AtomicBool = AtomicBool::new(false);
        if DONE.swap(true, Relaxed) { return; } // 每次启动最多一次
        let (ax, input) = unsafe { (AXIsProcessTrusted(), CGPreflightListenEventAccess()) };
        crate::nblog!("permissions at launch: accessibility={ax} input_monitoring={input}");
        if !ax { prompt_accessibility(); }
        if !input { unsafe { CGRequestListenEventAccess(); } }
    }

    fn reset_permissions(&self) -> Result {
        // 清掉旧的（签名不匹配的）授权记录：旧版用临时签名，0.1.7 起换成 Developer ID，
        // 系统设置里的开关可能是开着的，但对新签名无效
        let id = bundle_id();
        let mut errs = vec![];
        for service in ["Accessibility", "ListenEvent"] {
            match std::process::Command::new("/usr/bin/tccutil").args(["reset", service, &id]).output() {
                Ok(o) if o.status.success() => crate::nblog!("tccutil reset {service} {id}: ok"),
                Ok(o) => { let e = format!("tccutil reset {service} {id}: {} {}", o.status, String::from_utf8_lossy(&o.stderr).trim()); crate::nblog!("{e}"); errs.push(e); }
                Err(e) => { crate::nblog!("tccutil: {e}"); errs.push(e.to_string()); }
            }
        }
        // 重新发起请求
        prompt_accessibility();
        unsafe { CGRequestListenEventAccess(); }
        if errs.is_empty() { Ok(()) } else { Err(errs.join("; ")) }
    }

    fn persist(&self) -> Option<String> {
        let bits = KB_SAVED.load(Relaxed);
        let mut s = if bits == u32::MAX { "kbd=".to_string() } else { format!("kbd={}", f32::from_bits(bits)) };
        if let Some((id, v)) = BL_SAVED.lock().ok().and_then(|g| *g) { s.push_str(&format!("\nbl={id}:{v}")); }
        let d = DISCONNECTED.load(Relaxed);
        if d != 0 { s.push_str(&format!("\ndis={d}")); }
        Some(s)
    }

    fn set_launch_at_login(&mut self, on: bool) -> Result {
        let p = launch_agent().ok_or("no HOME")?;
        if !on { return match std::fs::remove_file(&p) { Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()), _ => Ok(()) }; }
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        // 在 .app 里时用 open -a 启动整个 App（LaunchServices），否则直接跑可执行文件
        let app = exe.ancestors().nth(3).filter(|a| a.extension().is_some_and(|x| x == "app"));
        let args: Vec<String> = match app {
            // --autostart：登录时静默启动到菜单栏，不弹设置窗口
            Some(a) => vec!["/usr/bin/open".into(), "-a".into(), a.display().to_string(), "--args".into(), "--autostart".into()],
            None => vec![exe.display().to_string(), "--autostart".into()],
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
        let mut r = Ok(());
        for line in s.lines() {
            if let Some(v) = line.strip_prefix("kbd=").and_then(|v| v.trim().parse::<f32>().ok()) {
                if !kb_set(v) { r = Err("restoring keyboard backlight failed".into()); }
            }
            if let Some((id, v)) = line.strip_prefix("bl=").and_then(|v| v.split_once(':')).and_then(|(a, b)| Some((a.parse::<u32>().ok()?, b.trim().parse::<f32>().ok()?))) {
                if let Some((_, set)) = display_services() { unsafe { set(id, v); } crate::nblog!("built-in display {id}: brightness restored to {v} after abnormal exit"); }
            }
            // 上次断开了内建屏就退出了：接回来（重新登录 / 重启过的话系统已经还原，这里不会出错）
            if let Some(id) = line.strip_prefix("dis=").and_then(|v| v.trim().parse::<u32>().ok()) {
                match set_display_enabled(id, true) {
                    Ok(()) => crate::nblog!("built-in display {id}: reconnected after abnormal exit"),
                    Err(e) => { crate::nblog!("built-in display {id}: reconnect after abnormal exit failed: {e}"); r = Err(e); }
                }
            }
        }
        r
    }
}

impl Drop for Native {
    fn drop(&mut self) {
        let _ = self.input_hook(false);
        if let Some(j) = self.drain.take() { let _ = j.join(); }
        let _ = reconnect_builtin("exit");
        let _ = self.keep_awake(false);
    }
}

/// panic 钩子里调用：不加锁
pub fn emergency_restore() {
    // 断开的内建屏先接回来（原子量，不加锁）
    let d = DISCONNECTED.swap(0, Relaxed);
    if d != 0 { let _ = set_display_enabled(d, true); }
    let d = TESTING.swap(0, Relaxed);
    if d != 0 { let _ = set_display_enabled(d, true); }
    let bits = KB_SAVED.swap(u32::MAX, Relaxed);
    if bits != u32::MAX { kb_set(f32::from_bits(bits)); }
    if let (Ok(mut g), Some((_, set))) = (BL_SAVED.try_lock(), display_services()) {
        if let Some((id, v)) = g.take() { unsafe { set(id, v); } }
    }
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

/// 辅助功能：AXIsProcessTrustedWithOptions(kAXTrustedCheckOptionPrompt = true)，没有授权记录时弹系统授权框
fn prompt_accessibility() -> bool {
    unsafe {
        let keys = [kAXTrustedCheckOptionPrompt];
        let vals = [kCFBooleanTrue];
        let opts = CFDictionaryCreate(std::ptr::null(), keys.as_ptr(), vals.as_ptr(), 1,
            &kCFTypeDictionaryKeyCallBacks as *const _ as *const c_void, &kCFTypeDictionaryValueCallBacks as *const _ as *const c_void);
        let ok = AXIsProcessTrustedWithOptions(opts);
        if !opts.is_null() { CFRelease(opts); }
        ok
    }
}

/// 本 App 的 bundle id（tauri.conf.json 的 identifier）。从 .app 里的 Info.plist 读，读不到用默认值
fn bundle_id() -> String {
    let id = std::env::current_exe().ok()
        .and_then(|e| e.ancestors().nth(2).map(|c| c.join("Info.plist")))
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| {
            let s = String::from_utf8_lossy(&b).into_owned();
            let i = s.find("<key>CFBundleIdentifier</key>")?;
            let rest = &s[i..];
            let a = rest.find("<string>")? + 8;
            let z = rest[a..].find("</string>")?;
            Some(rest[a..a + z].trim().to_string())
        });
    id.filter(|s| !s.is_empty()).unwrap_or_else(|| "com.cosfoc.blankrun".into())
}

/// 重启 App：先在后台排一个「1.5 秒后重新打开」，再由调用方退出（单实例锁要等本进程退出才释放）
pub fn relaunch_later() -> Result {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let target = exe.ancestors().nth(3).filter(|a| a.extension().is_some_and(|x| x == "app")).map(|a| a.to_path_buf()).unwrap_or(exe);
    std::process::Command::new("/bin/sh").arg("-c").arg("sleep 1.5; /usr/bin/open \"$0\"").arg(&target).spawn().map_err(|e| e.to_string())?;
    crate::nblog!("relaunch scheduled: {}", target.display());
    Ok(())
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
