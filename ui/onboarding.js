// 第一次运行的欢迎页：怎么用、托盘图标在哪；Mac 缺权限时顺带引导授权（每秒刷新一次授权状态）
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
let permSeen = false;
async function refresh() {
  const d = await invoke("get_diagnostics");
  if (!d.perms) return; // Windows：不需要权限
  const [ax, inp] = d.perms;
  // 一开始就都授权了（比如重装）就不显示这一段；显示过就一直显示到关掉
  if (!(ax && inp)) permSeen = true;
  $("#perm").hidden = !permSeen;
  if (!permSeen) return;
  stepUI($("#s1"), ax, "accessibility"); if (ax) $("#s1").dataset.state = "ok";
  stepUI($("#s2"), inp, "input"); if (inp) $("#s2").dataset.state = "ok";
  const n = (+ax) + (+inp), all = n === 2;
  $("#wait").hidden = false; $("#skip").hidden = all;
  $("#doneText").textContent = t("ob.progress", { n });
  $("#doneDot").className = "dot" + (all ? " ok" : "");
  $("#done").disabled = !all;
  $("#done").textContent = t(all ? "ob.start" : "ob.done");
}
$("#skip").addEventListener("click", () => invoke("finish_onboarding"));
$("#done").addEventListener("click", () => invoke("finish_onboarding"));
(async () => {
  // 换语言时清掉按钮让它按新语言重建
  await initPlatform(() => { $$("#s1, #s2").forEach((e) => (e.dataset.state = "")); refresh(); });
  const keys = (c) => { renderKeys($("#wakeKeys"), c.wake.hotkey); renderKeys($("#blankKeys"), c.general.blank_now_hotkey); };
  keys(await invoke("get_config"));
  listen("nb://config", async () => keys(await invoke("get_config")));
  refresh();
  setInterval(() => { if (!document.hidden) refresh(); }, 1000);
})();
