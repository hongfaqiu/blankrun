//! 快捷键字符串（"Ctrl+Alt+W"）解析与匹配。平台层把原生键码翻译成这里的规范键名。
pub const CTRL: u8 = 1;
pub const ALT: u8 = 2;
pub const SHIFT: u8 = 4;
/// Mac 上是 ⌘，Windows 上是 Win 键
pub const META: u8 = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hotkey { pub mods: u8, pub key: String }

/// 键名规范化：字母数字大写，常见别名统一。
pub fn norm_key(k: &str) -> String {
    let k = k.trim();
    match k.to_ascii_lowercase().as_str() {
        "esc" | "escape" => "Esc".into(),
        "space" | " " => "Space".into(),
        "enter" | "return" => "Enter".into(),
        "tab" => "Tab".into(),
        "backspace" | "delete" => "Backspace".into(),
        s if s.len() > 1 && s.starts_with('f') && s[1..].parse::<u8>().is_ok() => s.to_ascii_uppercase(),
        _ => k.to_ascii_uppercase(),
    }
}

impl Hotkey {
    pub fn parse(s: &str) -> Option<Self> {
        let mut mods = 0;
        let mut key = None;
        for part in s.split('+').map(str::trim).filter(|p| !p.is_empty()) {
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" | "⌃" => mods |= CTRL,
                "alt" | "option" | "opt" | "⌥" => mods |= ALT,
                "shift" | "⇧" => mods |= SHIFT,
                "cmd" | "command" | "meta" | "win" | "super" | "⌘" => mods |= META,
                _ if key.is_none() => key = Some(norm_key(part)),
                _ => return None,
            }
        }
        Some(Self { mods, key: key? })
    }
    /// 能不能当全局快捷键：至少要有 Ctrl / Alt / ⌘(Win) 之一，否则会吞掉正常打字
    pub fn usable_global(&self) -> bool { self.mods & (CTRL | ALT | META) != 0 }
    /// 修饰键必须完全一致（多按一个 Shift 也不算），主键名一致
    pub fn matches(&self, key: &str, mods: u8) -> bool { self.mods == mods && self.key == key }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parse_and_match() {
        let h = Hotkey::parse("Ctrl+Alt+Shift+Esc").unwrap();
        assert_eq!(h, Hotkey { mods: CTRL | ALT | SHIFT, key: "Esc".into() });
        assert!(h.matches("Esc", CTRL | ALT | SHIFT));
        assert!(!h.matches("Esc", CTRL | ALT));
        assert_eq!(Hotkey::parse("ctrl+alt+w").unwrap().key, "W");
        assert_eq!(Hotkey::parse("Cmd+f5").unwrap(), Hotkey { mods: META, key: "F5".into() });
        assert!(Hotkey::parse("Ctrl+Alt").is_none());
        assert!(Hotkey::parse("A+B").is_none());
        assert!(Hotkey::parse("Ctrl+Alt+B").unwrap().usable_global());
        assert!(!Hotkey::parse("Shift+B").unwrap().usable_global());
        assert!(!Hotkey::parse("B").unwrap().usable_global());
    }
}
