//! 每块显示器怎么关：断电（DDC/CI、Mac 内建屏断开）、黑色遮罩，或者不管（虚拟显示器、没选中的）。
//! Mac 内建屏**绝不自动把背光调 0**：断不开时只盖遮罩、亮度不动，并给出原因（`notice`）；背光 0 只在用户明确选了「只关背光」时才用。
//! 平台层负责探测（是不是虚拟屏、能不能单独断电），这里只做决定，便于单测。
use serde::Serialize;

use crate::config::{DisplayConfig, DisplayMethod};

/// 平台探测到的一块显示器
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Monitor {
    /// 与 config.display.targets 匹配的名字（Windows 是 \\.\DISPLAY1，Mac 是系统显示名）
    pub id: String,
    /// 给人看的名字（"HP X27q"）
    pub label: String,
    pub width: u32,
    pub height: u32,
    pub primary: bool,
    /// 虚拟显示器：远程软件 / 头显装的（向日葵/Oray、Parsec、Virtual Desktop、IddCx…），
    /// Mac 上的 BetterDisplay / DeskPad / Sidecar / AirPlay 等（CGVirtualDisplay）。不处理，也不算「别的物理屏」
    pub virtual_: bool,
    /// 能单独断电的方式："ddc"（外接屏 DDC/CI）| "disconnect"（Mac 内建屏断开，同合盖）| "backlight"（Mac 内建屏背光调 0，仅用户明确选了才有）；None = 只能遮罩
    pub power: Option<&'static str>,
    /// 平台层说明为什么没法断电（内建屏）："os" | "risky" | "tested" | "api"；没有就是 None
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Method {
    /// 遮罩 + 单独断电（DDC D6=4 / 背光 0）
    Off,
    /// 只盖黑色遮罩
    Overlay,
    /// 不处理（虚拟显示器、没选中的）
    Skip,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    /// 和传入的 monitors 一一对应
    pub per: Vec<Method>,
    /// 用系统级关屏（Windows SC_MONITORPOWER / Mac pmset displaysleepnow）一次关掉全部物理屏
    pub global_off: bool,
}

/// 名字或驱动描述像不像虚拟显示器
pub fn looks_virtual(s: &str) -> bool {
    let s = s.to_ascii_lowercase();
    ["oray", "sunlogin", "parsec", "virtual", "idd", "spacedesk", "usbmmidd", "deskreen", "todesk", "rustdesk", "dummy", "meta quest", "steamvr"]
        .iter().any(|k| s.contains(k))
}

/// Mac 上的虚拟显示器名字：在 `looks_virtual` 之外再认 BetterDisplay / DeskPad / Sidecar / AirPlay 这类（CGVirtualDisplay 创建的屏）
pub fn looks_virtual_mac(s: &str) -> bool {
    if looks_virtual(s) { return true; }
    let s = s.to_ascii_lowercase();
    ["betterdisplay", "deskpad", "sidecar", "airplay", "luna display", "duet", "splashtop", "headless", "vdisplay"]
        .iter().any(|k| s.contains(k))
}

/// 用户对这台屏的手动设置："auto" | "overlay" | "off"。
/// 按名字（label）找，找不到再按 id 找（Mac 上 label 现在是真实显示名，老配置里存的是 "Monitor #41055"）
pub fn pref<'a>(d: &'a DisplayConfig, m: &Monitor) -> &'a str {
    d.prefs.get(&m.label).or_else(|| d.prefs.get(&m.id)).map(String::as_str).unwrap_or("auto")
}

/// 这台屏被认定「不响应断电指令」，且用户没有强制要求断电
pub fn unresponsive(d: &DisplayConfig, m: &Monitor) -> bool {
    (d.no_ddc.contains(&m.label) || d.no_ddc.contains(&m.id)) && pref(d, m) != "off"
}

/// 「键鼠照常用」（Mac Mini 模式）此刻是否生效：开了 `keep_input`、范围是「只关所选」，
/// 并且至少还有一块物理屏不关——全部关掉时放行键鼠也看不见在操作什么，照旧拦截。
pub fn keep_input_active(d: &DisplayConfig, monitors: &[Monitor]) -> bool {
    d.keep_input && d.mode == "selected"
        && monitors.iter().any(|m| !m.virtual_ && d.targets.contains(&m.id))
        && monitors.iter().any(|m| !m.virtual_ && !d.targets.contains(&m.id))
}

/// Mac 内建屏单独关闭用哪种方式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinOff {
    /// SkyLight `SLSConfigureDisplayEnabled(false)`：从桌面布局里拿掉，面板进待机（与合盖相同）
    Disconnect,
    /// DisplayServices 背光调 0（面板仍在工作，只是不亮）。**只有用户明确选了「只关背光」才会是它**
    Backlight,
    /// 这台 Mac 上断不开（原因码："os" 系统 / 芯片不支持 | "risky" M3、iMac 且没测过 | "tested" 检测没通过）：
    /// 内建屏亮度原样不动，只盖黑色遮罩，并提示原因。不会退到背光 0
    Unavailable(&'static str),
}

/// 按设置和机器决定内建屏怎么关。`setting`：config.display.builtin_off；`chip`：machdep.cpu.brand_string；
/// `model`：hw.model；`os_major`：macOS 主版本；`arm`：Apple 芯片原生运行。
/// 「断开」只在 Apple 芯片 + macOS 13 起可用（Intel 上同一个接口不会真正断开）。
/// auto 时避开 M3 系列（BetterDisplay：部分 M3 机型断开后要重启才能接回来）和 iMac（没有盖子可以开合来救回屏幕）。
/// `tested`：这台机器（同机型、芯片、系统主版本）上「检测」的结果，Some(true) = 断开和接回都成功。
/// auto 时测过的结果优先：成功就断开（包括 M3 / iMac），失败就不断开；没测过按上面的规则。
/// 不能断开时返回 `Unavailable(原因)`，绝不会自动变成 `Backlight`——只有 `setting == "backlight"` 才返回它。
pub fn builtin_off(setting: &str, chip: &str, model: &str, os_major: u32, arm: bool, tested: Option<bool>) -> BuiltinOff {
    let supported = arm && os_major >= 13;
    let risky = chip.starts_with("Apple M3") || model.starts_with("iMac");
    if setting == "backlight" { return BuiltinOff::Backlight; }
    if !supported { return BuiltinOff::Unavailable("os"); }
    if setting == "disconnect" { return BuiltinOff::Disconnect; }
    match tested {
        Some(true) => BuiltinOff::Disconnect,
        Some(false) => BuiltinOff::Unavailable("tested"),
        None if risky => BuiltinOff::Unavailable("risky"),
        None => BuiltinOff::Disconnect,
    }
}

/// 这块屏能不能「断开」：它支持断开，而且断开后还有别的物理屏（不算虚拟屏）亮着。
/// 绝不断开唯一的物理屏——那样用户就什么都看不见了。平台层断开前还会再按系统当前的显示器列表查一次。
pub fn disconnect_ok(m: &Monitor, monitors: &[Monitor]) -> bool {
    m.power == Some("disconnect") && monitors.iter().any(|o| o.id != m.id && !o.virtual_)
}

/// 这块屏现在断不开 / 没断开的原因码（给设置页和日志）：
/// "virtual_only"（别的屏都是虚拟屏，macOS 不让虚拟屏当唯一的活动显示器）| "only_display"（没有别的显示器）|
/// 平台层给的 `m.note`（"os" | "risky" | "tested" | "api"）。虚拟屏本身、能正常处理的屏返回 None。
/// 引擎运行中才发现的原因（"came_back" 被系统反复接回、"disconnect_failed" 系统拒绝）由引擎自己记。
pub fn notice(m: &Monitor, monitors: &[Monitor]) -> Option<&'static str> {
    if m.virtual_ { return None; }
    if m.power == Some("disconnect") && !disconnect_ok(m, monitors) {
        return Some(if monitors.iter().any(|o| o.id != m.id && o.virtual_) { "virtual_only" } else { "only_display" });
    }
    m.note
}

/// `notice` / 引擎原因码的英文日志文字
pub fn notice_text(code: &str) -> &'static str {
    match code {
        "virtual_only" => "Built-in display can't be disconnected: the other displays are virtual (macOS won't keep a virtual display as the only one); brightness untouched, overlay only",
        "only_display" => "Built-in display can't be disconnected: there is no other display; brightness untouched, overlay only",
        "came_back" => "Built-in display can't be disconnected: macOS keeps re-enabling it; stopped trying, brightness untouched, overlay only",
        "disconnect_failed" => "Built-in display can't be disconnected: the system refused; brightness untouched, overlay only",
        "os" => "Built-in display can't be disconnected: needs Apple silicon and macOS 13+; brightness untouched, overlay only",
        "risky" => "Built-in display isn't disconnected automatically on M3 Macs / iMacs until the test passes; brightness untouched, overlay only",
        "tested" => "Built-in display can't be disconnected: the test on this Mac failed; brightness untouched, overlay only",
        "api" => "Built-in display can't be disconnected: the system interface is unavailable; brightness untouched, overlay only",
        _ => "Built-in display can't be turned off on its own",
    }
}

/// 内建屏被 macOS 反复接回来时停手：在 `RELIT_WINDOW_MS` 里第 `RELIT_MAX` 次又被接回，就认定 macOS 不让断开，不再对抗。
pub const RELIT_WINDOW_MS: u64 = 10_000;
pub const RELIT_MAX: usize = 2;
#[derive(Debug, Default, Clone)]
pub struct CameBack { at: Vec<u64> }
impl CameBack {
    pub const fn new() -> Self { Self { at: Vec::new() } }
    /// 又发现它被接回来了；返回 true = 该放弃了（10 秒内已被接回 `RELIT_MAX` 次）
    pub fn feed(&mut self, now: u64) -> bool {
        self.at.retain(|&t| now.saturating_sub(t) < RELIT_WINDOW_MS);
        self.at.push(now);
        self.at.len() >= RELIT_MAX
    }
    pub fn reset(&mut self) { self.at.clear(); }
}

/// 断开内建屏后，如果别的屏全没了（拔了外接屏），macOS 不会自己把内建屏接回来，用户就面对一块黑屏。
/// 「没有别的屏」连续 `RESCUE_MS` 才算数（显示器重新枚举、休眠唤醒时会短暂读到 0）。
pub const RESCUE_MS: u64 = 3000;
#[derive(Debug, Default, Clone)]
pub struct Rescue { zero_since: Option<u64> }
impl Rescue {
    pub const fn new() -> Self { Self { zero_since: None } }
    /// `others`：除断开的内建屏外，系统当前还亮着的真实显示器数；返回 true = 现在就把内建屏接回来
    pub fn feed(&mut self, others: usize, now: u64) -> bool {
        if others > 0 { self.zero_since = None; return false; }
        let s = *self.zero_since.get_or_insert(now);
        now.saturating_sub(s) >= RESCUE_MS
    }
    pub fn reset(&mut self) { self.zero_since = None; }
}

/// 屏幕矩形（Mac：全局坐标的点；Windows：物理像素），左上角原点
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect { pub x: f64, pub y: f64, pub w: f64, pub h: f64 }
impl Rect {
    pub fn contains(&self, px: f64, py: f64) -> bool { px >= self.x && py >= self.y && px < self.x + self.w && py < self.y + self.h }
    /// 矩形内离 (px, py) 最近的点（右/下边界内缩 1，保证点落在矩形里）
    fn clamp(&self, px: f64, py: f64) -> (f64, f64) {
        (px.clamp(self.x, self.x + (self.w - 1.0).max(0.0)), py.clamp(self.y, self.y + (self.h - 1.0).max(0.0)))
    }
}

/// 光标围栏：「键鼠照常用」时光标别跑进已经关掉的屏（在那里看不见、也点不到东西）。
/// 点落在 `blocked` 里时返回 `allowed` 中离它最近的点；不在 blocked 里、或没有可去的屏时返回 None（不动）。
pub fn fence(px: f64, py: f64, blocked: &[Rect], allowed: &[Rect]) -> Option<(f64, f64)> {
    if !blocked.iter().any(|r| r.contains(px, py)) || allowed.iter().any(|r| r.contains(px, py)) { return None; }
    allowed.iter().map(|r| r.clamp(px, py))
        .min_by(|a, b| {
            let d = |p: &(f64, f64)| (p.0 - px).powi(2) + (p.1 - py).powi(2);
            d(a).partial_cmp(&d(b)).unwrap_or(std::cmp::Ordering::Equal)
        })
}

/// 决定每块屏怎么关。
/// - `global_ok`：系统级关屏可用且不会让电脑进低功耗待机（Windows Modern Standby 上为 false）
/// - `remote`：「放行远程控制」开着：不用系统级关屏（它也会关掉远程看到的画面），物理屏逐块断电
///
/// 所有物理屏都要关（「全部显示器」，或「只关所选」选中了全部物理屏）、没开远程控制、也没有哪台被设成仅遮罩时，
/// 走系统级关屏：它对不支持 DDC/CI 或不响应 D6 的显示器（比如 SAC DM73u）也能真正断电。
/// 「键鼠照常用」生效时绝不走系统级关屏（pmset displaysleepnow / SC_MONITORPOWER 会把亮着的外接屏也关掉）。
pub fn plan(d: &DisplayConfig, monitors: &[Monitor], global_ok: bool, remote: bool) -> Plan {
    let selected = d.mode == "selected";
    let targeted = |m: &Monitor| !m.virtual_ && (!selected || d.targets.contains(&m.id));
    let physical = || monitors.iter().filter(|m| !m.virtual_);
    let covers_all = physical().all(targeted) && physical().next().is_some();
    let any_overlay_pref = physical().any(|m| targeted(m) && pref(d, m) == "overlay");
    let global_off = covers_all && !remote && !any_overlay_pref && !keep_input_active(d, monitors) && match d.method {
        DisplayMethod::PowerOff => true,
        DisplayMethod::Overlay => false,
        DisplayMethod::Auto => global_ok,
    };
    let per = monitors.iter().map(|m| {
        if !targeted(m) { Method::Skip }
        else if global_off || d.method == DisplayMethod::Overlay || m.power.is_none() || pref(d, m) == "overlay" || unresponsive(d, m) { Method::Overlay }
        // 断不开（没有别的物理屏：只剩虚拟屏或只有它自己）：亮度不动，只盖遮罩
        else if m.power == Some("disconnect") && !disconnect_ok(m, monitors) { Method::Overlay }
        else { Method::Off }
    }).collect();
    Plan { per, global_off }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mon(id: &str, virtual_: bool, power: Option<&'static str>) -> Monitor {
        Monitor { id: id.into(), label: id.into(), width: 1920, height: 1080, primary: false, virtual_, power, note: None }
    }
    fn cfg(mode: &str, targets: &[&str], method: DisplayMethod) -> DisplayConfig {
        DisplayConfig { mode: mode.into(), targets: targets.iter().map(|s| s.to_string()).collect(), method, ..Default::default() }
    }
    use Method::*;

    #[test]
    fn virtual_names() {
        for s in ["OrayIddDriver Device", "Parsec Virtual Display Adapter", "Virtual Desktop Monitor", "IddSampleDriver Device", "Generic Monitor (ORAYDISPLAY)"] { assert!(looks_virtual(s), "{s}"); }
        for s in ["NVIDIA GeForce RTX 4080 SUPER", "HP X27q", "DM73u pro", "Built-in Retina Display", "DELL U2723QE"] { assert!(!looks_virtual(s), "{s}"); }
    }

    #[test]
    fn all_mode_uses_global_off_and_skips_virtual() {
        let ms = [mon("A", false, Some("ddc")), mon("B", false, None), mon("V", true, None)];
        let p = plan(&cfg("all", &[], DisplayMethod::Auto), &ms, true, false);
        assert!(p.global_off);
        assert_eq!(p.per, vec![Overlay, Overlay, Skip]);
    }

    #[test]
    fn modern_standby_all_mode_uses_ddc_per_monitor() {
        let ms = [mon("A", false, Some("ddc")), mon("B", false, None)];
        let p = plan(&cfg("all", &[], DisplayMethod::Auto), &ms, false, false);
        assert!(!p.global_off);
        assert_eq!(p.per, vec![Off, Overlay]);
    }

    #[test]
    fn selected_mode_powers_off_only_targets() {
        let ms = [mon("A", false, Some("ddc")), mon("B", false, Some("ddc")), mon("C", false, None), mon("V", true, Some("ddc"))];
        let p = plan(&cfg("selected", &["A", "C", "V"], DisplayMethod::Auto), &ms, true, false);
        assert!(!p.global_off);
        assert_eq!(p.per, vec![Off, Skip, Overlay, Skip]);
        // 设成只用遮罩：不断电
        let p = plan(&cfg("selected", &["A"], DisplayMethod::Overlay), &ms, true, false);
        assert_eq!(p.per, vec![Overlay, Skip, Skip, Skip]);
        // 设成断电：选中的照样逐块断电，不走系统级
        let p = plan(&cfg("selected", &["A"], DisplayMethod::PowerOff), &ms, true, false);
        assert!(!p.global_off);
        assert_eq!(p.per[0], Off);
    }

    #[test]
    fn remote_mode_never_global() {
        let ms = [mon("A", false, Some("ddc")), mon("B", false, None), mon("V", true, None)];
        let p = plan(&cfg("all", &[], DisplayMethod::PowerOff), &ms, true, true);
        assert!(!p.global_off);
        assert_eq!(p.per, vec![Off, Overlay, Skip]);
    }

    #[test]
    fn selecting_every_physical_monitor_uses_global_off() {
        let ms = [mon("A", false, Some("ddc")), mon("B", false, None), mon("V", true, None)];
        let p = plan(&cfg("selected", &["A", "B"], DisplayMethod::Auto), &ms, true, false);
        assert!(p.global_off);
        assert_eq!(p.per, vec![Overlay, Overlay, Skip]);
        // 只选一台：不能系统级关
        assert!(!plan(&cfg("selected", &["A"], DisplayMethod::Auto), &ms, true, false).global_off);
    }

    #[test]
    fn overlay_pref_and_unresponsive() {
        let ms = [mon("A", false, Some("ddc")), mon("B", false, Some("ddc"))];
        let mut d = cfg("all", &[], DisplayMethod::Auto);
        // 远程模式：逐台断电；B 被认定不响应 → 遮罩
        d.no_ddc = vec!["B".into()];
        assert_eq!(plan(&d, &ms, true, true).per, vec![Off, Overlay]);
        // 不响应的屏不影响系统级关屏（它能真正关掉）
        assert!(plan(&d, &ms, true, false).global_off);
        // 用户强制「断电」：重新尝试 DDC
        d.prefs.insert("B".into(), "off".into());
        assert_eq!(plan(&d, &ms, true, true).per, vec![Off, Off]);
        // 有屏设成仅遮罩：系统级关屏会把它也关掉，所以不用
        d.prefs.insert("A".into(), "overlay".into());
        let p = plan(&d, &ms, true, false);
        assert!(!p.global_off);
        assert_eq!(p.per, vec![Overlay, Off]);
    }

    #[test]
    fn keep_input_only_with_a_display_left_on() {
        // MacBook 内屏 + 一块外接屏：只关内屏、键鼠照常用
        let ms = [mon("Built-in", false, Some("backlight")), mon("Ext", false, None), mon("V", true, None)];
        let mut d = cfg("selected", &["Built-in"], DisplayMethod::Auto);
        assert!(!keep_input_active(&d, &ms), "off by default");
        d.keep_input = true;
        assert!(keep_input_active(&d, &ms));
        let p = plan(&d, &ms, true, false);
        assert!(!p.global_off);
        assert_eq!(p.per, vec![Off, Skip, Skip]);
        // 两块物理屏都选了：没有亮着的屏可用，照旧（可以系统级关屏）
        let mut all = cfg("selected", &["Built-in", "Ext"], DisplayMethod::Auto);
        all.keep_input = true;
        assert!(!keep_input_active(&all, &ms));
        assert!(plan(&all, &ms, true, false).global_off);
        // 「全部显示器」模式下不生效
        let mut a = cfg("all", &[], DisplayMethod::Auto);
        a.keep_input = true;
        assert!(!keep_input_active(&a, &ms));
        // 什么都没选（或只选了虚拟屏）：不生效
        let mut none = cfg("selected", &["V"], DisplayMethod::Auto);
        none.keep_input = true;
        assert!(!keep_input_active(&none, &ms));
        // 拔掉外接屏后只剩内屏：不生效
        assert!(!keep_input_active(&d, &ms[..1]));
    }

    #[test]
    fn keep_input_never_global_even_with_power_off_method() {
        let ms = [mon("Built-in", false, Some("backlight")), mon("Ext", false, Some("ddc"))];
        let mut d = cfg("selected", &["Built-in"], DisplayMethod::PowerOff);
        d.keep_input = true;
        let p = plan(&d, &ms, true, false);
        assert!(!p.global_off);
        assert_eq!(p.per, vec![Off, Skip]);
    }

    #[test]
    fn cursor_fence() {
        // 外接屏在上方 (0,-1080) 1920×1080，内屏在下方 (0,0) 1512×982
        let ext = Rect { x: 0.0, y: -1080.0, w: 1920.0, h: 1080.0 };
        let builtin = Rect { x: 0.0, y: 0.0, w: 1512.0, h: 982.0 };
        // 在外接屏里：不动
        assert_eq!(fence(100.0, -10.0, &[builtin], &[ext]), None);
        // 跑进内屏：推回外接屏底边
        assert_eq!(fence(700.0, 5.0, &[builtin], &[ext]), Some((700.0, -1.0)));
        // 没有可去的屏（外接屏拔了）：不动，免得光标卡死
        assert_eq!(fence(700.0, 5.0, &[builtin], &[]), None);
        // 两块屏左右并排、内屏在右
        let left = Rect { x: -2560.0, y: 0.0, w: 2560.0, h: 1440.0 };
        assert_eq!(fence(30.0, 2000.0, &[builtin], &[left]), None, "不在关掉的屏里");
        assert_eq!(fence(30.0, 500.0, &[builtin], &[left]), Some((-1.0, 500.0)));
        assert!(builtin.contains(0.0, 0.0) && !builtin.contains(1512.0, 0.0));
    }

    #[test]
    fn builtin_off_choice() {
        use BuiltinOff::*;
        // M1/M2/M4 MacBook，macOS 14：断开
        assert_eq!(builtin_off("auto", "Apple M4", "Mac16,12", 26, true, None), Disconnect);
        assert_eq!(builtin_off("auto", "Apple M1 Pro", "MacBookPro18,3", 14, true, None), Disconnect);
        // macOS 12、Intel：断不开，不退背光
        assert_eq!(builtin_off("auto", "Apple M1", "MacBookAir10,1", 12, true, None), Unavailable("os"));
        assert_eq!(builtin_off("disconnect", "Intel(R) Core(TM) i7", "MacBookPro16,1", 14, false, None), Unavailable("os"));
        // M3 系列、iMac：auto 不断开，手动选 disconnect 才断开
        assert_eq!(builtin_off("auto", "Apple M3", "Mac15,12", 15, true, None), Unavailable("risky"));
        assert_eq!(builtin_off("auto", "Apple M3 Pro", "Mac15,6", 15, true, None), Unavailable("risky"));
        assert_eq!(builtin_off("disconnect", "Apple M3", "Mac15,12", 15, true, None), Disconnect);
        assert_eq!(builtin_off("auto", "Apple M1", "iMac21,1", 14, true, None), Unavailable("risky"));
        // 用户明确选只调背光
        assert_eq!(builtin_off("backlight", "Apple M4", "Mac16,12", 26, true, None), Backlight);
        // 检测过：auto 按检测结果（M3 测过成功就断开；测过失败就不断开）；不支持的系统测成功也不断开
        assert_eq!(builtin_off("auto", "Apple M3", "Mac15,12", 15, true, Some(true)), Disconnect);
        assert_eq!(builtin_off("auto", "Apple M1", "iMac21,1", 14, true, Some(true)), Disconnect);
        assert_eq!(builtin_off("auto", "Apple M4", "Mac16,12", 26, true, Some(false)), Unavailable("tested"));
        assert_eq!(builtin_off("auto", "Apple M1", "MacBookAir10,1", 12, true, Some(true)), Unavailable("os"));
        // 手动选择不受检测结果影响
        assert_eq!(builtin_off("disconnect", "Apple M4", "Mac16,12", 26, true, Some(false)), Disconnect);
        assert_eq!(builtin_off("backlight", "Apple M3", "Mac15,12", 15, true, Some(true)), Backlight);
    }

    #[test]
    fn never_auto_backlight() {
        // 不管什么机器、什么设置、什么检测结果：只有 setting == "backlight" 才会得到 Backlight
        for setting in ["auto", "disconnect", "", "whatever"] {
            for (chip, model, os, arm) in [("Apple M4", "Mac16,12", 26, true), ("Apple M3", "Mac15,12", 15, true), ("Apple M1", "iMac21,1", 14, true),
                ("Apple M1", "MacBookAir10,1", 12, true), ("Intel(R) Core(TM) i7", "MacBookPro16,1", 14, false)] {
                for tested in [None, Some(true), Some(false)] {
                    assert_ne!(builtin_off(setting, chip, model, os, arm, tested), BuiltinOff::Backlight, "{setting} {chip} {model} {os} {tested:?}");
                }
            }
        }
        assert_eq!(builtin_off("backlight", "Intel(R) Core(TM) i7", "MacBookPro16,1", 12, false, Some(false)), BuiltinOff::Backlight);
    }

    #[test]
    fn never_disconnect_the_only_display() {
        let b = Monitor { power: Some("disconnect"), ..mon("Built-in", false, None) };
        let ext = mon("Ext", false, None);
        let v = mon("V", true, None);
        assert!(disconnect_ok(&b, &[b.clone(), ext.clone()]));
        assert!(!disconnect_ok(&b, &[b.clone()]), "唯一的屏");
        assert!(!disconnect_ok(&b, &[b.clone(), v]), "只剩虚拟屏，macOS 不让它当唯一的显示器");
        let bl = Monitor { power: Some("backlight"), ..b.clone() };
        assert!(!disconnect_ok(&bl, &[bl.clone(), ext]));
        // 断开的屏照样算「单独断电」，计划里是 Off、不走系统级关屏
        let mut d = cfg("selected", &["Built-in"], DisplayMethod::Auto);
        d.keep_input = true;
        assert_eq!(plan(&d, &[b, mon("Ext", false, None)], true, false).per, vec![Off, Skip]);
    }

    #[test]
    fn virtual_display_does_not_enable_disconnect() {
        // 用户的 MacBook：内建屏 + BetterDisplay 的虚拟屏 VirtMitm。选中内建屏，不能断开它（macOS 会立刻接回来、屏幕闪一下）
        let b = Monitor { power: Some("disconnect"), label: "Color LCD".into(), ..mon("Monitor #41055", false, None) };
        let v = Monitor { label: "VirtMitm".into(), ..mon("Monitor #38145", true, None) };
        let ms = [b.clone(), v.clone()];
        assert!(!disconnect_ok(&b, &ms));
        let d = cfg("selected", &["Monitor #41055"], DisplayMethod::Auto);
        for remote in [false, true] {
            let p = plan(&d, &ms, true, remote);
            assert_eq!(p.per, vec![Overlay, Skip], "内建屏只盖遮罩、虚拟屏不处理 (remote={remote})");
        }
        assert_eq!(notice(&b, &ms), Some("virtual_only"));
        assert_eq!(notice(&v, &ms), None);
        // 接上一块真实外接屏：可以断开
        let ext = mon("Monitor #1", false, Some("ddc"));
        let ms2 = [b.clone(), v, ext];
        assert!(disconnect_ok(&b, &ms2));
        assert_eq!(plan(&d, &ms2, true, false).per, vec![Off, Skip, Skip]);
        assert_eq!(notice(&b, &ms2), None);
        // 只有它自己
        assert_eq!(notice(&b, &[b.clone()]), Some("only_display"));
        assert_eq!(plan(&d, &[b.clone()], true, true).per, vec![Overlay]);
    }

    #[test]
    fn explicit_backlight_still_powers_off() {
        // 用户明确选了「只关背光」：power = "backlight"，照旧单独断电（不需要别的屏）
        let b = Monitor { power: Some("backlight"), ..mon("Built-in", false, None) };
        let d = cfg("selected", &["Built-in"], DisplayMethod::Auto);
        assert_eq!(plan(&d, &[b.clone()], true, true).per, vec![Off]);
        assert_eq!(notice(&b, &[b.clone()]), None);
    }

    #[test]
    fn unavailable_builtin_gets_overlay_and_note() {
        // 断不开（power = None，note 给原因）：只盖遮罩，亮度不动，设置页能看到原因
        for why in ["os", "risky", "tested", "api"] {
            let b = Monitor { note: Some(why), ..mon("Built-in", false, None) };
            let ext = mon("Ext", false, Some("ddc"));
            let d = cfg("selected", &["Built-in"], DisplayMethod::Auto);
            assert_eq!(plan(&d, &[b.clone(), ext.clone()], true, true).per, vec![Overlay, Skip], "{why}");
            assert_eq!(notice(&b, &[b.clone(), ext]), Some(why));
            assert!(!notice_text(why).is_empty());
        }
    }

    #[test]
    fn mac_virtual_names() {
        for s in ["BetterDisplay VirtualScreen", "VirtMitm Virtual", "DeskPad Display", "Sidecar Display", "AirPlay Display", "Luna Display", "Duet Display", "Parsec Virtual Display Adapter"] {
            assert!(looks_virtual_mac(s), "{s}");
        }
        for s in ["Color LCD", "Built-in Retina Display", "DELL U2723QE", "LG UltraFine", "Studio Display", "Pro Display XDR"] {
            assert!(!looks_virtual_mac(s), "{s}");
        }
    }

    #[test]
    fn old_id_keyed_prefs_still_match() {
        // 老配置按 "Monitor #41055" 存了每台屏的设置；label 现在是真实显示名，也要匹配
        let m = Monitor { label: "Color LCD".into(), ..mon("Monitor #41055", false, Some("disconnect")) };
        let mut d = cfg("selected", &["Monitor #41055"], DisplayMethod::Auto);
        assert_eq!(pref(&d, &m), "auto");
        d.prefs.insert("Monitor #41055".into(), "overlay".into());
        assert_eq!(pref(&d, &m), "overlay");
        d.prefs.insert("Color LCD".into(), "off".into());
        assert_eq!(pref(&d, &m), "off", "按名字的优先");
        d.prefs.clear();
        d.no_ddc = vec!["Monitor #41055".into()];
        assert!(unresponsive(&d, &m));
    }

    #[test]
    fn came_back_gives_up_after_two_within_ten_seconds() {
        let mut c = CameBack::default();
        assert!(!c.feed(1_000), "第一次：再断开一次");
        assert!(c.feed(6_000), "10 秒内第二次：不再对抗");
        // 隔得够久不算
        let mut c = CameBack::default();
        assert!(!c.feed(0));
        assert!(!c.feed(RELIT_WINDOW_MS));
        assert!(c.feed(RELIT_WINDOW_MS + 1_000));
        c.reset();
        assert!(!c.feed(RELIT_WINDOW_MS + 2_000));
    }

    #[test]
    fn rescue_after_external_unplugged() {
        let mut r = Rescue::default();
        assert!(!r.feed(1, 0));
        // 短暂读到 0（重新枚举）：不救
        assert!(!r.feed(0, 100));
        assert!(!r.feed(1, 1500));
        assert!(!r.feed(0, 2000));
        assert!(!r.feed(0, 2000 + RESCUE_MS - 1));
        // 连续 3 秒没有别的屏：接回内建屏
        assert!(r.feed(0, 2000 + RESCUE_MS));
        r.reset();
        assert!(!r.feed(0, 10_000));
    }
}
