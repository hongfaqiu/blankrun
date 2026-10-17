//! 自动更新（tauri-plugin-updater）：启动后稍等一会儿、之后每天最多查一次 GitHub 上的 latest.json。
//! 有新版本只在设置和面板里安静地提示，用户点「更新并重启」才下载安装；息屏时不更新。
//! 安装包用 minisign 签名（公钥在 tauri.conf.json），签名不对就拒绝安装。
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::AppHandle;
use tauri_plugin_updater::{Update, UpdaterExt};

use blankrun_core::update;

pub const SITE_URL: &str = "https://blankrun.cosfoc.com";
pub const REPO_URL: &str = "https://github.com/hongfaqiu/blankrun";
pub const RELEASES_URL: &str = "https://github.com/hongfaqiu/blankrun/releases/latest";

static ENABLED: AtomicBool = AtomicBool::new(true);
static BUSY: AtomicBool = AtomicBool::new(false);
static PENDING: Mutex<Option<Update>> = Mutex::new(None);
static STATE: Mutex<UpdState> = Mutex::new(UpdState { phase: "idle", version: None, progress: None, error: None, updated_from: None });

/// 给界面看的更新状态（随 get_status 一起发）
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct UpdState {
    /// idle | checking | latest | available | downloading | installing | error
    pub phase: &'static str,
    /// 新版本号（"0.1.2"）
    pub version: Option<String>,
    /// 下载进度 0–100
    pub progress: Option<u8>,
    pub error: Option<String>,
    /// 刚从哪个版本自动更新过来（Mac 上提示重新授权）
    pub updated_from: Option<String>,
}

fn set(f: impl FnOnce(&mut UpdState)) { if let Ok(mut g) = STATE.lock() { f(&mut g); } }
pub fn state() -> UpdState { STATE.lock().map(|g| g.clone()).unwrap_or(UpdState { phase: "idle", version: None, progress: None, error: None, updated_from: None }) }

pub fn set_enabled(on: bool) { ENABLED.store(on, Relaxed); }

fn unix_now() -> u64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()) }

/// 状态文件：`<上次检查的 unix 秒> <运行过的版本>`
fn read_state(p: &PathBuf) -> (Option<u64>, Option<String>) {
    let s = std::fs::read_to_string(p).unwrap_or_default();
    let mut it = s.split_whitespace();
    (it.next().and_then(|t| t.parse().ok()), it.next().map(str::to_string))
}
fn write_state(p: &PathBuf, last: Option<u64>) {
    let _ = std::fs::write(p, format!("{} {}\n", last.unwrap_or(0), env!("CARGO_PKG_VERSION")));
}

/// 查一次。manual = 用户在「关于」里点的：失败要显示出来；自动检查失败只记日志
pub async fn check(app: &AppHandle, manual: bool) {
    if BUSY.swap(true, Relaxed) { return; }
    set(|s| { if manual || s.phase != "available" { s.phase = "checking"; s.error = None; } });
    let r = async { app.updater_builder().timeout(Duration::from_secs(30)).build()?.check().await }.await;
    match r {
        Ok(Some(u)) => {
            nblog!("update check: {} available (current {})", u.version, env!("CARGO_PKG_VERSION"));
            let v = u.version.clone();
            if let Ok(mut g) = PENDING.lock() { *g = Some(u); }
            set(|s| { s.phase = "available"; s.version = Some(v); s.error = None; });
        }
        Ok(None) => { nblog!("update check: up to date"); set(|s| { s.phase = "latest"; s.version = None; s.error = None; }); }
        Err(e) => {
            nblog!("update check failed: {e}");
            set(|s| if manual || s.phase == "checking" {
                if manual { s.phase = "error"; s.error = Some(e.to_string()); } else { s.phase = "idle"; }
            });
        }
    }
    BUSY.store(false, Relaxed);
}

/// 开始检查循环。state 文件记上次检查时间，重启也不会每次都联网
pub fn start(app: AppHandle, state_file: PathBuf) {
    let (_, ran) = read_state(&state_file);
    // 版本变了：刚更新过（或手动装了新版）
    if let Some(prev) = ran.filter(|v| v != env!("CARGO_PKG_VERSION")) {
        nblog!("updated from v{prev} to v{}", env!("CARGO_PKG_VERSION"));
        set(|s| s.updated_from = Some(prev));
    }
    let (last, _) = read_state(&state_file);
    write_state(&state_file, last);
    let _ = std::thread::Builder::new().name("nb-update".into()).spawn(move || {
        std::thread::sleep(Duration::from_secs(20));
        loop {
            let (last, _) = read_state(&state_file);
            if ENABLED.load(Relaxed) && update::due(last, unix_now()) {
                tauri::async_runtime::block_on(check(&app, false));
                write_state(&state_file, Some(unix_now()));
            }
            std::thread::sleep(Duration::from_secs(3 * 3600));
        }
    });
}

/// 下载、校验签名、安装、重启。busy() = 正在息屏或倒计时：这时不更新（下载完也会再看一次）
pub async fn install(app: AppHandle, busy: impl Fn() -> bool) -> Result<(), String> {
    if busy() { return Err("blanked".into()); }
    let u = PENDING.lock().ok().and_then(|g| g.clone()).ok_or("no update")?;
    if BUSY.swap(true, Relaxed) { return Err("busy".into()); }
    nblog!("update: downloading v{}", u.version);
    set(|s| { s.phase = "downloading"; s.progress = Some(0); s.error = None; });
    // 网络抖动时自动重试 2 次（2 秒、6 秒后）；签名不对之类的错误不重试
    let mut attempt = 0;
    let r = loop {
        let (mut got, mut last_pct) = (0usize, 0u8);
        let r = u.download(
            |n, total| {
                got += n;
                if let Some(t) = total.filter(|t| *t > 0) {
                    let p = ((got as u64 * 100) / t).min(100) as u8;
                    if p != last_pct { last_pct = p; set(|s| s.progress = Some(p)); }
                }
            },
            || {},
        ).await;
        match r {
            Err(e @ (tauri_plugin_updater::Error::Reqwest(_) | tauri_plugin_updater::Error::Network(_))) if attempt < 2 => {
                let wait = [2u64, 6][attempt];
                attempt += 1;
                nblog!("update: download failed ({e}), retry {attempt}/2 in {wait}s");
                set(|s| s.progress = Some(0));
                let _ = tauri::async_runtime::spawn_blocking(move || std::thread::sleep(std::time::Duration::from_secs(wait))).await;
                if busy() { break Ok(Vec::new()); }
            }
            r => break r,
        }
    };
    let r = r.and_then(|bytes| {
        if busy() { set(|s| { s.phase = "available"; s.progress = None; }); return Ok(false); }
        set(|s| { s.phase = "installing"; s.progress = Some(100); });
        nblog!("update: installing v{}", u.version);
        u.install(bytes).map(|_| true)
    });
    BUSY.store(false, Relaxed);
    match r {
        Ok(false) => { nblog!("update: blank started during download, not installing now"); Err("blanked".into()) }
        // Windows 上安装程序会自己结束本进程，走不到这里；Mac 换好 .app 后重启
        Ok(true) => { nblog!("update: installed v{}, restarting", u.version); app.restart() }
        Err(e) => {
            nblog!("update failed: {e}");
            set(|s| { s.phase = "error"; s.progress = None; s.error = Some(e.to_string()); });
            Err(e.to_string())
        }
    }
}

/// 用默认浏览器打开（只允许这几个固定地址）
pub fn open(which: &str) {
    let url = match which { "site" => SITE_URL, "github" => REPO_URL, _ => RELEASES_URL };
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("/usr/bin/open").arg(url).spawn();
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(url).spawn();
    #[cfg(not(any(target_os = "macos", windows)))]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}
