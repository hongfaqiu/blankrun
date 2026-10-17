//! config.toml 结构，与 DESIGN.md 第 3 节一致。所有字段都有默认值。
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    pub general: GeneralConfig,
    pub wake: WakeConfig,
    pub schedule: ScheduleConfig,
    pub display: DisplayConfig,
    pub lights: LightsConfig,
    pub safety: SafetyConfig,
    pub power: PowerConfig,
}

/// 界面语言："auto" 跟随系统，否则是 ui/i18n/ 下的语言代码（如 "zh-CN"、"en"）。
/// 新增语言只需加一个 ui/i18n/<代码>.js，这里不用改。
pub const LANG_AUTO: &str = "auto";

/// 语言代码是否合法：auto，或 BCP 47 形式（字母开头，2–3 位主语言 + 可选子标签）
pub fn valid_language(s: &str) -> bool {
    if s == LANG_AUTO { return true; }
    let mut parts = s.split('-');
    let main = parts.next().unwrap_or("");
    (2..=3).contains(&main.len()) && main.chars().all(|c| c.is_ascii_alphabetic())
        && parts.all(|p| (2..=8).contains(&p.len()) && p.chars().all(|c| c.is_ascii_alphanumeric()))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralConfig {
    /// 登录 / 开机时启动；公开发布版默认关闭，由用户自己打开
    pub launch_at_login: bool,
    pub language: String,
    /// 「立即息屏」全局快捷键（Mac 上显示为 ⌃⌥B）
    pub blank_now_hotkey: String,
}
impl Default for GeneralConfig {
    fn default() -> Self {
        Self { launch_at_login: false, language: LANG_AUTO.into(), blank_now_hotkey: "Ctrl+Alt+B".into() }
    }
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
    /// 唤醒快捷键要按住多久才算（毫秒），短按不唤醒
    pub hold_ms: u32,
}
impl Default for WakeConfig {
    fn default() -> Self {
        Self { method: WakeMethod::Hotkey, hotkey: "Ctrl+Alt+W".into(), mouse_threshold_px: 150,
               mouse_window_ms: 800, lock_on_wake: false, reblank_after_s: 3, hold_ms: 1000 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScheduleConfig {
    pub enabled: bool,
    pub start: String,
    pub end: String,
    /// 不写时默认每天
    pub days: Vec<String>,
    /// 开始前多少分钟弹通知（可推迟 / 跳过今晚），0 = 不提醒
    pub remind_before_min: u32,
    /// 有程序在全屏播放视频时跳过本次计划
    pub skip_when_fullscreen_video: bool,
}
impl Default for ScheduleConfig {
    fn default() -> Self {
        Self { enabled: false, start: "23:30".into(), end: "07:30".into(),
               days: ["mon","tue","wed","thu","fri","sat","sun"].iter().map(|s| s.to_string()).collect(),
               remind_before_min: 5, skip_when_fullscreen_video: true }
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct PowerConfig {
    /// 仅 macOS：只在接通电源时防睡眠，用电池时照常睡眠。默认关闭
    pub ac_only: bool,
}

impl Config {
    pub fn from_toml(s: &str) -> Result<Self, String> {
        let mut c: Self = toml::from_str(s).map_err(|e| e.to_string())?;
        c.normalize()?;
        Ok(c)
    }
    /// 校验并规范化：旧版本写的 language = "zh" 迁移成 "zh-CN"
    pub fn normalize(&mut self) -> Result<(), String> {
        if self.general.language == "zh" { self.general.language = "zh-CN".into(); }
        if !valid_language(&self.general.language) { return Err(format!("invalid language: {}", self.general.language)); }
        Ok(())
    }
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

    #[test]
    fn new_field_defaults() {
        let c = Config::from_toml("").unwrap();
        assert!(!c.general.launch_at_login);
        assert_eq!(c.general.language, "auto");
        assert_eq!(c.general.blank_now_hotkey, "Ctrl+Alt+B");
        assert_eq!(c.wake.hold_ms, 1000);
        assert_eq!(c.schedule.days.len(), 7);
        assert_eq!(c.schedule.remind_before_min, 5);
        assert!(c.schedule.skip_when_fullscreen_video);
        assert!(!c.power.ac_only);
        assert_eq!(c, Config::default());
    }

    #[test]
    fn new_fields_parse() {
        let c = Config::from_toml(
            "[general]\nlanguage = \"en\"\nlaunch_at_login = true\n[power]\nac_only = true\n[schedule]\nremind_before_min = 0\n",
        ).unwrap();
        assert_eq!(c.general.language, "en");
        assert!(c.general.launch_at_login && c.power.ac_only);
        assert_eq!(c.schedule.remind_before_min, 0);
        // 任意合法语言代码都能存（界面没有这个语言时回落到英文）；旧的 "zh" 迁移成 "zh-CN"
        assert_eq!(Config::from_toml("[general]\nlanguage = \"fr\"\n").unwrap().general.language, "fr");
        assert_eq!(Config::from_toml("[general]\nlanguage = \"zh\"\n").unwrap().general.language, "zh-CN");
        assert!(Config::from_toml("[general]\nlanguage = \"../x\"\n").is_err());
        assert!(valid_language("zh-Hant-TW") && valid_language("pt-BR") && !valid_language("") && !valid_language("e"));
        assert_eq!(Config::from_toml(&c.to_toml()).unwrap(), c);
    }
}
