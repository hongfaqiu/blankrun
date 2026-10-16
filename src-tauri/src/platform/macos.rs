use super::{Platform, Result};

#[derive(Default)]
pub struct Native { saved_backlight: Option<f32> }

impl Platform for Native {
    fn keep_awake(&mut self, _on: bool) -> Result {
        // TODO(M0): IOPMAssertionCreateWithName(kIOPMAssertPreventUserIdleSystemSleep)，
        // 等价 `caffeinate -i`，不要用 -d。见 RESEARCH.md §2「防睡眠」。
        Ok(())
    }
    fn display_off(&mut self, _off: bool) -> Result {
        // TODO(M0): 默认 `pmset displaysleepnow` 等价 IOKit 调用（真断电）。
        // 被输入点亮后由状态机 ScreenLitUnexpectedly 触发再关。兜底：DisplayServices 亮度 0 + 黑色遮罩 + gamma。
        // 见 RESEARCH.md §2「真正关掉屏幕」「兜底方案」、§5 风险 1。
        Ok(())
    }
    fn keyboard_backlight(&mut self, _off: bool) -> Result {
        // TODO(M1): CoreBrightness KeyboardBrightnessClient，运行时 dlopen 解析；
        // 先 copyKeyboardBacklightIDs 拿 UInt64 ID（传 0 会无声失败），失效时降级。见 RESEARCH.md §2「键盘背光」。
        let _ = self.saved_backlight;
        Ok(())
    }
    fn input_hook(&mut self, _on: bool) -> Result {
        // TODO(M0): CGEventTapCreate 吞键鼠事件，需辅助功能/输入监控权限；
        // 需实测能否阻止 HID 层点亮屏幕。见 RESEARCH.md §2「防误触」「权限」。
        Ok(())
    }
    fn lock_screen(&mut self) -> Result {
        // TODO(M3): SACLockScreenImmediate（私有）或系统锁屏快捷键。见 DESIGN.md §5。
        Ok(())
    }
}
