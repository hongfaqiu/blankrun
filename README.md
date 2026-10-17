<p align="center"><img src="src-tauri/icons/128x128.png" width="72" alt="BlankRun icon"></p>

<h1 align="center">BlankRun</h1>

<p align="center">Turn the screens off at night. Keep your AI agents running.</p>

<p align="center"><b>English</b> · <a href="README.zh-CN.md">简体中文</a></p>

<p align="center"><a href="https://blankrun.cosfoc.com">Website</a> · <a href="https://github.com/hongfaqiu/blankrun/releases">Download</a> · macOS + Windows · MIT</p>

> **Status: v0.1.2. Blanking, keep-awake, wake and restore have been checked on a MacBook Air M4 (macOS 26.5) and a dual-monitor Windows 11 desktop. The installers are unsigned.** [Download the latest version](https://github.com/hongfaqiu/blankrun/releases/latest)

<p align="center"><img src="docs/screenshots/popover-mac-en.png" width="380" alt="Menu bar panel"> <img src="docs/screenshots/popover-mac-en-dark.png" width="380" alt="Menu bar panel (dark)"></p>

## What it is

You queue up a night of work for Claude Code, Codex or a local model, plus a build and some downloads. A screen glowing all night is too bright, but letting the computer sleep stops the jobs.

BlankRun lives in the menu bar (the notification area on Windows). One click really turns the displays off and blocks system sleep, so your jobs keep running. While the screen is off, it ignores accidental input and only wakes the way you chose.

Features:

- **Blank now or after a delay**: right away, or after 1, 5 or 15 minutes.
- **No sleep while blank** (on by default, can be turned off): IOPMAssertion on macOS, SetThreadExecutionState on Windows.
- **Schedule**: for example 23:30 to 07:30 every day. Works across midnight, and you can skip just tonight.
- **Configurable wake**: any key, holding a shortcut (⌃⌥W for 1 second by default), moving the mouse, clicking, locking on wake so you enter your password, or only when the schedule ends. A separate emergency shortcut always restores everything.
- **No accidental wake**: keyboard and mouse input is blocked while blank. If the screen lights up anyway, it goes dark again within seconds. On macOS without Accessibility permission, the wake shortcut still works and stray touches are re-blanked.
- **Launch at login, blank-now shortcut** (⌃⌥B by default), **a reminder before the schedule starts** (skip tonight or postpone 30 min), and **waiting while a video plays full screen**.
- **Keyboard backlight**: turned off together with the screen on macOS. It uses a private API that is probed at runtime and skipped when unavailable.
- **Windows Modern Standby**: on PCs that drop into low-power standby when the display turns off, BlankRun uses a black overlay instead so jobs aren't suspended.
- **English and Chinese UI**: follows the system language by default. Switch in Settings › General › Language and the tray menu, wake hints and onboarding update immediately, with no restart.
- **Small**: built on Tauri 2 with a zero-dependency UI. Measured for v0.1.0: 1.2 MB Windows installer, 3.2 MB universal macOS DMG.
- **Auto-update**: checks once a day (can be turned off). Updates are signed and install only when you click "Update and restart", never while the screen is blank. From v0.1.1 on; v0.1.0 needs one manual install.
- **Blank history**: the last 30 sessions with time, trigger, how they ended and how many accidental touches were blocked. Stored locally and can be cleared.
- **Local only**: apart from the daily update check, nothing goes over the network and nothing is uploaded.

Later versions: external monitor power via DDC/CI, RGB lighting.

## Screenshots

| Settings · Wake | Screen off | First-run setup |
|---|---|---|
| <img src="docs/screenshots/settings-mac-wake-en.png" width="300"> | <img src="docs/screenshots/overlay-mac-hint-en.png" width="300"> | <img src="docs/screenshots/onboarding-mac-en.png" width="300"> |

| Windows tray panel | Settings · Schedule (Windows) |
|---|---|
| <img src="docs/screenshots/popover-win-en.png" width="260"> | <img src="docs/screenshots/settings-win-schedule-en.png" width="360"> |

Screenshots show the v0.1.0 interface. On your machine, fonts follow the system.

## Docs

- [DESIGN.md](DESIGN.md): design document (Chinese)
- [RESEARCH.md](RESEARCH.md): research and plan (Chinese)
- [design/](design/): UI mockups and design card
- [site/](site/): website source (static HTML/CSS)

## Layout
```
Cargo.toml              workspace + size-first release profile
crates/core/            platform-independent core (cargo test on any host)
  src/state.rs          state machine Idle → Armed → ScreenOff → Waking, emits Actions
  src/config.rs         config.toml schema (DESIGN.md §3); every field has a default
  src/schedule.rs       schedule (across midnight, time until next start, start/end edges, skip tonight)
  src/wake.rs           wake policy and accidental-wake guard (hold shortcut, mouse distance, emergency key)
  src/hotkey.rs         "Ctrl+Alt+W" parsing and matching
  src/locale.rs         UI language choice (match system or explicit; same rules as ui/common.js)
src-tauri/              Tauri 2 tray / menu bar app
  src/main.rs           tray and panel, windows, Tauri commands (config, blank, diagnostics, permissions, locale)
  src/engine.rs         engine thread: state machine, countdown, schedule, wake policy, overlays, re-blank
  src/platform/mod.rs   Platform trait, input event channel
  src/platform/macos.rs   IOPMAssertion, IORequestIdle/pmset, CoreBrightness backlight (probed), CGEventTap (untested on hardware)
  src/platform/windows.rs SetThreadExecutionState, SC_MONITORPOWER, Modern Standby detection, display-state callback, LL hooks
  src/platform/stub.rs  no-op for other platforms, compile checks only
ui/                     the real UI: plain HTML/JS/CSS, no framework, no bundler, no inline styles (CSP)
  popover / settings / overlay / onboarding .html + their .js/.css, shared nb.css and common.js
  i18n/<code>.js        UI strings (en, zh-CN), one file per language
site/                   website (static HTML/CSS); pushes to main auto-deploy via Cloudflare Pages (project blankrun-site) to blankrun.cosfoc.com
design/                 mockups; design/preview/ renders ui/ in Chrome with a fake __TAURI__ to check layouts
docs/screenshots/       English screenshots used in this README
```
Config file: `com.cosfoc.blankrun/config.toml` in the system app-config directory.

## Size targets
| Artifact | Target |
|---|---|
| Windows installer (NSIS, lzma, system WebView2) | ≤ 3 MB |
| macOS DMG | ≤ 4 MB |
| Main binary (single arch) | ≤ 5 MB |

How: `opt-level="z"`, `lto`, `codegen-units=1`, `panic="abort"`, `strip`; Tauri default features off, keeping only `wry`/`tray-icon`/`compression`;
`windows-sys` instead of `windows`; no chrono/regex/tokio in the core; zero frontend dependencies. Check the size impact before adding a crate.

## Development
```
cargo test -p blankrun-core                              # core tests, any platform
cargo check -p blankrun --target x86_64-pc-windows-gnu   # check the Windows code on Linux
cargo install tauri-cli --version "^2" && cargo tauri build  # package on a Mac / Windows machine
```
macOS targets only build on a Mac (or a macOS CI runner). On Linux you can type-check without linking:
```
rustup target add aarch64-apple-darwin
# objc2-exception-helper's build script compiles a .m file; a fake cc that emits an empty object works around it (check only)
CC_aarch64_apple_darwin=$PWD/tools/fake-cc.sh AR_aarch64_apple_darwin=ar cargo check -p blankrun --target aarch64-apple-darwin
```
UI preview: `NODE_PATH=<node_modules with playwright-core> node design/preview/render.mjs` renders both languages to `design/preview/out/`.

Logs: `blankrun.log` next to `config.toml`. For scripted tests: `blankrun --blank-now[=delay_s] --wake-after=s --quit-after=s` (on macOS: `open -n -a BlankRun --args …`).

Releases: pushing a `v*` tag runs `.github/workflows/release.yml`, which builds a universal macOS DMG and a Windows NSIS installer and publishes a GitHub Release (tags with `-` become pre-releases). Running the workflow manually builds without publishing.

## Adding a UI language

1. Copy `ui/i18n/en.js` to `ui/i18n/<code>.js` (for example `ja.js` or `zh-TW.js`). Change `NB_LOCALE("en", …)` and `"_name"` to the new code and the language's own name (for example "日本語").
2. Translate the strings. Anything left out falls back to English. `.mac` / `.win` suffixes are platform-specific variants, `{n}` and similar are placeholders, and `{ "one": …, "other": … }` are plural forms.
3. Rebuild. `build.rs` scans `ui/i18n/`, so the language shows up in Settings › General › Language, and "Match system" picks it automatically.

Before the UI loads, the tray menu uses `tray_text` in `src-tauri/src/main.rs` (Chinese and English). Once the UI loads it switches to the `tray.*` strings from the locale file, so a new language needs no Rust changes.

## License

[MIT](LICENSE) © 2026 COSFOC / hongfaqiu
