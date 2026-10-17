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

## 和 config.rs 的对应
已有字段：`wake.method`（任意键 / 快捷键 / 鼠标移动 / 点击 / 密码 / 只在定时结束时）、`wake.hotkey`、`mouse_threshold_px=150`、`mouse_window_ms=800`、`lock_on_wake=false`、`reblank_after_s=3`、`schedule.*`、`display.mode/targets/method`、`lights.*`、`safety.emergency_hotkey`。

设计稿里出现了几个**新字段**，目前 config.rs 里还没有，需要再定：
- `wake.hold_ms`：快捷键要按住多久才算（默认 1000）
- `general.autostart`、`general.language`、`general.blank_hotkey`：立即息屏的快捷键，稿里写的是 ⌃⌥B / Ctrl+Alt+B
- `schedule.remind_before_min`、`schedule.skip_when_fullscreen_video`
- `power.ac_only`（Mac）：只在接通电源时才防睡眠

注意：Tauri 的 CSP 是 `style-src 'self'`，迁到正式 UI 时要把稿里少量的内联 `style=""` 挪进 CSS。
