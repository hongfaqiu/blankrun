//! 平台抽象。每个平台实现 `Platform`，核心状态机输出的 Action 由此执行。
//! 所有实现目前都是 stub，TODO 指向 RESEARCH.md 对应章节。

pub type Result<T = ()> = std::result::Result<T, String>;

pub trait Platform {
    /// 防系统睡眠但允许屏幕关闭
    fn keep_awake(&mut self, on: bool) -> Result;
    /// 关屏 / 恢复
    fn display_off(&mut self, off: bool) -> Result;
    /// 关键盘背光（off=false 时恢复原值）；不支持时返回 Ok 并记录降级
    fn keyboard_backlight(&mut self, off: bool) -> Result;
    /// 安装/卸载全局输入钩子（吞事件 + 喂给 WakePolicy）
    fn input_hook(&mut self, on: bool) -> Result;
    /// 唤醒后锁屏（可选设置项）
    fn lock_screen(&mut self) -> Result;
    /// 启动时调用：崩溃后还原 gamma/亮度/背光（RESEARCH.md §5 风险 4）
    fn restore_on_startup(&mut self) -> Result { Ok(()) }
}

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::Native;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use self::windows::Native;

#[cfg(not(any(target_os = "macos", windows)))]
mod stub;
#[cfg(not(any(target_os = "macos", windows)))]
pub use stub::Native;
