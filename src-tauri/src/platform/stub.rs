//! Linux 等非目标平台：仅用于在开发机上编译检查，全部 no-op。
use super::{Cap, Platform, Result};

#[derive(Default)]
pub struct Native;

impl Platform for Native {
    fn keep_awake(&mut self, _on: bool) -> Result { Ok(()) }
    fn display_off(&mut self, _off: bool) -> Result { Ok(()) }
    fn keyboard_backlight(&mut self, _off: bool) -> Result { Ok(()) }
    fn input_hook(&mut self, _on: bool) -> Result { Ok(()) }
    fn lock_screen(&mut self) -> Result { Ok(()) }
    fn capabilities(&mut self) -> Vec<Cap> { vec![Cap::new("stub", "stub_off", "off", std::env::consts::OS)] }
}

pub fn emergency_restore() {}

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
    let _ = std::process::Command::new("xdg-open").arg(path.parent().unwrap_or(path)).spawn();
}

pub fn system_locale() -> String {
    ["LC_ALL", "LC_MESSAGES", "LANG"].iter().filter_map(|k| std::env::var(k).ok()).find(|v| !v.is_empty() && v != "C" && v != "POSIX")
        .map(|v| v.split('.').next().unwrap_or("").replace('_', "-")).unwrap_or_default()
}
