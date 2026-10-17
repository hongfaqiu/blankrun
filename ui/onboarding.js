// Mac 首次启动的权限引导：每秒刷新一次授权状态
"use strict";
const SVG_NS = "http://www.w3.org/2000/svg";
function check() {
  const s = document.createElementNS(SVG_NS, "svg");
  s.setAttribute("width", "12"); s.setAttribute("height", "12"); s.setAttribute("viewBox", "0 0 12 12"); s.setAttribute("fill", "none");
  const p = document.createElementNS(SVG_NS, "path");
  p.setAttribute("d", "M2 6.2 4.8 9 10 3"); p.setAttribute("stroke", "currentColor"); p.setAttribute("stroke-width", "1.6");
  p.setAttribute("stroke-linecap", "round"); p.setAttribute("stroke-linejoin", "round");
  s.append(p); return s;
}
function stepUI(el, ok, which) {
  if (ok) { const d = h("span", "done"); d.append(check(), t("ob.enabled")); el.replaceChildren(d); el.dataset.state = "ok"; return; }
  if (el.dataset.state === "ask") return;
  const b = h("button", "btn primary", t("btn.open_settings"));
  b.onclick = () => invoke("request_permission", { which });
  el.replaceChildren(b);
  el.dataset.state = "ask";
}
async function refresh() {
  const d = await invoke("get_diagnostics");
  const [ax, inp] = d.perms || [true, true];
  stepUI($("#s1"), ax, "accessibility"); if (ax) $("#s1").dataset.state = "ok";
  stepUI($("#s2"), inp, "input"); if (inp) $("#s2").dataset.state = "ok";
  const n = (+ax) + (+inp);
  $("#doneText").textContent = t("ob.progress", { n });
  $("#doneDot").className = "dot" + (n === 2 ? " ok" : "");
  $("#done").disabled = n < 2;
  $("#done").classList.toggle("primary", n === 2);
}
$("#skip").addEventListener("click", () => invoke("finish_onboarding"));
$("#done").addEventListener("click", () => invoke("finish_onboarding"));
(async () => {
  // 换语言时清掉按钮让它按新语言重建
  await initPlatform(() => { $$("#s1, #s2").forEach((e) => (e.dataset.state = "")); refresh(); });
  const cfg = await invoke("get_config");
  renderKeys($("#wakeKeys"), cfg.wake.hotkey);
  listen("nb://config", async () => { const c = await invoke("get_config"); renderKeys($("#wakeKeys"), c.wake.hotkey); });
  refresh();
  setInterval(() => { if (!document.hidden) refresh(); }, 1000);
})();
