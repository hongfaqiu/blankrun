use super::{Platform, Result};

#[derive(Default)]
pub struct Native;

impl Platform for Native {
    fn keep_awake(&mut self, on: bool) -> Result {
        // 只在息屏期间持有 ES_SYSTEM_REQUIRED（不加 DISPLAY_REQUIRED）。见 RESEARCH.md §3「防睡眠」。
        // TODO(M0): 考虑改用 PowerCreateRequest，便于 `powercfg /requests` 可见；
        // 并实测 Modern Standby 下能否挡住 S0 低功耗待机（RESEARCH.md §3、§5 风险 2）。
        use windows_sys::Win32::System::Power::*;
        let flags = if on { ES_CONTINUOUS | ES_SYSTEM_REQUIRED } else { ES_CONTINUOUS };
        if unsafe { SetThreadExecutionState(flags) } == 0 { return Err("SetThreadExecutionState failed".into()); }
        Ok(())
    }
    fn display_off(&mut self, _off: bool) -> Result {
        // TODO(M0): SendMessage(HWND_BROADCAST, WM_SYSCOMMAND, SC_MONITORPOWER, 2)；
        // Modern Standby 机器上改用每屏顶层黑窗 + SetDeviceGammaRamp；DDC/CI 0xD6=4 放到 M3。见 RESEARCH.md §3。
        Ok(())
    }
    fn keyboard_backlight(&mut self, _off: bool) -> Result {
        // TODO(M2): OpenRGB SDK (TCP 6742) 设全黑；Win11 Dynamic Lighting LampArray 作补充；
        // 厂商笔记本背光走插件（M4）。见 RESEARCH.md §3「RGB」。
        Ok(())
    }
    fn input_hook(&mut self, _on: bool) -> Result {
        // TODO(M2): SetWindowsHookEx(WH_KEYBOARD_LL / WH_MOUSE_LL)，可能触发杀软误报。见 RESEARCH.md §3「输入」。
        Ok(())
    }
    fn lock_screen(&mut self) -> Result {
        // TODO(M3): LockWorkStation()
        Ok(())
    }
}
