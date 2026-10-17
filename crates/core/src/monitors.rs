//! 每块显示器怎么关：断电（DDC/CI 或内建屏背光调 0）、黑色遮罩，或者不管（虚拟显示器、没选中的）。
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
    /// 远程软件 / 头显装的虚拟显示器（向日葵/Oray、Parsec、Virtual Desktop、IddCx…）
    pub virtual_: bool,
    /// 能单独断电的方式："ddc"（外接屏 DDC/CI）| "backlight"（Mac 内建屏背光调 0）；None = 只能遮罩
    pub power: Option<&'static str>,
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

/// 用户对这台屏的手动设置："auto" | "overlay" | "off"
pub fn pref<'a>(d: &'a DisplayConfig, m: &Monitor) -> &'a str {
    d.prefs.get(&m.label).map(String::as_str).unwrap_or("auto")
}

/// 这台屏被认定「不响应断电指令」，且用户没有强制要求断电
pub fn unresponsive(d: &DisplayConfig, m: &Monitor) -> bool {
    d.no_ddc.contains(&m.label) && pref(d, m) != "off"
}

/// 决定每块屏怎么关。
/// - `global_ok`：系统级关屏可用且不会让电脑进低功耗待机（Windows Modern Standby 上为 false）
/// - `remote`：「放行远程控制」开着：不用系统级关屏（它也会关掉远程看到的画面），物理屏逐块断电
///
/// 所有物理屏都要关（「全部显示器」，或「只关所选」选中了全部物理屏）、没开远程控制、也没有哪台被设成仅遮罩时，
/// 走系统级关屏：它对不支持 DDC/CI 或不响应 D6 的显示器（比如 SAC DM73u）也能真正断电。
pub fn plan(d: &DisplayConfig, monitors: &[Monitor], global_ok: bool, remote: bool) -> Plan {
    let selected = d.mode == "selected";
    let targeted = |m: &Monitor| !m.virtual_ && (!selected || d.targets.contains(&m.id));
    let physical = || monitors.iter().filter(|m| !m.virtual_);
    let covers_all = physical().all(targeted) && physical().next().is_some();
    let any_overlay_pref = physical().any(|m| targeted(m) && pref(d, m) == "overlay");
    let global_off = covers_all && !remote && !any_overlay_pref && match d.method {
        DisplayMethod::PowerOff => true,
        DisplayMethod::Overlay => false,
        DisplayMethod::Auto => global_ok,
    };
    let per = monitors.iter().map(|m| {
        if !targeted(m) { Method::Skip }
        else if global_off || d.method == DisplayMethod::Overlay || m.power.is_none() || pref(d, m) == "overlay" || unresponsive(d, m) { Method::Overlay }
        else { Method::Off }
    }).collect();
    Plan { per, global_off }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mon(id: &str, virtual_: bool, power: Option<&'static str>) -> Monitor {
        Monitor { id: id.into(), label: id.into(), width: 1920, height: 1080, primary: false, virtual_, power }
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
}
