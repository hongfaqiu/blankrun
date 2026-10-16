# NightBlank 调研与规划（2026-10-17）

> 只做了调研和规划，还没写代码。所有链接都是这次检索时实际拿到的结果。置信度：高 = 有官方文档或多个独立来源；中 = 只有社区或单一来源；低 = 推测，需要实测。

## 0. 需求验证（先回答"到底有没有人要"）

### 0.1 信号
| 信号 | 数据 | 说明 |
|---|---|---|
| KeepingYouAwake（Mac 防睡眠） | GitHub ⭐ 约 6,987，2026-08 仍在更新 | "防睡眠"这个大类需求明确存在 https://github.com/newmarcel/KeepingYouAwake |
| PowerToys（含 Awake） | ⭐ 约 139k | Awake 只是其中一个小工具，星数不能直接算作需求 https://github.com/microsoft/PowerToys |
| Amphetamine | Mac App Store 4.8–4.9 分，评价数量大 | https://apps.apple.com/us/app/amphetamine/id937984704 |
| Lungo（Sindre Sorhus，付费） | 付费的防睡眠工具也能活下来 | https://sindresorhus.com/lungo |
| BetterDisplay | ⭐ 约 34k，免费版就带 DDC 电源控制 | https://github.com/waydabber/BetterDisplay |
| OpenRGB | ⭐ 约 4.6k | 有人需要统一管理 RGB 灯 https://github.com/CalcProgrammer1/OpenRGB |
| awake（nickita-khylkouski） | ⭐ 6 | 最接近的竞品体量很小 https://github.com/nickita-khylkouski/awake |
| vigil（NewTurn2017） | ⭐ 1 | https://github.com/NewTurn2017/vigil |
| 吾爱熄屏 WuAiScreenOff（Windows） | 中文圈里"息屏不休眠"已经有人专门做了工具 | https://github.com/steen889/WuAiScreenOff ，https://www.i3zh.com/52649.html |
| Never Sleep（"屏幕关掉，电脑不睡"） | 有商业产品直接主打这个卖点 | https://xyz-ai.app/never-sleep/zh/ |
| Windows 上的小型关屏工具 | MonitorOff、monoff、SweetDisplayStandby 等，都只做"关屏" | https://github.com/MiniModeLabs/MonitorOff https://github.com/t-mart/monoff https://github.com/Vavooon/SweetDisplayStandby |
| Reddit 用户提问 | "下载时能不能关显示器"，有人整夜盖 T 恤挡屏幕 | https://www.reddit.com/r/computers/comments/1932ax7/ ，https://www.reddit.com/r/pcmasterrace/comments/xs70tr/ ，https://www.reddit.com/r/buildapc/comments/x63w3u/ |
| Reddit r/MacOS | "不用第三方软件，能不能让 MacBook 不睡眠但屏幕黑掉" | https://www.reddit.com/r/MacOS/comments/1gmljjo/ |
| HN 新动向 | Show HN: Adrafinil，只在 agent 跑任务时让合盖的 Mac 保持唤醒 | https://news.ycombinator.com/item?id=48701512 |
| Stack Overflow | 开发者抱怨 Modern Standby 下关屏会让整机进入睡眠 | https://stackoverflow.com/questions/73766490 |

### 0.2 用户画像
1. 晚上挂下载、渲染、编译或 AI/agent 任务的人（agent 用户在增加，HN 上已有相关讨论）
2. 卧室里放着台式机或 RGB 电竞主机，夜里嫌灯光太亮的人
3. 把 Mac 当家庭服务器用、MacBook 合盖或接外接屏的人
4. 中文圈"挂机"用户（吾爱熄屏能流行起来就是证据）

### 0.3 结论：**中等偏小众，但痛点真实**
- 只说"防睡眠"：需求很强，但市场已经饱和（Amphetamine、KeepingYouAwake、PowerToys 都免费）。
- 只说"关屏不睡"：需求确实存在。不过系统设置和快捷键（Mac 上是 ⌃⇧⏏ 或 `pmset displaysleepnow`）已经能解决大部分情况，用户对付费的意愿不高。
- **防睡眠 + 真关屏 + 关键盘背光和 RGB + 防误触唤醒 + 定时 + 跨平台**，这一整套组合目前没有找到哪个工具做齐了。这就是差异化所在，但受众偏小众（极客、挂机党、RGB 玩家）。
- 建议：按开源免费、靠口碑传播来定位，不要指望靠它赚钱。先发 Mac 或 Windows 其中一个版本试水，看 star 和反馈再决定要不要做全。
- 还没核实：Setapp、Microsoft Store、winget 上的下载量（这次没拿到公开数据）；V2EX、LINUX DO 的具体帖子（只找到一个旁证 https://www.v2ex.com/t/475171 ，是 Insomnia 防休眠工具的帖子）。

## 1. 竞品对比
| 工具 | 平台 | 做什么 | 怎么实现 | 缺口 |
|---|---|---|---|---|
| awake | macOS | 防睡眠（会识别 agent 在跑），菜单栏加命令行，黑屏加关键盘背光 | IOPMAssertion + CoreBrightness | 只有 Mac，唤醒方式固定，没有 RGB，用户很少 |
| vigil | macOS | 工作/离开/睡眠三种模式，命令行调亮度 | 断言 + 亮度 | 只有 Mac，不关灯 |
| PowerToys Awake | Windows | 防睡眠 | SetThreadExecutionState | 不关屏也不关灯 |
| Amphetamine / KeepingYouAwake / Lungo | macOS | 防睡眠，触发条件丰富 | IOPMAssertion | 不主动关屏，不关灯 |
| BetterDisplay | macOS | 显示器全能管理，包括 DDC 电源 | 私有 DDC（支持 M1 的 HDMI 口） | 不是为夜间场景设计的，没有唤醒控制 |
| 吾爱熄屏 / MonitorOff / nircmd | Windows | 一键关屏 | SC_MONITORPOWER | 碰一下就亮，Modern Standby 机器上可能直接睡眠，不关 RGB |
| Never Sleep | 未核实 | 关屏不睡 | 未核实 | 需要进一步调查 |

## 2. macOS 可行性
| 能力 | 方案 | 置信度 | 来源 |
|---|---|---|---|
| 防睡眠但允许屏幕关闭 | `IOPMAssertion PreventUserIdleSystemSleep`，等同 `caffeinate -i`。不要用 `-d`，否则屏幕也会被保持常亮 | 高 | https://www.sudo.is/docs/macos/man_pmset.html ，https://schoenwald.aero/posts/2026-07-25_keeping-your-mac-awake-with-caffeinate/ |
| 真正关掉屏幕（内置屏和外接屏一起） | `pmset displaysleepnow` 或 IOKit 的等价调用。缺点：任何输入都会点亮屏幕，还可能按系统设置触发锁屏 | 高 | https://apple.stackexchange.com/questions/27279 |
| 防误触 | **思路**：屏幕关闭后用 CGEventTap 吞掉输入，不让它走到 HID 层，看能否阻止屏幕被唤醒。**要实测**，HID 层的唤醒很可能发生在事件分发之前。不行的话，就只能接受"屏幕会亮"，亮起后立刻再关掉，同时只在满足唤醒条件时才解除黑屏状态 | 低 | 需要做原型验证 |
| 外接屏 DDC 关电源 | 0xD6 写 4 或 5。m1ddc 支持 USB-C/DP，**不支持 M1 和入门款 M2 的内置 HDMI 口**。各家显示器支持程度差别很大，值 5 写入后有些显示器无法再唤醒 | 中 | https://github.com/waydabber/m1ddc ，https://github.com/waydabber/m1ddc/issues/47 ，https://github.com/waydabber/BetterDisplay/issues/1372 |
| 内置屏亮度归零 + 黑色遮罩（兜底方案） | DisplayServices（私有接口）+ 黑色窗口 + gamma 压到 0 | 中 | DESIGN.md |
| 键盘背光 | CoreBrightness 的 `KeyboardBrightnessClient`。有人在 macOS 26 Tahoe + M1 上测过可用。键盘 ID 是 UInt64，要先调 `copyKeyboardBacklightIDs` 拿到，传 0 会无声失败。要在运行时解析接口，失效时降级 | 中高 | https://github.com/peter6366/KeyboardBacklightToggle ，https://github.com/Maxnflaxl/kblight |
| 权限 | CGEventTap 需要"辅助功能"或"输入监控"权限 | 高 | Apple 文档 |
| 分发 | 用了私有 API 就不能上 Mac App Store，只能走 Developer ID 签名 + 公证。公证不审查私有 API | 高 | 同上 |

## 3. Windows 可行性
| 能力 | 方案 | 置信度 | 来源 |
|---|---|---|---|
| 防睡眠 | `SetThreadExecutionState(ES_CONTINUOUS\|ES_SYSTEM_REQUIRED)`。官方提醒长期持有会很耗电，所以只在息屏期间持有 | 高 | https://learn.microsoft.com/en-us/windows/win32/power/system-sleep-criteria |
| SC_MONITORPOWER 关屏 | 在传统 S3 睡眠的台式机上可用。**在 Modern Standby 机器上，屏幕一关系统就会进入 S0 低功耗待机，后台任务会被暂停**，必须实测这时持有 SYSTEM_REQUIRED 能不能挡住 | 中（失败案例证据充分） | https://stackoverflow.com/questions/73766490 ，https://stackoverflow.com/questions/78877477 ，https://learn.microsoft.com/en-us/windows-hardware/design/device-experiences/display--sleep--and-hibernate-idle-timers |
| DDC/CI 关电源 | dxva2 的 `SetVCPFeature(0xD6, 4 或 5)`。台式机外接屏可用，笔记本内置屏不行 | 中 | 同上 SO 73766490 |
| 遮罩兜底 | 每块屏一个顶层黑色窗口 + gamma | 高 | — |
| 输入 | WH_KEYBOARD_LL / WH_MOUSE_LL 低级钩子，可能被杀毒软件当成键盘记录器 | 高 | — |
| RGB | OpenRGB SDK（TCP 6742），需要用户单独安装并开启 SDK Server；另外可接 Windows 11 Dynamic Lighting | 中 | https://github.com/CalcProgrammer1/OpenRGB |
| 签名 | Azure Artifact Signing（原 Trusted Signing），9.99 美元/月。**个人开发者只限美国和加拿大**。签了名 SmartScreen 也要慢慢积累信誉 | 高 | https://learn.microsoft.com/en-us/azure/artifact-signing/faq ，https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation |

## 4. 外接屏和笔记本的区别
- 笔记本内置屏：DDC 用不了。只能靠系统关屏（pmset / SC_MONITORPOWER）或者亮度 0 + 遮罩。键盘背光可以控制。合盖的话需要另外处理（`pmset disablesleep`，要 root）。
- 台式机外接屏：DDC 能真正断电，但不同显示器差异很大，要维护兼容列表，默认写 4，不用 5。系统关屏后显示器会进入待机，功耗通常低于 0.5 W。
- 遮罩方案在 LCD 上背光照样亮着，不省电。在 OLED 上基本不耗电，但有烧屏风险（全黑画面风险较小）。

## 5. 风险（前几位）
1. **防误触和真关屏可能有冲突**：系统关屏后，输入在 HID 层就可能把屏幕点亮。这是整个产品体验的核心风险，必须在 M0 先验证。
2. Windows Modern Standby 机器上关屏会导致系统睡眠，后台任务停止。
3. 私有 API（CoreBrightness、DisplayServices）在系统升级后可能失效。
4. 崩溃后屏幕一直黑着（gamma 或遮罩没还原）。需要 watchdog、紧急快捷键、启动时自动还原。
5. 签名：Windows 个人签名受地区限制，SmartScreen 会弹警告；全局钩子可能被杀毒软件误报。
6. 需求偏小众（见第 0 节）。

## 6. MVP 范围（建议 Mac 先行）
- 菜单栏常驻 + 全局快捷键 + 定时计划
- 防睡眠断言
- 关屏：默认用系统关屏（真断电）。屏幕被意外点亮后，在 N 秒内自动再关掉，直到满足唤醒条件
- 唤醒方式：快捷键、长按、鼠标累计移动阈值、定时结束
- 关闭内置键盘背光，结束后恢复原值
- 紧急快捷键 + 崩溃后自动还原
- 暂不做：DDC、RGB、按显示器分别设置、密码唤醒（这几项放到 M2 及以后）

## 7. 里程碑
| 阶段 | 内容 | 时间 |
|---|---|---|
| M0 | 技术验证：Mac 上验证"关屏 + EventTap 能否防唤醒"；Windows 上验证 Modern Standby + SYSTEM_REQUIRED + SC_MONITORPOWER | 1 周 |
| M1 | Mac MVP（第 6 节），签名公证后发 GitHub Release 和 Homebrew cask | 2 周 |
| M2 | Windows MVP：托盘、关屏或遮罩、低级钩子、OpenRGB；发 winget 和 Scoop | 2 周 |
| M3 | DDC（带兼容列表）、按显示器设置、唤醒后锁屏、设置界面 | 2 周 |
| M4 | 厂商背光插件、自动更新、多语言、宣传（V2EX、LINUX DO、r/macapps、Show HN） | 持续 |

## 8. 发布和分发
- Mac：Apple Developer Program（99 美元/年）→ Developer ID 签名 → notarytool 公证 → DMG + Homebrew cask + Sparkle 自动更新。私有 API 决定了不能上 Mac App Store。
- Windows：MSI/MSIX + winget + Scoop。签名可选：Azure Artifact Signing（受地区限制）、OV 证书（需要硬件密钥，每年几百美元）、或者先不签名，靠开源和 winget 建立信任。Microsoft Store 不用私有接口，可以考虑上架，商店代签还能绕开 SmartScreen 警告。
- 开源许可：MIT 或 Apache-2.0。开源能降低杀毒软件误报和"键盘记录器"疑虑。

## 9. 测试计划（Mac Mini 没接屏幕）
- 逻辑层：Rust 单元测试，覆盖状态机、计划、防抖阈值
- 防睡眠：Mac Mini 上用 `pmset -g assertions` 和 `pmset -g log` 验证，可以无头测试
- 关屏：需要真屏幕。可选方案：
  1. 借或用一台 MacBook（覆盖内置屏 + 键盘背光）
  2. 给 Mac Mini 接一台便宜的 DDC 显示器（USB-C/DP 口，避开 M1 的 HDMI）
  3. HDMI 假负载头只能让系统以为有显示器，**测不了真正的断电和背光**
  4. 用 GitHub Actions 的 macOS 和 Windows runner 做构建和冒烟测试
- Windows：分别准备一台 Modern Standby 笔记本和一台 S3 台式机测试，用 `powercfg /a`、`/requests`、`/sleepstudy` 确认后台任务没有被暂停
- 功耗：用墙插功率计对比关屏前后的功耗

## 10. 需要你确认的问题
1. 先做 Mac 还是 Windows？（建议 Mac：你有 Mac 设备，私有 API 路径也已经验证过）
2. 能不能弄到带屏幕的测试机（MacBook 或外接 DDC 显示器）？
3. 防误触做不到完美时能接受吗？比如"屏幕会短暂亮一下然后自动再黑"
4. 开源免费，还是要收费？要不要付费签 Windows 证书（你所在地区可能用不了 Azure 个人签名）？
5. 是否需要支持 MacBook 合盖场景（要 root）？
