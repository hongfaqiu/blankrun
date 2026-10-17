# NightBlank（暂名）

夜里关掉屏幕、键盘背光和 RGB，电脑不睡眠，按配置的方式唤醒。macOS + Windows 同步开发。
设计见 [DESIGN.md](DESIGN.md)，调研见 [RESEARCH.md](RESEARCH.md)。

## 目录结构
```
Cargo.toml              workspace + 体积优先的 release profile
crates/core/            平台无关核心（可在任何主机 cargo test）
  src/state.rs          状态机 Idle → Armed → ScreenOff → Waking，输出 Action
  src/config.rs         config.toml 结构（DESIGN.md §3），全部字段有默认值
  src/schedule.rs       定时计划（跨午夜、距下次开始、开始/结束边沿 + 跳过今晚）
  src/wake.rs           唤醒策略 / 防误触（按住快捷键、鼠标累计位移、紧急键）
  src/hotkey.rs         "Ctrl+Alt+W" 解析与匹配
src-tauri/              Tauri 2 托盘/菜单栏应用
  src/main.rs           托盘与弹出面板、窗口、Tauri 命令（配置、息屏、诊断、权限）
  src/engine.rs         引擎线程：状态机 + 倒计时 + 定时 + 唤醒策略 + 遮罩窗口 + 意外点亮后重新息屏
  src/platform/mod.rs   Platform trait、输入事件通道
  src/platform/macos.rs   IOPMAssertion、IORequestIdle/pmset、CoreBrightness 背光（运行时探测）、CGEventTap（未在真机测试）
  src/platform/windows.rs SetThreadExecutionState、SC_MONITORPOWER、Modern Standby 检测、显示器状态回调、LL 键鼠钩子
  src/platform/stub.rs  其他平台 no-op，仅供编译检查
ui/                     正式界面：原生 HTML/JS/CSS，无框架、无打包器、无内联样式（CSP）
  popover / settings / overlay / onboarding .html + 各自的 .js/.css，共享 nb.css、common.js
design/                 设计稿；design/preview/ 用假的 __TAURI__ 在 Chrome 里渲染 ui/ 检查布局
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
macOS 目标只能在 Mac（或 CI 的 macOS runner）上构建。Linux 上可以做类型检查（不链接）：
```
rustup target add aarch64-apple-darwin
# objc2-exception-helper 的构建脚本要编译 .m 文件，用一个只产生空目标文件的假 cc 绕过（只用于 check）
CC_aarch64_apple_darwin=$PWD/tools/fake-cc.sh AR_aarch64_apple_darwin=ar cargo check -p nightblank --target aarch64-apple-darwin
```
界面预览：`NODE_PATH=<含 playwright-core 的 node_modules> node design/preview/render.mjs`，输出到 `design/preview/out/`。
