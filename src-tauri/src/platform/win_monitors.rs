//! Windows 逐块显示器：友好名（DisplayConfig）、虚拟显示器识别（显卡驱动描述）、DDC/CI 断电（VCP 0xD6）。
//! DDC/CI 走 dxva2 的 Monitor Configuration API，每次调用几十毫秒，显示器要在 OSD 里打开 DDC/CI。
use std::collections::HashMap;

use windows_sys::Win32::{
    Devices::Display::*,
    Foundation::{BOOL, LPARAM, RECT},
    Graphics::Gdi::*,
};

use super::MonHw;

fn wstr(w: &[u16]) -> String { String::from_utf16_lossy(&w[..w.iter().position(|&c| c == 0).unwrap_or(w.len())]) }

/// \\.\DISPLAY1 → "HP X27q"
pub fn friendly_names() -> HashMap<String, String> {
    let mut out = HashMap::new();
    unsafe {
        let (mut np, mut nm) = (0u32, 0u32);
        if GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut np, &mut nm) != 0 { return out; }
        let mut paths: Vec<DISPLAYCONFIG_PATH_INFO> = vec![std::mem::zeroed(); np as usize];
        let mut modes: Vec<DISPLAYCONFIG_MODE_INFO> = vec![std::mem::zeroed(); nm as usize];
        if QueryDisplayConfig(QDC_ONLY_ACTIVE_PATHS, &mut np, paths.as_mut_ptr(), &mut nm, modes.as_mut_ptr(), std::ptr::null_mut()) != 0 { return out; }
        for p in &paths[..np as usize] {
            let mut src: DISPLAYCONFIG_SOURCE_DEVICE_NAME = std::mem::zeroed();
            src.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME;
            src.header.size = std::mem::size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32;
            src.header.adapterId = p.sourceInfo.adapterId;
            src.header.id = p.sourceInfo.id;
            let mut tgt: DISPLAYCONFIG_TARGET_DEVICE_NAME = std::mem::zeroed();
            tgt.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME;
            tgt.header.size = std::mem::size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32;
            tgt.header.adapterId = p.targetInfo.adapterId;
            tgt.header.id = p.targetInfo.id;
            if DisplayConfigGetDeviceInfo(&mut src.header) == 0 && DisplayConfigGetDeviceInfo(&mut tgt.header) == 0 {
                let name = wstr(&tgt.monitorFriendlyDeviceName);
                if !name.is_empty() { out.insert(wstr(&src.viewGdiDeviceName), name); }
            }
        }
    }
    out
}

/// \\.\DISPLAY1 所在显卡的描述（"NVIDIA GeForce RTX 4080 SUPER" / "Parsec Virtual Display Adapter"）
pub fn adapter_string(dev: &str) -> String {
    unsafe {
        for i in 0.. {
            let mut d: DISPLAY_DEVICEW = std::mem::zeroed();
            d.cb = std::mem::size_of::<DISPLAY_DEVICEW>() as u32;
            if EnumDisplayDevicesW(std::ptr::null(), i, &mut d, 0) == 0 { break; }
            if wstr(&d.DeviceName).eq_ignore_ascii_case(dev) { return wstr(&d.DeviceString); }
        }
    }
    String::new()
}

unsafe extern "system" fn collect(h: HMONITOR, _dc: HDC, _r: *mut RECT, data: LPARAM) -> BOOL {
    let v = &mut *(data as *mut Vec<(HMONITOR, String)>);
    let mut mi: MONITORINFOEXW = std::mem::zeroed();
    mi.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
    if GetMonitorInfoW(h, &mut mi as *mut _ as *mut MONITORINFO) != 0 { v.push((h, wstr(&mi.szDevice))); }
    1
}

fn hmonitor(dev: &str) -> Option<HMONITOR> {
    let mut v: Vec<(HMONITOR, String)> = Vec::new();
    unsafe { EnumDisplayMonitors(std::ptr::null_mut(), std::ptr::null(), Some(collect), &mut v as *mut _ as LPARAM); }
    v.into_iter().find(|(_, n)| n.eq_ignore_ascii_case(dev)).map(|(h, _)| h)
}

/// 对这块屏上的每个物理显示器句柄调用 f（通常只有一个）；返回第一个结果
fn with_physical<R>(dev: &str, mut f: impl FnMut(windows_sys::Win32::Foundation::HANDLE) -> Option<R>) -> Option<R> {
    let h = hmonitor(dev)?;
    unsafe {
        let mut n = 0u32;
        if GetNumberOfPhysicalMonitorsFromHMONITOR(h, &mut n) == 0 || n == 0 { return None; }
        let mut a: Vec<PHYSICAL_MONITOR> = vec![std::mem::zeroed(); n as usize];
        if GetPhysicalMonitorsFromHMONITOR(h, n, a.as_mut_ptr()) == 0 { return None; }
        let mut r = None;
        for p in &a { if let Some(x) = f(p.hPhysicalMonitor) { r = Some(x); break; } }
        DestroyPhysicalMonitors(n, a.as_ptr());
        r
    }
}

/// VCP D6（电源模式）当前值：1 = 开，2–5 = 各级待机/关；None = 读不到（不支持 DDC/CI 或 OSD 里关了）。
/// DDC/CI 走 I²C，偶尔会失败或回垃圾值（实测 SAC DM73u 偶发 err 203、HP X27q 唤醒后回过 0xAAAA），所以重试几次、只认 1–5。
pub fn ddc_power(dev: &str) -> Option<u32> {
    for i in 0..3 {
        if i > 0 { std::thread::sleep(std::time::Duration::from_millis(60)); }
        let v = with_physical(dev, |h| unsafe {
            let (mut cur, mut max) = (0u32, 0u32);
            (GetVCPFeatureAndVCPFeatureReply(h, 0xD6, std::ptr::null_mut(), &mut cur, &mut max) != 0).then_some(cur)
        });
        if let Some(v @ 1..=5) = v { return Some(v); }
    }
    None
}

/// 写 VCP D6：4 = DPM 关（有信号/再写 1 能唤醒），1 = 开。失败重试一次
pub fn set_ddc_power(dev: &str, v: u32) -> Result<(), String> {
    for i in 0..2 {
        if i > 0 { std::thread::sleep(std::time::Duration::from_millis(100)); }
        if with_physical(dev, |h| unsafe { (SetVCPFeature(h, 0xD6, v) != 0).then_some(()) }).is_some() { return Ok(()); }
    }
    Err(format!("SetVCPFeature(D6={v}) failed on {dev}"))
}

pub fn probe(dev: &str, names: &HashMap<String, String>) -> MonHw {
    let adapter = adapter_string(dev);
    let label = names.get(dev).cloned();
    let virtual_ = blankrun_core::monitors::looks_virtual(&adapter) || label.as_deref().is_some_and(blankrun_core::monitors::looks_virtual);
    let power = if virtual_ { None } else { ddc_power(dev).map(|_| "ddc") };
    MonHw { label, virtual_, power, detail: adapter, ..Default::default() }
}

/// 把所有读得到 D6 且不是「开」的显示器重新打开（崩溃恢复 / 紧急恢复）
pub fn wake_all() {
    let mut v: Vec<(HMONITOR, String)> = Vec::new();
    unsafe { EnumDisplayMonitors(std::ptr::null_mut(), std::ptr::null(), Some(collect), &mut v as *mut _ as LPARAM); }
    for (_, dev) in v {
        if ddc_power(&dev).is_some_and(|p| p != 1) { let _ = set_ddc_power(&dev, 1); }
    }
}
