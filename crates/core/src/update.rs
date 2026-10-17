//! 检查更新：只比较版本号、决定多久查一次。网络请求和安装在 App 层（tauri-plugin-updater）。

/// "v0.1.2" / "0.1.2" → (0,1,2)。带预发布后缀（"0.2.0-beta"）的不算正式版，返回 None。
pub fn parse_version(s: &str) -> Option<(u32, u32, u32)> {
    let s = s.trim().trim_start_matches('v');
    if s.contains('-') { return None; }
    let mut it = s.split('.').map(|p| p.parse::<u32>().ok());
    let v = (it.next()??, it.next().unwrap_or(Some(0))?, it.next().unwrap_or(Some(0))?);
    if it.next().is_some() { return None; }
    Some(v)
}

/// latest 比 current 新（current 是预发布版时按它的主版本号比较）
pub fn is_newer(latest: &str, current: &str) -> bool {
    let cur = current.trim().trim_start_matches('v').split('-').next().unwrap_or("");
    match (parse_version(latest), parse_version(cur)) {
        (Some(l), Some(c)) => l > c || (l == c && current.contains('-')),
        _ => false,
    }
}

/// 最多每天查一次；时钟被往回调过也重新查
pub const INTERVAL_S: u64 = 24 * 3600;
pub fn due(last_check_unix: Option<u64>, now_unix: u64) -> bool {
    match last_check_unix { None => true, Some(t) => now_unix < t || now_unix - t >= INTERVAL_S }
}


#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn versions() {
        assert_eq!(parse_version("v0.1.2"), Some((0, 1, 2)));
        assert_eq!(parse_version("1.2"), Some((1, 2, 0)));
        assert_eq!(parse_version("0.2.0-beta"), None);
        assert_eq!(parse_version("x"), None);
        assert!(is_newer("v0.1.1", "0.1.0"));
        assert!(is_newer("v0.2.0", "0.1.9"));
        assert!(!is_newer("v0.1.0", "0.1.0"));
        assert!(!is_newer("v0.1.0", "0.1.1"));
        assert!(is_newer("v0.1.0", "0.1.0-alpha"));
        assert!(!is_newer("v0.2.0-rc1", "0.1.0"));
        assert!(!is_newer("garbage", "0.1.0"));
    }
    #[test]
    fn interval() {
        assert!(due(None, 100));
        assert!(!due(Some(100), 100 + 3600));
        assert!(due(Some(100), 100 + INTERVAL_S));
        assert!(due(Some(1000), 10));
    }
}
