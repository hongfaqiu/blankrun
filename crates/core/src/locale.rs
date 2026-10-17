//! 界面语言选择。界面文案在 ui/i18n/<代码>.js；Rust 侧只需要知道选哪个代码（托盘菜单的初始文字用）。
//! ui/common.js 里的 pickLocale 是同一套规则，两边要一致。

/// `pref`：配置里的 language（"auto" 或语言代码）；`system`：系统首选语言（如 "zh-Hans-CN"、"en-US"）；
/// `available`：ui/i18n 里有的语言代码。规则：完全匹配 → 中文按简繁/地区匹配 → 主语言匹配 → "en" → 第一个。
pub fn resolve(pref: &str, system: &str, available: &[&str]) -> String {
    let want = if pref == crate::config::LANG_AUTO || pref.is_empty() { system } else { pref };
    if let Some(c) = match_one(want, available) { return c; }
    if want != system { if let Some(c) = match_one(system, available) { return c; } }
    if available.contains(&"en") { return "en".into(); }
    available.first().map(|s| s.to_string()).unwrap_or_else(|| "en".into())
}

fn match_one(want: &str, available: &[&str]) -> Option<String> {
    let w = want.replace('_', "-").to_ascii_lowercase();
    if w.is_empty() { return None; }
    if let Some(c) = available.iter().find(|c| c.to_ascii_lowercase() == w) { return Some(c.to_string()); }
    let main = w.split('-').next().unwrap_or("");
    if main == "zh" {
        // zh-Hant / zh-TW / zh-HK / zh-MO 是繁体，其余（zh、zh-Hans、zh-CN、zh-SG）按简体
        let trad = w.contains("hant") || w.ends_with("-tw") || w.ends_with("-hk") || w.ends_with("-mo");
        let pick = if trad { "zh-tw" } else { "zh-cn" };
        if let Some(c) = available.iter().find(|c| c.to_ascii_lowercase() == pick) { return Some(c.to_string()); }
    }
    available.iter().find(|c| c.to_ascii_lowercase().split('-').next() == Some(main)).map(|c| c.to_string())
}

#[cfg(test)]
mod tests {
    use super::resolve;
    const AV: &[&str] = &["en", "zh-CN"];
    #[test]
    fn auto_follows_system() {
        assert_eq!(resolve("auto", "zh-Hans-CN", AV), "zh-CN");
        assert_eq!(resolve("auto", "zh_CN", AV), "zh-CN");
        assert_eq!(resolve("auto", "zh-Hant-TW", AV), "zh-CN"); // 还没有繁体时回落到同主语言
        assert_eq!(resolve("auto", "en-US", AV), "en");
        assert_eq!(resolve("auto", "fr-FR", AV), "en");
        assert_eq!(resolve("auto", "", AV), "en");
    }
    #[test]
    fn explicit_choice() {
        assert_eq!(resolve("en", "zh-CN", AV), "en");
        assert_eq!(resolve("zh-CN", "en-US", AV), "zh-CN");
        assert_eq!(resolve("fr", "zh-CN", AV), "zh-CN"); // 选的语言不在了 → 跟随系统
        assert_eq!(resolve("zh-TW", "en", &["en", "zh-CN", "zh-TW"]), "zh-TW");
        assert_eq!(resolve("auto", "zh-HK", &["en", "zh-CN", "zh-TW"]), "zh-TW");
    }
}
