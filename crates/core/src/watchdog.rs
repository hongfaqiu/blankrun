//! 崩溃看门狗的命令行约定（纯逻辑，便于测试）。
//!
//! 主进程启动后拉起一个自己的副本当看门狗：`--watchdog=<主进程 pid>` + `--watchdog-dir=<配置目录>`。
//! 看门狗只等主进程退出；退出时如果 `blanking.state` 还在（正常退出会先删掉它），
//! 说明主进程是在息屏状态下被强退 / 崩溃 / 结束任务的，看门狗立刻替它把屏幕和背光恢复。
//!
//! Windows 上分两段：主进程先拉起 `--watchdog-launch=<pid>`，它再拉起真正的 `--watchdog=<pid>` 后马上退出，
//! 这样看门狗不再是主进程的子进程，任务管理器对主进程“结束任务”时不会把它一起结束。

/// 看门狗相关的启动阶段
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stage {
    /// 中转：再拉起一个 `Watch` 后立刻退出（Windows 用来脱离进程树）
    Launch,
    /// 真正的看门狗：等主进程退出
    Watch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub stage: Stage,
    /// 被看护的主进程 pid
    pub parent: u32,
    /// 配置目录（`blanking.state`、`blankrun.log` 所在处）
    pub dir: String,
}

pub const LAUNCH: &str = "--watchdog-launch=";
pub const WATCH: &str = "--watchdog=";
pub const DIR: &str = "--watchdog-dir=";
/// 崩溃标记文件名（和 engine 写的一致）
pub const MARKER: &str = "blanking.state";

/// 从命令行参数里认出看门狗模式。不是看门狗（普通启动）返回 None；
/// 参数不全或 pid 不合法也返回 None——宁可不看护，也不要把一个畸形参数当成主进程跑起来以外的东西。
pub fn parse<I, S>(args: I) -> Option<Args>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let (mut stage, mut parent, mut dir) = (None, None, None);
    for a in args {
        let a = a.as_ref();
        if let Some(v) = a.strip_prefix(LAUNCH) {
            stage = Some(Stage::Launch);
            parent = v.trim().parse::<u32>().ok();
        } else if let Some(v) = a.strip_prefix(WATCH) {
            stage = Some(Stage::Watch);
            parent = v.trim().parse::<u32>().ok();
        } else if let Some(v) = a.strip_prefix(DIR) {
            dir = Some(v.to_string());
        }
    }
    let parent = parent.filter(|p| *p > 1)?;
    let dir = dir.filter(|d| !d.is_empty())?;
    Some(Args { stage: stage?, parent, dir })
}

/// 拉起下一段要用的参数
pub fn child_args(stage: Stage, parent: u32, dir: &str) -> Vec<String> {
    let flag = match stage { Stage::Launch => LAUNCH, Stage::Watch => WATCH };
    vec![format!("{flag}{parent}"), format!("{DIR}{dir}")]
}

/// 主进程没了以后要不要恢复：只看崩溃标记还在不在、有没有内容
pub fn needs_restore(marker: Option<&str>) -> bool { marker.is_some() }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_launch_is_not_watchdog() {
        assert_eq!(parse(["blankrun"]), None);
        assert_eq!(parse(["blankrun", "--autostart", "--blank-now=3"]), None);
    }

    #[test]
    fn parses_watch_and_launch() {
        let a = parse(["x", "--watchdog=1234", "--watchdog-dir=C:\\Users\\a b\\AppData\\Roaming\\com.cosfoc.blankrun"]).unwrap();
        assert_eq!(a, Args { stage: Stage::Watch, parent: 1234, dir: "C:\\Users\\a b\\AppData\\Roaming\\com.cosfoc.blankrun".into() });
        let a = parse(["x", "--watchdog-dir=/tmp/d", "--watchdog-launch=99"]).unwrap();
        assert_eq!(a.stage, Stage::Launch);
        assert_eq!(a.parent, 99);
    }

    #[test]
    fn rejects_incomplete_or_bad() {
        assert_eq!(parse(["x", "--watchdog=12"]), None); // 没有目录
        assert_eq!(parse(["x", "--watchdog-dir=/tmp"]), None); // 没有 pid
        assert_eq!(parse(["x", "--watchdog=abc", "--watchdog-dir=/tmp"]), None);
        assert_eq!(parse(["x", "--watchdog=0", "--watchdog-dir=/tmp"]), None);
        assert_eq!(parse(["x", "--watchdog=1", "--watchdog-dir=/tmp"]), None); // launchd / init 不看护
        assert_eq!(parse(["x", "--watchdog=5", "--watchdog-dir="]), None);
    }

    #[test]
    fn child_args_roundtrip() {
        for st in [Stage::Launch, Stage::Watch] {
            let mut v = vec!["exe".to_string()];
            v.extend(child_args(st.clone(), 4321, "/Users/me/Library/Application Support/com.cosfoc.blankrun"));
            let a = parse(&v).unwrap();
            assert_eq!(a.stage, st);
            assert_eq!(a.parent, 4321);
            assert_eq!(a.dir, "/Users/me/Library/Application Support/com.cosfoc.blankrun");
        }
    }

    #[test]
    fn restore_only_when_marker_left() {
        assert!(!needs_restore(None));
        assert!(needs_restore(Some("blanked\n")));
        assert!(needs_restore(Some("")));
    }
}
