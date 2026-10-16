//! Linux 等非目标平台：仅用于在 CI/开发机上编译检查，全部 no-op。
use super::{Platform, Result};

#[derive(Default)]
pub struct Native;

impl Platform for Native {
    fn keep_awake(&mut self, _on: bool) -> Result { Ok(()) }
    fn display_off(&mut self, _off: bool) -> Result { Ok(()) }
    fn keyboard_backlight(&mut self, _off: bool) -> Result { Ok(()) }
    fn input_hook(&mut self, _on: bool) -> Result { Ok(()) }
    fn lock_screen(&mut self) -> Result { Ok(()) }
}
