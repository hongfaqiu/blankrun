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
    /// 每天最多一次访问 GitHub 看有没有新版本（只读版本号，不下载、不上传）
    pub check_updates: bool,
}
impl Default for GeneralConfig {
    fn default() -> Self {
        Self { launch_at_login: false, language: LANG_AUTO.into(), blank_now_hotkey: "Ctrl+Alt+B".into(), check_updates: true }
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
    /// 放行远程控制：息屏时远程软件、屏幕键盘发来的「注入」输入照常送达，不算误触也不唤醒；物理屏保持关闭
    pub allow_remote: bool,
}
impl Default for WakeConfig {
    fn default() -> Self {
        Self { method: WakeMethod::Hotkey, hotkey: "Ctrl+Alt+W".into(), mouse_threshold_px: 150,
               mouse_window_ms: 800, lock_on_wake: false, reblank_after_s: 3, hold_ms: 1000, allow_remote: false }
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
pub struct DisplayConfig {
    pub mode: String,
    pub targets: Vec<String>,
    pub method: DisplayMethod,
    /// 每台显示器（按显示名）手动指定："overlay" 仅遮罩 | "off" 断电；没写 = 自动
    pub prefs: std::collections::BTreeMap<String, String>,
    /// 自动发现「接受 DDC/CI 断电指令但其实不关」的显示器（按显示名），以后只用遮罩
    pub no_ddc: Vec<String>,
    /// 「键鼠照常用」（Mac Mini 模式）：只关所选、且还有屏亮着时，本机键盘鼠标照常送给前台 App，
    /// 不算误触也不唤醒；只认唤醒快捷键 / 紧急键 / 菜单栏 / 计划结束。见 DESIGN.md §3
    pub keep_input: bool,
    /// 仅 macOS：内建屏单独关闭时怎么关。"auto"：Apple 芯片 + macOS 13 起「断开」（与合盖相同，面板进待机），
    /// M3 系列和 iMac 用背光调 0；"disconnect"：能断开就断开（含 M3）；"backlight"：只把背光调到 0
    pub builtin_off: String,
}
impl Default for DisplayConfig {
    fn default() -> Self { Self { mode: "all".into(), targets: vec![], method: DisplayMethod::Auto, prefs: Default::default(), no_ddc: vec![], keep_input: false, builtin_off: "auto".into() } }
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PowerConfig {
    /// 息屏期间阻止系统睡眠（默认开）。关掉后按系统自己的睡眠设置走，关屏、拦截输入、唤醒都不变
    pub keep_awake: bool,
    /// 仅 macOS：只在接通电源时防睡眠，用电池时照常睡眠。默认关闭
    pub ac_only: bool,
}
impl Default for PowerConfig {
    fn default() -> Self { Self { keep_awake: true, ac_only: false } }
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
        // 手改配置文件写出离谱的数也别出事：夹到合理范围
        let w = &mut self.wake;
        w.hold_ms = w.hold_ms.min(10_000);
        w.reblank_after_s = w.reblank_after_s.clamp(1, 600);
        w.mouse_threshold_px = w.mouse_threshold_px.clamp(10, 10_000);
        w.mouse_window_ms = w.mouse_window_ms.clamp(100, 10_000);
        self.schedule.remind_before_min = self.schedule.remind_before_min.min(240);
        for t in [&self.schedule.start, &self.schedule.end] {
            if crate::schedule::parse_hhmm(t).is_none() { return Err(format!("invalid time: {t} (use HH:MM)")); }
        }
        if !["auto", "disconnect", "backlight"].contains(&self.display.builtin_off.as_str()) { self.display.builtin_off = "auto".into(); }
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
        assert!(c.general.check_updates);
        // 旧配置没有 allow_remote：默认不放行
        assert!(!Config::from_toml("[wake]\nmethod = \"hotkey\"\n").unwrap().wake.allow_remote);
        // 0.1.0 的配置没有 keep_awake：迁移成开
        assert!(Config::from_toml("[power]\nac_only = true\n").unwrap().power.keep_awake);
        assert!(!Config::from_toml("[power]\nkeep_awake = false\n").unwrap().power.keep_awake);
        assert_eq!(c.wake.hold_ms, 1000);
        // 0.1.5 及以前的配置没有 keep_input：默认关（照旧拦截全部输入）
        assert!(!Config::from_toml("[display]\nmode = \"selected\"\ntargets = [\"Built-in Retina Display\"]\n").unwrap().display.keep_input);
        assert!(Config::from_toml("[display]\nkeep_input = true\n").unwrap().display.keep_input);
        // 没有 builtin_off：auto；写错的值回到 auto
        assert_eq!(c.display.builtin_off, "auto");
        assert_eq!(Config::from_toml("[display]\nbuiltin_off = \"backlight\"\n").unwrap().display.builtin_off, "backlight");
        assert_eq!(Config::from_toml("[display]\nbuiltin_off = \"zap\"\n").unwrap().display.builtin_off, "auto");
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

    #[test]
    fn monitor_prefs_roundtrip() {
        // 每台显示器的设置能写回 TOML 再读出来
        let mut c = Config::default();
        c.display.prefs.insert("DM73u pro".into(), "overlay".into());
        c.display.no_ddc = vec!["DM73u pro".into()];
        let back = Config::from_toml(&c.to_toml()).unwrap();
        assert_eq!(back.display, c.display);
    }

    #[test]
    fn normalize_validates_and_clamps() {
        let mut c = Config::default();
        c.wake.reblank_after_s = 0; c.wake.hold_ms = 99_999; c.schedule.remind_before_min = 9999;
        c.normalize().unwrap();
        assert_eq!((c.wake.reblank_after_s, c.wake.hold_ms, c.schedule.remind_before_min), (1, 10_000, 240));
        let mut c = Config::default(); c.schedule.start = "25:00".into();
        assert!(c.normalize().is_err());
    }
}
