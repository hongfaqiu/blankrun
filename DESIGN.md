# 夜间息屏工具（暂名 NightBlank）设计文档

## 1. 定位
目标：夜里把屏幕、键盘背光、鼠标/RGB 灯全关掉，但电脑不睡眠，下载、服务、agent 照常跑；按用户配置的方式唤醒。

| 现有工具 | 能做 | 缺口 |
|---|---|---|
| awake（macOS） | 防睡眠，Option+1 黑屏加关键盘背光 | 只有 Mac，唤醒方式固定，没有定时，不管 RGB |
| vigil（macOS） | 防睡眠 | 只有 Mac，不关灯 |
| PowerToys Awake / Caffeine | 防睡眠 | 不黑屏，不关灯 |
| 空白屏保 / Multiscreen Blank | 黑屏 | 碰一下就醒，不关灯，没有定时 |

差异化：
- 一套工具覆盖 Mac 和 Windows，配置格式相同
- 防睡眠、黑屏、关灯一键完成
- 唤醒方式可配置，并且能防误触（猫踩、桌子震动导致鼠标抖动）
- 按计划自动进入和退出
- 可按显示器分别设置

## 2. MVP 功能
1. 托盘/菜单栏常驻，提供快捷键和"立即息屏"菜单
2. 息屏期间防止系统睡眠，网络和磁盘照常运行
3. 黑屏：所有显示器或指定显示器
4. 关键盘背光（Mac 内置键盘；Windows 支持 OpenRGB 能识别的设备）
5. 唤醒方式可配置（见第 3 节），唤醒后恢复亮度和灯光
6. 定时计划，例如 23:30 进入、07:30 退出
7. 崩溃或异常退出时自动恢复：亮度、gamma、背光都还原

## 3. 配置项（config.toml）
与 `crates/core/src/config.rs` 保持一致；以下都是默认值，文件里没写的字段取默认。
```toml
[general]
launch_at_login = false    # 登录 / 开机时启动，默认关，由用户自己打开
language = "auto"          # auto | zh | en（auto = 跟随系统）
blank_now_hotkey = "Ctrl+Alt+B"   # 立即息屏；Mac 上显示为 ⌃⌥B
[wake]
method = "hotkey"          # any_key | hotkey | mouse_move | click | password | schedule_only
hotkey = "Ctrl+Alt+W"
hold_ms = 1000             # 唤醒快捷键要按住 1.0 秒，短按不唤醒
mouse_threshold_px = 150   # 累计移动超过这个距离才算唤醒
mouse_window_ms = 800      # 并且要在这个时间窗口内完成，用来过滤抖动
lock_on_wake = false       # 唤醒后锁屏（用系统密码解锁）；公开版做成设置项，默认关
reblank_after_s = 3        # 屏幕被意外点亮后几秒内自动再关
[schedule]
enabled = false
start = "23:30"
end = "07:30"
days = ["mon","tue","wed","thu","fri","sat","sun"]   # 默认每天
remind_before_min = 5      # 开始前几分钟弹通知（可推迟 30 分钟 / 跳过今晚），0 = 不提醒
skip_when_fullscreen_video = true   # 有程序在全屏播放视频时跳过本次
[display]
mode = "all"               # all | list
targets = []               # 显示器 ID / 名称
method = "auto"            # auto | power_off | overlay
[lights]
keyboard_backlight = true
openrgb = false
openrgb_host = "127.0.0.1:6742"
[safety]
emergency_hotkey = "Ctrl+Alt+Shift+Esc"   # 任何唤醒方式下都有效
[power]
ac_only = false            # 仅 macOS：只在接通电源时防睡眠，用电池时照常睡眠
```

## 4. 技术栈
推荐 **Tauri 2 + Rust 核心，平台相关代码按需写原生**：
- Rust 内核（状态机、计划、配置）两个平台共用一份
- 平台层通过 `objc2` / `core-graphics` / `IOKit` 和 `windows` crate 直接调系统 API
- 设置界面用 Tauri 的 WebView，安装包小（约 10 MB）
- OpenRGB 有 Rust 客户端（`openrgb` crate）

备选方案是 Swift + C# 两套原生代码：体验最好，但要维护两份逻辑。如果只做 Mac，可以直接用 Swift。

## 5. 各平台实现

### macOS
- **防睡眠**：`IOPMAssertionCreateWithName(kIOPMAssertPreventUserIdleSystemSleep)`。只阻止系统睡眠，允许显示器关闭。
- **黑屏**，三种方式：
  1. `pmset displaysleepnow`：真正关掉背光，最省电。但任何输入都会点亮屏幕，而且可能触发锁屏，不好控制。
  2. 每块屏幕放一个全屏的纯黑无边框窗口（层级设为 screenSaver 以上），同时用 `CGSetDisplayTransferByTable` 把 gamma 压到 0。这样能完全控制唤醒，但背光还亮着。可以再配合 DisplayServices 私有接口 `DisplayServicesSetBrightness` 把内置屏亮度调到 0。
  3. 默认用 auto：内置屏用"亮度 0 + 黑色遮罩"，外接屏用 gamma 0 + 遮罩（DDC/CI 关电源列为后续功能）。
- **键盘背光**：私有框架 CoreBrightness 的 `KeyboardBrightnessClient`（`setBrightness:forKeyboard:`、`enableAutoBrightness:`）。awake 已经证明这条路可行。私有 API 不能上架 Mac App Store，只能直接分发。
- **唤醒输入**：`CGEventTapCreate` 监听键盘和鼠标事件，在息屏期间吞掉这些事件，防止误操作。需要"辅助功能"权限，部分情况还要"输入监控"权限。
- **锁屏**：唤醒后调用 `SACLockScreenImmediate`（私有接口），或者走系统锁屏快捷键。

### Windows
- **防睡眠**：`SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED)`，不加 `ES_DISPLAY_REQUIRED`。也可以用 `PowerCreateRequest(PowerRequestSystemRequired)`，在 `powercfg /requests` 里能看到。
- **黑屏**：
  1. `SendMessage(HWND_BROADCAST, WM_SYSCOMMAND, SC_MONITORPOWER, 2)`：真正关屏，但会关掉所有显示器，任意输入都会唤醒，在部分显卡和现代待机机器上表现不一致。
  2. 每块显示器一个顶层黑色窗口，加 `SetDeviceGammaRamp` 压暗。外接屏可以用 DDC/CI（`SetVCPFeature` 0xD6 电源模式）真正关闭。
  3. 默认 auto：先用遮罩，DDC/CI 可用时再关电源。
- **键盘和 RGB 灯**：Windows 没有统一接口。
  - OpenRGB SDK（TCP 6742）覆盖大多数外设，设成全黑即可
  - 笔记本键盘背光要走厂商接口（如联想 Vantage、华硕 Armoury、Dell WMI），列为后续插件
  - Windows 11 的 Dynamic Lighting（`Windows.Devices.Lights.LampArray`）可以作为补充
- **唤醒输入**：`SetWindowsHookEx(WH_KEYBOARD_LL / WH_MOUSE_LL)` 低级钩子，不需要管理员权限。普通权限的钩子拦不到管理员进程的窗口，所以遮罩窗口要自己保持焦点。
- **锁屏**：`LockWorkStation()`。

## 6. 权限
- **macOS**：辅助功能、输入监控（首次启动时引导授权）。私有框架要求不上架 App Store，签名和公证用 Developer ID。
- **Windows**：普通用户权限即可。OpenRGB 需要单独安装并开启 SDK Server。

## 7. 风险
- 私有 API（CoreBrightness、DisplayServices）可能在 macOS 升级后失效，需要做探测和降级（失效时只黑屏）
- 用了遮罩或 gamma 后如果程序崩溃，屏幕会一直黑着。对策：watchdog 进程、紧急快捷键、启动时自动还原 gamma 和亮度
- OLED 屏幕不关电源时，长时间黑色遮罩基本不耗电，但 LCD 背光仍然亮着
- 外设灯在 BIOS 或固件层面控制时，软件关不掉
- 杀毒软件可能把全局钩子当成键盘记录器报警。对策：代码签名、开源

## 8. 里程碑
| 阶段 | 内容 | 时间 |
|---|---|---|
| M0 | 技术验证：两个平台各写命令行原型（防睡眠 + 黑屏 + 热键唤醒） | 1 周 |
| M1 | Mac MVP：菜单栏、背光、可配置唤醒、防抖、定时 | 2 周 |
| M2 | Windows MVP：托盘、遮罩 + DDC/CI、OpenRGB | 2 周 |
| M3 | 设置界面、按显示器配置、唤醒后锁屏、崩溃恢复 | 1–2 周 |
| M4 | 厂商背光插件、签名和公证、自动更新、开源发布 | 持续 |

## 9. 待确认问题
1. 只给自己用，还是要公开发布？这决定了能不能用私有 API、要不要上架商店。
2. 主要在哪几台机器上用？需要知道 Mac 型号和 Windows 本的品牌，好确定键盘背光怎么关。
3. 唤醒后要不要强制锁屏？还是只在密码唤醒模式下才锁？
