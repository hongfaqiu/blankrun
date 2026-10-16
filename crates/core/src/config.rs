//! config.toml 结构，与 DESIGN.md 第 3 节一致。所有字段都有默认值。
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    pub wake: WakeConfig,
    pub schedule: ScheduleConfig,
    pub display: DisplayConfig,
    pub lights: LightsConfig,
    pub safety: SafetyConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WakeMethod { AnyKey, Hotkey, MouseMove, Click, Password, ScheduleOnly }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WakeConfig {
    pub method: WakeMethod,
    pub hotkey: String,
    pub mouse_threshold_px: u32,
    pub mouse_window_ms: u32,
    /// 公开发布版做成设置项，默认关闭，由用户自己选
    pub lock_on_wake: bool,
    /// 屏幕被意外点亮后多少秒内自动再关（RESEARCH.md §5 风险 1 的兜底）
    pub reblank_after_s: u32,
}
impl Default for WakeConfig {
    fn default() -> Self {
        Self { method: WakeMethod::Hotkey, hotkey: "Ctrl+Alt+W".into(), mouse_threshold_px: 150,
               mouse_window_ms: 800, lock_on_wake: false, reblank_after_s: 3 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScheduleConfig {
    pub enabled: bool,
    pub start: String,
    pub end: String,
    pub days: Vec<String>,
}
impl Default for ScheduleConfig {
    fn default() -> Self {
        Self { enabled: false, start: "23:30".into(), end: "07:30".into(),
               days: ["mon","tue","wed","thu","fri","sat","sun"].iter().map(|s| s.to_string()).collect() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayMethod { Auto, PowerOff, Overlay }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DisplayConfig { pub mode: String, pub targets: Vec<String>, pub method: DisplayMethod }
impl Default for DisplayConfig {
    fn default() -> Self { Self { mode: "all".into(), targets: vec![], method: DisplayMethod::Auto } }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LightsConfig { pub keyboard_backlight: bool, pub openrgb: bool, pub openrgb_host: String }
impl Default for LightsConfig {
    fn default() -> Self { Self { keyboard_backlight: true, openrgb: false, openrgb_host: "127.0.0.1:6742".into() } }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SafetyConfig { pub emergency_hotkey: String }
impl Default for SafetyConfig {
    fn default() -> Self { Self { emergency_hotkey: "Ctrl+Alt+Shift+Esc".into() } }
}

impl Config {
    pub fn from_toml(s: &str) -> Result<Self, String> { toml::from_str(s).map_err(|e| e.to_string()) }
    pub fn to_toml(&self) -> String { toml::to_string(self).unwrap_or_default() }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip_and_partial() {
        let c = Config::from_toml("[wake]\nmethod = \"mouse_move\"\n").unwrap();
        assert_eq!(c.wake.method, WakeMethod::MouseMove);
        assert_eq!(c.wake.mouse_threshold_px, 150);
        assert_eq!(Config::from_toml(&c.to_toml()).unwrap(), c);
    }
}
