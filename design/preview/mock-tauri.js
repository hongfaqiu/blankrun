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
    { id: "awake", key: "awake_ok", status: "ok", tech: "IOPMAssertion · PreventUserIdleSystemSleep" },
    { id: "kbd", key: "kbd_mac_ok", status: "ok", tech: "CoreBrightness" },
    { id: "hook", key: "hook_mac_ok", status: "ok", tech: "CGEventTap" },
    { id: "ddc", key: "ddc_todo", status: "off", tech: "DDC/CI" },
  ] : [
    { id: "awake", key: "awake_ok", status: "ok", tech: "SetThreadExecutionState · ES_SYSTEM_REQUIRED" },
    { id: "hook", key: "hook_win_ok", status: "ok", tech: "WH_KEYBOARD_LL / WH_MOUSE_LL" },
    { id: "s0", key: "s0_standby", status: "off", tech: "AoAc / Modern Standby" },
    { id: "kbd", key: "kbd_win_off", status: "off", tech: "—" },
  ];
  // ?lang=zh-CN|en 设置配置里的语言；?sys= 模拟系统语言（默认 zh-CN）
  if (q.get("lang")) cfg.general.language = q.get("lang");
  const sysLocale = q.get("sys") || "zh-CN";
  const handlers = {};
  const calls = (window.__calls = []);
  window.__TAURI__ = {
    core: { invoke: async (cmd, args) => {
      calls.push([cmd, args]);
      switch (cmd) {
        case "get_info": return { os, version: "0.1.0-alpha", system_locale: sysLocale, locales: [{ code: "en", name: "English" }, { code: "zh-CN", name: "简体中文" }], config_path: os === "mac" ? "/Users/me/Library/Application Support/com.cosfoc.blankrun/config.toml" : "C:\\Users\\me\\AppData\\Roaming\\com.cosfoc.blankrun\\config.toml" };
        case "get_config": return JSON.parse(JSON.stringify(cfg));
        case "save_config": Object.assign(cfg, args.cfg); setTimeout(() => window.__emit("nb://config", null), 0); return null;
        case "get_status": return status;
        case "skip_tonight": status.schedule.skipped = true; window.__emit("nb://status", status); return null;
        case "unskip": status.schedule.skipped = false; window.__emit("nb://status", status); return null;
        case "get_diagnostics": return { caps, perms: os === "mac" ? [true, q.get("perm") !== "no"] : null, monitors: ["Built-in Retina Display", "DELL U2723QE", "LG HDR 4K"], modern_standby: os === "win", last_session: { start_unix: 1792596600, end_unix: 1792625400, lit: 2, by_schedule: true, power_off: true } };
        default: return null;
      }
    } },
    event: { listen: async (ev, cb) => { (handlers[ev] ||= []).push(cb); return () => {}; } },
  };
  window.__emit = (ev, payload) => (handlers[ev] || []).forEach((f) => f({ payload }));
})();
