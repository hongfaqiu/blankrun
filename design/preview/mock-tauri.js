// 浏览器预览用的假 window.__TAURI__（只在 design/preview 里用，不打包进应用）
(() => {
  const q = new URLSearchParams(location.search);
  const os = q.get("os") || "mac", state = q.get("state") || "idle";
  const cfg = {
    general: { launch_at_login: false, language: "auto", blank_now_hotkey: "Ctrl+Alt+B" },
    wake: { method: "hotkey", hotkey: "Ctrl+Alt+W", mouse_threshold_px: 150, mouse_window_ms: 800, lock_on_wake: false, reblank_after_s: 3, hold_ms: 1000 },
    schedule: { enabled: true, start: "23:30", end: "07:30", days: ["mon", "tue", "wed", "thu", "fri"], remind_before_min: 5, skip_when_fullscreen_video: true },
    display: { mode: q.get("mode") || "all", targets: ["DELL U2723QE"], method: "auto" },
    lights: { keyboard_backlight: true, openrgb: false, openrgb_host: "127.0.0.1:6742" },
    safety: { emergency_hotkey: "Ctrl+Alt+Shift+Esc" }, power: { ac_only: false },
  };
  const status = {
    state, countdown_left: 272, countdown_total: 300, now_min: 20 * 60 + 43,
    schedule: { enabled: true, start: "23:30", end: "07:30", in_window: false, until_min: 167, duration_min: 480, skipped: false },
    displays: 2, keyboard: os === "mac", power_off: true,
  };
  const caps = os === "mac" ? [
    { id: "awake", label: "防止系统睡眠", detail: "IOPMAssertion · PreventUserIdleSystemSleep", status: "ok", text: "正常" },
    { id: "kbd", label: "键盘背光", detail: "CoreBrightness · 系统更新后可能失效，失效时只关屏幕", status: "ok", text: "可用" },
    { id: "hook", label: "输入拦截", detail: "CGEventTap · 会话级", status: "ok", text: "正常" },
    { id: "ddc", label: "显示器断电（DDC/CI）", detail: "后续版本提供", status: "off", text: "未实现" },
  ] : [
    { id: "awake", label: "防止系统睡眠", detail: "SetThreadExecutionState · ES_SYSTEM_REQUIRED", status: "ok", text: "正常" },
    { id: "hook", label: "输入钩子", detail: "WH_KEYBOARD_LL / WH_MOUSE_LL · 无需管理员权限", status: "ok", text: "正常" },
    { id: "s0", label: "现代待机（S0）", detail: "这台电脑关屏后可能自动进入低功耗状态，自动模式下改用遮罩息屏", status: "off", text: "已规避" },
    { id: "kbd", label: "键盘背光", detail: "未识别到这台电脑的背光接口，厂商插件会在后续版本提供", status: "off", text: "不支持" },
  ];
  const handlers = {};
  const calls = (window.__calls = []);
  window.__TAURI__ = {
    core: { invoke: async (cmd, args) => {
      calls.push([cmd, args]);
      switch (cmd) {
        case "get_info": return { os, version: "0.1.0", config_path: os === "mac" ? "/Users/me/Library/Application Support/com.cosfoc.blankrun/config.toml" : "C:\\Users\\me\\AppData\\Roaming\\com.cosfoc.blankrun\\config.toml" };
        case "get_config": return JSON.parse(JSON.stringify(cfg));
        case "save_config": Object.assign(cfg, args.cfg); return null;
        case "get_status": return status;
        case "get_diagnostics": return { caps, perms: os === "mac" ? [true, q.get("perm") !== "no"] : null, monitors: ["Built-in Retina Display", "DELL U2723QE", "LG HDR 4K"], modern_standby: os === "win", last_session: { start_unix: 1792596600, end_unix: 1792625400, lit: 2, by_schedule: true, power_off: true } };
        default: return null;
      }
    } },
    event: { listen: async (ev, cb) => { (handlers[ev] ||= []).push(cb); return () => {}; } },
  };
  window.__emit = (ev, payload) => (handlers[ev] || []).forEach((f) => f({ payload }));
})();
