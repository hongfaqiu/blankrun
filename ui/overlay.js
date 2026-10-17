// 息屏遮罩：引擎通过 nb://overlay 告诉我们"被碰到了"或"正在按住唤醒键"
"use strict";
let cfg = null, hideTimer = null;

function line() {
  const w = cfg.wake, el = $("#line");
  if (w.method === "hotkey") {
    const k = h("span", "keys"); renderKeys(k, w.hotkey, "k");
    el.replaceChildren(t("o.hold_a"), k, t("o.hold_b", { s: secs(w.hold_ms) }));
  } else if (w.method === "schedule_only") {
    const k = h("span", "keys"); renderKeys(k, cfg.safety.emergency_hotkey, "k");
    el.replaceChildren(t("o.sched", { end: cfg.schedule.end }), k);
  } else el.textContent = t("o.wake." + w.method);
}

// 提示保持稳定：已经显示时只更新秒数和计时，不重放动画（连续误触不闪）
const me = window.__TAURI__ && window.__TAURI__.window ? window.__TAURI__.window.getCurrentWindow().label : "";
function showHint(p) {
  if (!cfg) return;
  const hint = $("#hint"), holding = p.kind === "hold";
  if (p.target && me && p.target !== me) { hint.hidden = true; return; }
  const was = !hint.hidden && !hint.classList.contains("out");
  if (!was || document.body.classList.contains("holding") !== holding) line();
  hint.hidden = false;
  hint.classList.remove("out");
  document.body.classList.toggle("holding", holding);
  $("#after").hidden = holding;
  $("#holdBox").hidden = !holding;
  $("#em").hidden = holding || cfg.wake.method === "schedule_only";
  if (holding) $("#bar").style.width = Math.round(p.progress * 100) + "%";
  else $("#secs").textContent = p.reblank_s;
  clearTimeout(hideTimer);
  // 在重新息屏前一点点淡出
  const ms = holding ? 1500 : Math.max(2000, p.reblank_s * 1000 - 600);
  hideTimer = setTimeout(() => { hint.classList.add("out"); hideTimer = setTimeout(() => { hint.hidden = true; }, 500); }, ms);
}

// 输入钩子装不上时（例如没给权限），遮罩自己收到的输入交给引擎判断
["keydown", "pointerdown"].forEach((ev) => document.addEventListener(ev, () => invoke("overlay_input")));

(async () => {
  await initPlatform();
  cfg = await invoke("get_config");
  renderKeys($("#emKeys"), cfg.safety.emergency_hotkey, "k");
  listen("nb://overlay", showHint);
  listen("nb://config", async () => { cfg = await invoke("get_config"); renderKeys($("#emKeys"), cfg.safety.emergency_hotkey, "k"); });
})();
