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

/// 更新镜像（官网 Cloudflare Pages Functions，见 functions/ 和 mirror/，随 main 自动部署）：latest.json 和安装包都从这里走，失败再回 GitHub。
/// 两条路下到的安装包都用 tauri.conf.json 里内置的公钥校验 minisign 签名，镜像改不了内容。
pub const MIRROR_BASE: &str = "https://blankrun.cosfoc.com";
pub const MIRROR_JSON: &str = "https://blankrun.cosfoc.com/update/latest.json";
pub const GITHUB_JSON: &str = "https://github.com/hongfaqiu/blankrun/releases/latest/download/latest.json";
const GITHUB_DL: &str = "https://github.com/hongfaqiu/blankrun/releases/download/";

/// 镜像上的安装包地址 → GitHub 上同一个文件（镜像下载失败时回退用）。
/// 只认 `https://blankrun.cosfoc.com/dl/<vX.Y.Z>/<文件名>`，别的返回 None
pub fn mirror_to_github(url: &str) -> Option<String> {
    let rest = url.strip_prefix(MIRROR_BASE)?.strip_prefix("/dl/")?;
    let (tag, file) = rest.split_once('/')?;
    let tag_ok = tag.starts_with('v') && parse_version(tag).is_some();
    let file_ok = !file.is_empty() && file.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c)) && !file.starts_with('.');
    (tag_ok && file_ok).then(|| format!("{GITHUB_DL}{tag}/{file}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mirror_fallback_url() {
        assert_eq!(mirror_to_github("https://blankrun.cosfoc.com/dl/v0.1.11/BlankRun_universal.app.tar.gz").as_deref(),
            Some("https://github.com/hongfaqiu/blankrun/releases/download/v0.1.11/BlankRun_universal.app.tar.gz"));
        assert_eq!(mirror_to_github("https://blankrun.cosfoc.com/dl/v0.1.11/BlankRun_0.1.11_x64-setup.exe").as_deref(),
            Some("https://github.com/hongfaqiu/blankrun/releases/download/v0.1.11/BlankRun_0.1.11_x64-setup.exe"));
        // 已经是 GitHub 地址、路径不对、带 .. 的都不改
        assert_eq!(mirror_to_github("https://github.com/hongfaqiu/blankrun/releases/download/v0.1.11/a.tar.gz"), None);
        assert_eq!(mirror_to_github("https://blankrun.cosfoc.com/dl/latest/a.tar.gz"), None);
        assert_eq!(mirror_to_github("https://blankrun.cosfoc.com/dl/v0.1.11/../x"), None);
        assert_eq!(mirror_to_github("https://blankrun.cosfoc.com.evil.com/dl/v0.1.11/a.tar.gz"), None);
    }
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
