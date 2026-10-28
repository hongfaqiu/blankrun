//! 崩溃看门狗：主进程在息屏中被强退 / 结束任务 / abort 时，替它把屏幕和背光恢复。
//!
//! panic 钩子和退出事件只能管到“进程自己还能跑代码”的情况；任务管理器“结束任务”、`kill -9`、
//! 强制退出、release 版 `panic = "abort"` 之后的路径都不会经过它们，用户只能对着黑屏
//! 等下次启动 BlankRun 才恢复（Mac 上断开的内建屏甚至要重启）。
//! 看门狗是同一个可执行文件的另一个进程，什么界面都不开，只等主进程退出，
//! 然后看 `blanking.state` 还在不在（正常退出会先删掉）——在就照启动时的恢复流程做一遍。
use blankrun_core::watchdog::{self, Args, Stage};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// main() 最开头调用：是看门狗模式就跑完直接退出进程，不会返回
pub fn run_if_requested() {
    let Some(a) = watchdog::parse(std::env::args().skip(1)) else { return };
    match a.stage {
        Stage::Launch => { let _ = spawn_stage(Stage::Watch, a.parent, Path::new(&a.dir)); }
        Stage::Watch => watch(a),
    }
    std::process::exit(0);
}

/// 主进程在拿到单实例锁、做完启动恢复之后调用
pub fn start(dir: &Path) {
    // Windows：先拉一个中转进程，由它再拉真正的看门狗，看门狗就不在主进程的进程树 / 任务管理器分组里
    let first = if cfg!(windows) { Stage::Launch } else { Stage::Watch };
    match spawn_stage(first, std::process::id(), dir) {
        Ok(pid) => crate::nblog!("watchdog started (pid {pid})"),
        Err(e) => crate::nblog!("watchdog not started: {e}"),
    }
}

fn spawn_stage(stage: Stage, parent: u32, dir: &Path) -> std::result::Result<u32, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut c = Command::new(exe);
    c.args(watchdog::child_args(stage, parent, &dir.to_string_lossy()))
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
        // 主进程如果在一个“关闭即结束”的作业对象里（某些启动器会这样），尽量脱离；不允许脱离就退而求其次
        c.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_BREAKAWAY_FROM_JOB);
        match c.spawn() {
            Ok(ch) => return Ok(ch.id()),
            Err(_) => { c.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP); }
        }
    }
    c.spawn().map(|ch| ch.id()).map_err(|e| e.to_string())
}

fn watch(a: Args) {
    let dir = PathBuf::from(&a.dir);
    if !wait_for_exit(a.parent) { return; }
    let marker = dir.join(watchdog::MARKER);
    let saved = std::fs::read_to_string(&marker).ok();
    if !watchdog::needs_restore(saved.as_deref()) { return; } // 正常退出：什么都不做，也不写日志
    crate::log::init(dir.join("blankrun.log"));
    crate::nblog!("watchdog: BlankRun (pid {}) exited while the screen was off; restoring ({saved:?})", a.parent);
    use crate::platform::Platform;
    let mut plat = crate::platform::Native::default();
    match plat.restore_on_startup(saved.as_deref()) {
        Ok(()) => crate::nblog!("watchdog: restored"),
        Err(e) => crate::nblog!("watchdog: restore failed: {e}"),
    }
    // 只删我们读到的那份：主进程这时已经被用户重新打开、又写了新标记的话不要动它
    if std::fs::read_to_string(&marker).ok() == saved { let _ = std::fs::remove_file(&marker); }
}

/// 阻塞到主进程退出。拿不到主进程（已经不在 / 无权限）返回 false，看门狗直接退出
#[cfg(windows)]
fn wait_for_exit(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject, INFINITE, PROCESS_SYNCHRONIZE};
    unsafe {
        let h = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if h.is_null() { return false; }
        let r = WaitForSingleObject(h, INFINITE);
        CloseHandle(h);
        r == WAIT_OBJECT_0
    }
}

/// Mac：看门狗是主进程的直接子进程；主进程一退出它就被 launchd 收养，父进程号随之改变
#[cfg(not(windows))]
fn wait_for_exit(pid: u32) -> bool {
    if std::os::unix::process::parent_id() != pid { return false; }
    while std::os::unix::process::parent_id() == pid {
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    true
}
