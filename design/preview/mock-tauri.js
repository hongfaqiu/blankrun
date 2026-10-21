// 浏览器预览用的假 window.__TAURI__（只在 design/preview 里用，不打包进应用）
(() => {
  const q = new URLSearchParams(location.search);
  const os = q.get("os") || "mac", state = q.get("state") || "idle";
  const cfg = {
    general: { launch_at_login: false, language: "auto", blank_now_hotkey: "Ctrl+Alt+B", check_updates: true },
    wake: { allow_remote: false, method: "hotkey", hotkey: "Ctrl+Alt+W", mouse_threshold_px: 150, mouse_window_ms: 800, lock_on_wake: false, reblank_after_s: 3, hold_ms: 1000 },
    schedule: { enabled: true, start: "23:30", end: "07:30", days: ["mon", "tue", "wed", "thu", "fri"], remind_before_min: 5, skip_when_fullscreen_video: true },
    display: { mode: q.get("mode") || "all", targets: q.get("keep") ? ["Built-in Retina Display"] : ["DISPLAY1"], method: "auto", prefs: { "LG HDR 4K": "overlay" }, no_ddc: ["DM73u pro"], keep_input: !!q.get("keep"), builtin_off: "auto" },
    lights: { keyboard_backlight: true, openrgb: false, openrgb_host: "127.0.0.1:6742" },
    safety: { emergency_hotkey: "Ctrl+Alt+Shift+Esc" }, power: { keep_awake: q.get("awake") !== "off", ac_only: false },
  };
  const MONS = os === "win"
    ? [{ id: "DISPLAY2", label: "DM73u pro", width: 3840, height: 2160, primary: true, virtual: false, power: "ddc", unresponsive: true },
       { id: "DISPLAY1", label: "HP X27q", width: 2560, height: 1440, primary: false, virtual: false, power: "ddc" },
       { id: "DISPLAY3", label: "OrayIddDriver Device", width: 1920, height: 1080, primary: false, virtual: true, power: null },
       { id: "DISPLAY4", label: "LG HDR 4K", width: 3840, height: 2160, primary: false, virtual: false, power: null }]
    : [{ id: "Built-in Retina Display", label: "Built-in Retina Display", width: 3024, height: 1964, primary: true, virtual: false, power: "disconnect" },
       { id: "DISPLAY1", label: "DELL U2723QE", width: 3840, height: 2160, primary: false, virtual: false, power: null }];
  const monReport = () => MONS.map((m) => {
    const pref = (cfg.display.prefs || {})[m.label] || "auto", un = !!m.unresponsive && pref !== "off";
    const how = m.virtual ? "skip" : cfg.display.mode === "selected" && !cfg.display.targets.includes(m.id) ? "skip" : pref === "overlay" || un ? "overlay" : m.power || "overlay";
    return { ...m, pref, unresponsive: un, how };
  });
  const status = {
    state, countdown_left: 272, countdown_total: 300, now_min: 20 * 60 + 43,
    schedule: { enabled: true, start: "23:30", end: "07:30", in_window: false, until_min: 167, duration_min: 480, skipped: false, waiting: null, postponed_min: null },
    hotkey_error: q.get("hkerr") || null, login: false, input_guard: q.get("guard") || "ok",
    displays: 2, keyboard: os === "mac", power_off: true, keep_awake: state !== "idle",
    update: { phase: q.get("upd") || "latest", version: q.get("upd") ? "0.1.2" : null, progress: q.get("upd") === "downloading" ? 42 : null, error: null, updated_from: q.get("from") || null },
  };
  const D = 1792596600, hist = { sessions: [
    { start: D - 86400 * 2, end: D - 86400 * 2 + 28800, trigger: "schedule", ended: "schedule", touches: 3, reblanks: 1 },
    { start: D - 86400 + 3000, end: D - 86400 + 5400, trigger: "hotkey", ended: "wake", touches: 0, reblanks: 0 },
    { start: D, end: D + 15120, trigger: "manual", ended: "crash", touches: 1, reblanks: 0 },
    { start: D + 40000, end: D + 40720, trigger: "manual", ended: "emergency", touches: 5, reblanks: 2 },
  ] };
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
        case "get_info": return { os, version: "0.1.1", system_locale: sysLocale, locales: [{ code: "en", name: "English" }, { code: "zh-CN", name: "简体中文" }], config_path: os === "mac" ? "/Users/me/Library/Application Support/com.cosfoc.blankrun/config.toml" : "C:\\Users\\me\\AppData\\Roaming\\com.cosfoc.blankrun\\config.toml" };
        case "get_config": return JSON.parse(JSON.stringify(cfg));
        case "save_config": Object.assign(cfg, args.cfg); setTimeout(() => window.__emit("nb://config", null), 0); return null;
        case "get_status": return status;
        case "get_history": return q.get("hist") === "none" ? { sessions: [] } : hist;
        case "skip_tonight": status.schedule.skipped = true; window.__emit("nb://status", status); return null;
        case "postpone": status.schedule.waiting = "postponed"; status.schedule.postponed_min = args.min; window.__emit("nb://status", status); return null;
        case "unskip": status.schedule.skipped = false; window.__emit("nb://status", status); return null;
        case "get_monitors": return monReport();
        case "get_diagnostics": return { caps, perms: os === "mac" ? [true, q.get("perm") !== "no"] : null, monitors: monReport(), modern_standby: os === "win", last_session: { start_unix: 1792596600, end_unix: 1792625400, lit: 2, by_schedule: true, power_off: true } };
        case "get_permissions": return os === "mac" ? [true, q.get("perm") !== "no"] : null;
        default: return null;
      }
    } },
    event: { listen: async (ev, cb) => { (handlers[ev] ||= []).push(cb); return () => {}; } },
  };
  window.__emit = (ev, payload) => (handlers[ev] || []).forEach((f) => f({ payload }));
})();
