# 设计稿（v0.1）

先读 `DESIGN-CARD.md`（设计卡）。所有页面都是纯 HTML/CSS，不用框架和网页字体，共享 `nb.css`，以后可以直接迁到 `ui/`。

| 文件 | 内容 | 参数 |
|---|---|---|
| `popover.html` | 菜单栏 / 托盘面板 | `?p=mac\|win`，`&state=countdown` |
| `settings.html` | 设置窗口，5 个分区 | `?p=mac\|win#general\|blank\|wake\|schedule\|about` |
| `overlay.html` | 息屏遮罩上的唤醒提示 | `?p=mac\|win&state=hint\|hold` |
| `onboarding.html` | Mac 首次启动的权限引导 | — |
| `overview.html` | 总览拼图 | — |

重新出图（2x DPR，只截 `.stage`）：
```bash
npm i --prefix /tmp/pw playwright-core   # 只装一次；用系统里的 /usr/bin/google-chrome
NODE_PATH=/tmp/pw/node_modules node design/render.mjs [文件名过滤]
```

## 深色模式
`nb.css` 用 `@media (prefers-color-scheme: dark)` 整体换一套中性灰阶变量，页面里不再写死颜色，跟随系统自动切换。`render.mjs` 会额外用深色输出 `popover-mac/win`、`settings-mac-wake/blank`、`onboarding-mac` 的 `*-dark.png`，以及 `overview-dark.png`。息屏遮罩本身就是纯黑，不分深浅。

## 和 config.rs 的对应
稿里的设置项都已经进了 `crates/core/src/config.rs`（与 DESIGN.md 第 3 节一致）：
- `general.launch_at_login=false`、`general.language=auto|zh|en`、`general.blank_now_hotkey="Ctrl+Alt+B"`（⌃⌥B）
- `wake.*`：`method`、`hotkey`、`hold_ms=1000`（按住 1.0 秒）、`mouse_threshold_px=150`、`mouse_window_ms=800`、`lock_on_wake=false`、`reblank_after_s=3`
- `schedule.*`：`days` 默认每天、`remind_before_min=5`、`skip_when_fullscreen_video=true`
- `power.ac_only=false`（仅 Mac：只在接通电源时防睡眠）
- `display.*`、`lights.*`、`safety.emergency_hotkey`

设计稿里「登录时启动」画的是打开后的样子，实际默认关闭。

注意：Tauri 的 CSP 是 `style-src 'self'`，迁到正式 UI 时要把稿里少量的内联 `style=""` 挪进 CSS。
