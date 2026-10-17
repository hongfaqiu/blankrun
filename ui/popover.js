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
    wake.replaceChildren("按住 ", k, ` ${(w.hold_ms / 1000).toFixed(1).replace(/\.0$/, "")} 秒`);
  } else wake.textContent = WAKE_TEXT[w.method] || w.method;
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
  $("#stText").textContent = counting ? "倒计时中 · 已开始防睡眠" : s.state === "blank" ? "息屏中 · 正在防睡眠" : "待命 · 未在防睡眠";
  const off = [`${s.displays} 台显示器`]; if (s.keyboard) off.push("键盘背光");
  $("#offText").textContent = off.join(" · ");
  $("#blankSub").textContent = `关掉${s.displays > 1 ? ` ${s.displays} 台` : ""}显示器${s.keyboard ? "和键盘背光" : ""}，电脑保持运行`;

  const sc = s.schedule;
  $("#planToggle").classList.toggle("on", sc.enabled);
  $("#planToggle").setAttribute("aria-checked", sc.enabled);
  $("#planDot").classList.toggle("standby", sc.enabled && !sc.skipped);
  document.body.classList.toggle("plan-off", !sc.enabled);
  const pt = $("#planTime"); pt.replaceChildren(sc.start, h("span", "arrow", "→"), sc.end);
  $("#planWhen").textContent = !sc.enabled ? "未开启" : sc.skipped ? "今晚已跳过" : sc.in_window ? "进行中" : sc.until_min != null ? `${fmtDur(sc.until_min)}后开始` : "";
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

(async () => {
  await initPlatform();
  renderDelay();
  [cfg, status] = await Promise.all([invoke("get_config"), invoke("get_status")]);
  renderCfg(); renderStatus();
  listen("nb://status", (s) => { status = s; renderStatus(); });
  listen("nb://config", async () => { cfg = await invoke("get_config"); renderCfg(); });
  listen("nb://shown", async () => { status = await invoke("get_status"); renderStatus(); });
})();
