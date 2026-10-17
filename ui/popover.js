// 菜单栏 / 托盘弹出面板
"use strict";
let cfg = null, status = null;
let delay = +(localStorage.getItem("nb.delay") || 0);

function renderDelay() { $$("#delay button").forEach((b) => b.classList.toggle("on", +b.dataset.s === delay)); }

function renderCfg() {
  if (!cfg) return;
  const w = cfg.wake;
  const wake = $("#wakeText");
  if (w.method === "hotkey") {
    const k = h("span", "keys"); renderKeys(k, w.hotkey);
    wake.replaceChildren(t("p.hold_a"), k, t("p.hold_b", { s: secs(w.hold_ms) }));
  } else wake.textContent = t("wake." + w.method);
  if (OS === "mac") renderKeys($("#emKeys"), cfg.safety.emergency_hotkey);
  else $("#emKeys").replaceChildren(h("span", "mono", cfg.safety.emergency_hotkey));
}

function renderStatus() {
  if (!status) return;
  const s = status, counting = s.state === "countdown";
  $("#blankBtn").hidden = counting;
  $("#countBox").hidden = !counting;
  if (counting) $("#count").textContent = `${Math.floor(s.countdown_left / 60)}:${pad2(s.countdown_left % 60)}`;
  $("#stDot").className = "dot" + (counting ? " ink" : s.state === "blank" ? " standby" : "");
  $("#stText").textContent = t(counting ? "p.counting" : s.state === "blank" ? "p.blanked" : "p.idle");
  const off = [t("p.off_displays", { n: s.displays })]; if (s.keyboard) off.push(t("p.off_kbd"));
  $("#offText").textContent = off.join(" · ");
  $("#blankSub").textContent = t((s.displays > 1 ? "p.sub_n" : "p.sub") + (s.keyboard ? "_kbd" : ""), { n: s.displays });

  const sc = s.schedule;
  $("#planToggle").classList.toggle("on", sc.enabled);
  $("#planToggle").setAttribute("aria-checked", sc.enabled);
  $("#planDot").classList.toggle("standby", sc.enabled && !sc.skipped);
  document.body.classList.toggle("plan-off", !sc.enabled);
  const pt = $("#planTime"); pt.replaceChildren(sc.start, h("span", "arrow", "→"), sc.end);
  $("#planWhen").textContent = !sc.enabled ? t("p.when_off") : sc.skipped ? t("p.when_skipped") : sc.in_window ? t("p.when_in") : sc.until_min != null ? t("p.when_until", { d: fmtDur(sc.until_min) }) : "";
  drawTrack($("#track"), sc.start, sc.end, s.now_min);
}

$("#delay").addEventListener("click", (e) => {
  const b = e.target.closest("button"); if (!b) return;
  delay = +b.dataset.s; localStorage.setItem("nb.delay", delay); renderDelay();
});
$("#blankBtn").addEventListener("click", async () => {
  await invoke("blank_now", { delayS: delay });
  if (delay === 0) invoke("hide_window");
});
$("#cancelBtn").addEventListener("click", () => invoke("cancel"));
$("#planToggle").addEventListener("click", async () => {
  cfg = await invoke("get_config");
  cfg.schedule.enabled = !cfg.schedule.enabled;
  await invoke("save_config", { cfg });
  status = await invoke("get_status"); renderStatus();
});
$("#openSettings").addEventListener("click", () => invoke("open_settings", { section: null }));
$$("[data-open]").forEach((b) => b.addEventListener("click", () => invoke("open_settings", { section: b.dataset.open })));
$("#quitBtn").addEventListener("click", () => invoke("quit"));
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") invoke("hide_window");
  if (e.metaKey && e.key === ",") invoke("open_settings", { section: null });
  if (e.metaKey && e.key.toLowerCase() === "q") invoke("quit");
});

/** 托盘菜单和窗口标题跟着界面语言走（弹出面板在启动时就加载了，由它来同步） */
function syncNative() {
  const text = {};
  ["tray.blank", "tray.settings", "tray.quit", "tray.tooltip", "title.popover", "title.settings", "title.onboarding"].forEach((k) => (text[k] = t(k)));
  invoke("apply_locale", { text });
}

(async () => {
  await initPlatform(() => { syncNative(); renderCfg(); renderStatus(); });
  syncNative();
  renderDelay();
  [cfg, status] = await Promise.all([invoke("get_config"), invoke("get_status")]);
  renderCfg(); renderStatus();
  listen("nb://status", (s) => { status = s; renderStatus(); });
  listen("nb://config", async () => { cfg = await invoke("get_config"); renderCfg(); });
  listen("nb://shown", async () => { status = await invoke("get_status"); renderStatus(); });
})();
