//! 极简日志：写到配置目录的 blankrun.log（超过 256 KB 时清空重写），同时打到 stderr。
//! Windows 发布版没有控制台，排查问题只能靠这个文件；设置 › 诊断导出时也会附上。
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

static FILE: Mutex<Option<PathBuf>> = Mutex::new(None);

pub fn init(path: PathBuf) {
    if let Some(d) = path.parent() { let _ = std::fs::create_dir_all(d); }
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > 256 * 1024) { let _ = std::fs::remove_file(&path); }
    *FILE.lock().unwrap_or_else(|p| p.into_inner()) = Some(path);
}

pub fn path() -> Option<PathBuf> { FILE.lock().ok()?.clone() }

pub fn write(msg: std::fmt::Arguments) {
    let (_, min) = crate::platform::local_time();
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() % 60_000).unwrap_or(0);
    let line = format!("{:02}:{:02}:{:02}.{:03} {msg}\n", min / 60, min % 60, ms / 1000, ms % 1000);
    eprint!("[blankrun] {line}");
    if let Some(p) = path() {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) { let _ = f.write_all(line.as_bytes()); }
    }
}

#[macro_export]
macro_rules! nblog { ($($t:tt)*) => { $crate::log::write(format_args!($($t)*)) } }
