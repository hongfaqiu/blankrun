# NightBlank（暂名）

夜里关掉屏幕、键盘背光和 RGB，电脑不睡眠，按配置的方式唤醒。macOS + Windows 同步开发。
设计见 [DESIGN.md](DESIGN.md)，调研见 [RESEARCH.md](RESEARCH.md)。

## 目录结构
```
Cargo.toml              workspace + 体积优先的 release profile
crates/core/            平台无关核心（可在任何主机 cargo test）
  src/state.rs          状态机 Idle → Armed → ScreenOff → Waking，输出 Action
  src/config.rs         config.toml 结构（DESIGN.md §3），全部字段有默认值
  src/schedule.rs       定时计划（支持跨午夜，不依赖 chrono）
  src/wake.rs           唤醒策略 / 防误触（鼠标累计位移 + 时间窗、紧急键）
src-tauri/              Tauri 2 托盘/菜单栏应用
  src/main.rs           托盘菜单、Action 分发、配置读写命令
  src/platform/mod.rs   Platform trait：keep_awake / display_off / keyboard_backlight / input_hook / lock_screen
  src/platform/macos.rs   stub + TODO（IOPMAssertion、pmset、CoreBrightness、CGEventTap）
  src/platform/windows.rs stub + TODO（SetThreadExecutionState 已接、SC_MONITORPOWER、LL hooks、OpenRGB）
  src/platform/stub.rs  其他平台 no-op，仅供编译检查
ui/                     原生 HTML/JS/CSS，无框架、无打包器
```
配置文件位置：系统应用配置目录下的 `com.hongfaqiu.nightblank/config.toml`。

## 体积目标
| 产物 | 目标 |
|---|---|
| Windows 安装包（NSIS, lzma，WebView2 用系统自带） | ≤ 3 MB |
| macOS DMG | ≤ 4 MB |
| 主程序（单架构） | ≤ 5 MB |

手段：`opt-level="z"`、`lto`、`codegen-units=1`、`panic="abort"`、`strip`；Tauri 关闭默认特性只留 `wry`/`tray-icon`/`compression`；
Windows 用 `windows-sys` 而非 `windows`；核心不用 chrono/regex/tokio 等重依赖；前端零依赖。新增 crate 前请先评估体积。

## 开发
```
cargo test -p nightblank-core                              # 核心单测，任意平台
cargo check -p nightblank --target x86_64-pc-windows-gnu   # 在 Linux 上检查 Windows 代码
cargo install tauri-cli --version "^2" && cargo tauri build  # 在 Mac / Windows 本机打包
```
macOS 目标只能在 Mac（或 CI 的 macOS runner）上检查和构建。
