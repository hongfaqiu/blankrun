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

function showHint(p) {
  if (!cfg) return;
  const hint = $("#hint"), holding = p.kind === "hold";
  line();
  hint.hidden = false;
  document.body.classList.toggle("holding", holding);
  $("#after").hidden = holding;
  $("#holdBox").hidden = !holding;
  if (holding) $("#bar").style.width = Math.round(p.progress * 100) + "%";
  else $("#secs").textContent = p.reblank_s;
  // 重新触发淡出动画
  hint.classList.remove("fade");
  void hint.offsetWidth;
  if (!holding) hint.classList.add("fade");
  clearTimeout(hideTimer);
  hideTimer = setTimeout(() => { hint.hidden = true; }, holding ? 1500 : 3000);
}

// 输入钩子装不上时（例如没给权限），遮罩自己收到的输入交给引擎判断
["keydown", "pointerdown"].forEach((ev) => document.addEventListener(ev, () => invoke("overlay_input")));

(async () => {
  await initPlatform();
  cfg = await invoke("get_config");
  listen("nb://overlay", showHint);
  listen("nb://config", async () => { cfg = await invoke("get_config"); });
})();
