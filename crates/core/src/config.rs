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
    /// 外观："system" 跟随系统 | "light" 浅色 | "dark" 深色
    pub theme: String,
}
pub const THEMES: [&str; 3] = ["system", "light", "dark"];
impl Default for GeneralConfig {
    fn default() -> Self {
        Self { launch_at_login: false, language: LANG_AUTO.into(), blank_now_hotkey: "Ctrl+Alt+B".into(), check_updates: true,
               theme: "system".into() }
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

/// 最多几组定时
pub const MAX_SCHEDULES: usize = 10;
/// 时段名称最多几个字
pub const MAX_LABEL_CHARS: usize = 24;
pub const ALL_DAYS: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];

/// 定时计划。`enabled` 是总开关；每组时段在 `entries` 里各自设开始、结束、星期。
///
/// ```toml
/// [schedule]
/// enabled = true
/// remind_before_min = 5
///
/// [[schedule.entries]]
/// label = "工作日"
/// start = "23:30"
/// end = "07:30"
/// days = ["mon", "tue", "wed", "thu", "fri"]
/// ```
///
/// 0.1.8 及以前只有一组，写在 `[schedule]` 里的 start / end / days；读取时自动迁移成一组 entries。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, from = "ScheduleRaw")]
pub struct ScheduleConfig {
    pub enabled: bool,
    /// 开始前多少分钟弹通知（可推迟 / 跳过今晚），0 = 不提醒
    pub remind_before_min: u32,
    /// 有程序在全屏播放视频时跳过本次计划
    pub skip_when_fullscreen_video: bool,
    /// 各组时段；重叠或首尾相接的时段合并成一段
    pub entries: Vec<ScheduleEntry>,
}
impl Default for ScheduleConfig {
    fn default() -> Self {
        Self { enabled: false, remind_before_min: 5, skip_when_fullscreen_video: true, entries: vec![ScheduleEntry::default()] }
    }
}

/// 一组定时：每周哪几天、几点到几点。跨午夜（23:30–07:30）的时段算在开始那天
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScheduleEntry {
    pub enabled: bool,
    /// 可选的名称（如「工作日」），只用于显示
    #[serde(skip_serializing_if = "String::is_empty")]
    pub label: String,
    pub start: String,
    pub end: String,
    /// 不写时默认每天；写空列表 = 哪天都不生效
    pub days: Vec<String>,
}
impl Default for ScheduleEntry {
    fn default() -> Self {
        Self { enabled: true, label: String::new(), start: "23:30".into(), end: "07:30".into(),
               days: ALL_DAYS.iter().map(|s| s.to_string()).collect() }
    }
}

/// 读取用的中间结构：兼容旧版单组写法（[schedule] 里的 start / end / days）
#[derive(Deserialize)]
#[serde(default)]
struct ScheduleRaw {
    enabled: bool,
    remind_before_min: u32,
    skip_when_fullscreen_video: bool,
    entries: Option<Vec<ScheduleEntry>>,
    start: Option<String>,
    end: Option<String>,
    days: Option<Vec<String>>,
}
impl Default for ScheduleRaw {
    fn default() -> Self {
        let d = ScheduleConfig::default();
        Self { enabled: d.enabled, remind_before_min: d.remind_before_min, skip_when_fullscreen_video: d.skip_when_fullscreen_video,
               entries: None, start: None, end: None, days: None }
    }
}
impl From<ScheduleRaw> for ScheduleConfig {
    fn from(r: ScheduleRaw) -> Self {
        let entries = r.entries.unwrap_or_else(|| {
            // 旧配置（或没写 entries）：迁移成一组，没写的字段用默认值
            let d = ScheduleEntry::default();
            vec![ScheduleEntry { enabled: true, label: String::new(), start: r.start.unwrap_or(d.start),
                                 end: r.end.unwrap_or(d.end), days: r.days.unwrap_or(d.days) }]
        });
        Self { enabled: r.enabled, remind_before_min: r.remind_before_min, skip_when_fullscreen_video: r.skip_when_fullscreen_video, entries }
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
    /// 仅 macOS：「设置 › 息屏 › 内建屏 › 检测」的结果。只在同一机型、芯片、系统主版本上算数，
    /// builtin_off = "auto" 时测过成功就断开（包括 M3），测过失败就不断开，没测过按默认规则
    #[serde(skip_serializing_if = "Option::is_none")]
    pub builtin_test: Option<BuiltinTest>,
}
impl Default for DisplayConfig {
    fn default() -> Self { Self { mode: "all".into(), targets: vec![], method: DisplayMethod::Auto, prefs: Default::default(), no_ddc: vec![], keep_input: false, builtin_off: "auto".into(), builtin_test: None } }
}

/// 内建屏断开检测的结果
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct BuiltinTest {
    /// "ok"：断开和接回都成功；"unsupported"：这台机器 / 系统不能断开（只能背光 + 遮罩）；"stuck"：断开后接不回
    pub result: String,
    /// hw.model，例如 "Mac15,12"
    pub model: String,
    /// machdep.cpu.brand_string，例如 "Apple M3"
    pub chip: String,
    /// kern.osproductversion，例如 "15.1"
    pub os: String,
    /// 检测时间（unix 秒）
    pub at: u64,
}
pub const BUILTIN_TEST_RESULTS: [&str; 3] = ["ok", "unsupported", "stuck"];
impl BuiltinTest {
    /// 这次检测对当前机器还算不算数：机型、芯片、系统主版本都一样才算。Some(true) = 可以断开
    pub fn verdict(&self, chip: &str, model: &str, os_major: u32) -> Option<bool> {
        let major = self.os.split('.').next().and_then(|v| v.parse::<u32>().ok())?;
        (self.model == model && self.chip == chip && major == os_major).then_some(self.result == "ok")
    }
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
        if self.schedule.entries.len() > MAX_SCHEDULES { return Err(format!("too many schedules (max {MAX_SCHEDULES})")); }
        for e in &mut self.schedule.entries {
            for t in [&mut e.start, &mut e.end] {
                let m = crate::schedule::parse_hhmm(t).ok_or_else(|| format!("invalid time: {t} (use HH:MM)"))?;
                *t = format!("{:02}:{:02}", m / 60, m % 60); // "7:5" → "07:05"
            }
            // 星期：小写、去掉不认识的和重复的，按周一到周日排好
            let days: Vec<String> = e.days.iter().map(|d| d.trim().to_ascii_lowercase()).collect();
            e.days = ALL_DAYS.iter().filter(|d| days.iter().any(|x| x == *d)).map(|d| d.to_string()).collect();
            e.label = e.label.trim().chars().filter(|c| !c.is_control()).take(MAX_LABEL_CHARS).collect();
        }
        if !THEMES.contains(&self.general.theme.as_str()) { self.general.theme = "system".into(); }
        if !["auto", "disconnect", "backlight"].contains(&self.display.builtin_off.as_str()) { self.display.builtin_off = "auto".into(); }
        if self.display.builtin_test.as_ref().is_some_and(|t| !BUILTIN_TEST_RESULTS.contains(&t.result.as_str())) { self.display.builtin_test = None; }
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
        assert_eq!(c.schedule.entries.len(), 1);
        assert_eq!(c.schedule.entries[0].days.len(), 7);
        assert_eq!(c.general.theme, "system");
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
    fn builtin_test_roundtrip_and_verdict() {
        let mut c = Config::default();
        assert!(!c.to_toml().contains("builtin_test"));
        c.display.builtin_test = Some(BuiltinTest { result: "ok".into(), model: "Mac15,12".into(), chip: "Apple M3".into(), os: "15.1".into(), at: 1 });
        let back = Config::from_toml(&c.to_toml()).unwrap();
        assert_eq!(back, c);
        let t = back.display.builtin_test.unwrap();
        assert_eq!(t.verdict("Apple M3", "Mac15,12", 15), Some(true));
        // 换了系统主版本 / 机型：不算数
        assert_eq!(t.verdict("Apple M3", "Mac15,12", 26), None);
        assert_eq!(t.verdict("Apple M3", "Mac15,13", 15), None);
        let stuck = BuiltinTest { result: "stuck".into(), ..t.clone() };
        assert_eq!(stuck.verdict("Apple M3", "Mac15,12", 15), Some(false));
        // 写错的结果丢掉
        assert_eq!(Config::from_toml("[display.builtin_test]\nresult = \"zap\"\n").unwrap().display.builtin_test, None);
    }

    #[test]
    fn normalize_validates_and_clamps() {
        let mut c = Config::default();
        c.wake.reblank_after_s = 0; c.wake.hold_ms = 99_999; c.schedule.remind_before_min = 9999;
        c.normalize().unwrap();
        assert_eq!((c.wake.reblank_after_s, c.wake.hold_ms, c.schedule.remind_before_min), (1, 10_000, 240));
        let mut c = Config::default(); c.schedule.entries[0].start = "25:00".into();
        assert!(c.normalize().is_err());
        let mut c = Config::default(); c.schedule.entries.push(ScheduleEntry { end: "7:60".into(), ..Default::default() });
        assert!(c.normalize().is_err());
        // 最多 10 组
        let mut c = Config::default(); c.schedule.entries = vec![ScheduleEntry::default(); MAX_SCHEDULES];
        assert!(c.normalize().is_ok());
        c.schedule.entries.push(ScheduleEntry::default());
        assert!(c.normalize().is_err());
        // 时间补零、星期规范化、名称截断
        let mut c = Config::default();
        c.schedule.entries[0] = ScheduleEntry { start: "7:5".into(), days: vec!["SUN".into(), "mon".into(), "xyz".into(), "mon".into()],
                                                label: "  ".to_string() + &"长".repeat(40), ..Default::default() };
        c.normalize().unwrap();
        let e = &c.schedule.entries[0];
        assert_eq!((e.start.as_str(), e.days.clone()), ("07:05", vec!["mon".to_string(), "sun".to_string()]));
        assert_eq!(e.label.chars().count(), MAX_LABEL_CHARS);
    }

    #[test]
    fn legacy_schedule_migrates() {
        // 0.1.8 及以前：单组写在 [schedule] 里
        let old = "[schedule]\nenabled = true\nstart = \"22:15\"\nend = \"06:45\"\ndays = [\"mon\", \"wed\", \"fri\"]\nremind_before_min = 10\nskip_when_fullscreen_video = false\n";
        let c = Config::from_toml(old).unwrap();
        let s = &c.schedule;
        assert!(s.enabled && !s.skip_when_fullscreen_video);
        assert_eq!(s.remind_before_min, 10);
        assert_eq!(s.entries, vec![ScheduleEntry { enabled: true, label: String::new(), start: "22:15".into(), end: "06:45".into(),
                                                   days: vec!["mon".into(), "wed".into(), "fri".into()] }]);
        // 写回去是新格式，不再有旧字段；再读一遍不变
        let t = c.to_toml();
        let head = &t[t.find("[schedule]").unwrap()..t.find("[[schedule.entries]]").unwrap()];
        assert!(!head.contains("start") && !head.contains("days"), "{t}");
        assert_eq!(Config::from_toml(&t).unwrap(), c);
        // 旧配置只写了一部分：其余用默认值（23:30–07:30、每天）
        let c = Config::from_toml("[schedule]\nenabled = true\nstart = \"21:00\"\n").unwrap();
        assert_eq!(c.schedule.entries.len(), 1);
        assert_eq!((c.schedule.entries[0].start.as_str(), c.schedule.entries[0].end.as_str()), ("21:00", "07:30"));
        assert_eq!(c.schedule.entries[0].days.len(), 7);
        // 只开了开关、没写时间：与旧版默认一致
        assert_eq!(Config::from_toml("[schedule]\nenabled = true\n").unwrap().schedule.entries, vec![ScheduleEntry::default()]);
        // 旧版写错的时间仍然报错
        assert!(Config::from_toml("[schedule]\nstart = \"24:00\"\n").is_err());
        // 新格式里同时写了 entries，旧字段忽略；entries = [] 表示一组都没有
        let c = Config::from_toml("[schedule]\nstart = \"21:00\"\n[[schedule.entries]]\nstart = \"01:00\"\nend = \"05:00\"\n").unwrap();
        assert_eq!(c.schedule.entries.len(), 1);
        assert_eq!(c.schedule.entries[0].start, "01:00");
        assert!(Config::from_toml("[schedule]\nentries = []\n").unwrap().schedule.entries.is_empty());
        // 界面经 JSON 传过来的配置也走同一套
        let j: Config = serde_json::from_str(r#"{"schedule":{"enabled":true,"start":"20:00","end":"21:00","days":["sat"]}}"#).unwrap();
        assert_eq!((j.schedule.entries[0].start.as_str(), j.schedule.entries[0].days.len()), ("20:00", 1));
    }

    #[test]
    fn theme_parse() {
        assert_eq!(Config::from_toml("[general]\ntheme = \"dark\"\n").unwrap().general.theme, "dark");
        assert_eq!(Config::from_toml("[general]\ntheme = \"light\"\n").unwrap().general.theme, "light");
        assert_eq!(Config::from_toml("[general]\ntheme = \"neon\"\n").unwrap().general.theme, "system");
    }
}
