// 「只能遮罩」确认：有屏没法真正断电时，息屏前先问用遮罩还是跳过（默认跳过）。
// 关窗 / 取消 = 这次不息屏；继续 = 按每块屏的选择息屏，勾了「记住」的存进设置。
"use strict";
let asks = [], picks = {};
function render() {
  const list = $("#list");
  list.replaceChildren(...asks.map((a) => {
    const p = picks[a.id] || (picks[a.id] = { overlay: false, remember: false });
    const it = h("div", "item");
    it.append(h("div", "name", a.builtin ? t("c.builtin") : a.label));
    if (a.reason) it.append(h("div", "why", t("c.why." + a.reason)));
    const seg = h("div", "seg");
    for (const [v, k] of [[true, "c.overlay"], [false, "c.skip"]]) {
      const b = h("button", p.overlay === v ? "on" : "", t(k));
      b.onclick = () => { p.overlay = v; render(); };
      seg.append(b);
    }
    const rem = h("button", "rem"); const box = h("span", "check" + (p.remember ? " on" : ""));
    rem.setAttribute("role", "checkbox"); rem.setAttribute("aria-checked", p.remember);
    rem.append(box, t("c.remember"));
    rem.onclick = () => { p.remember = !p.remember; render(); };
    it.append(seg, rem);
    return it;
  }));
  fit();
}
let lastH = 0;
function fit() {
  const want = Math.ceil($("#cf").getBoundingClientRect().height);
  if (want !== lastH && T.window) { lastH = want; T.window.getCurrentWindow().setSize(new T.window.LogicalSize(440, want)).catch(() => {}); }
}
async function load() { asks = await invoke("get_overlay_ask"); picks = {}; render(); }
function answer(go) {
  const choices = asks.map((a) => ({ id: a.id, overlay: !!(picks[a.id] && picks[a.id].overlay), remember: !!(picks[a.id] && picks[a.id].remember) }));
  invoke("overlay_answer", { choices, go });
  invoke("hide_window");
}
$("#cancelBtn").addEventListener("click", () => answer(false));
$("#goBtn").addEventListener("click", () => answer(true));
document.addEventListener("keydown", (e) => { if (e.key === "Escape") answer(false); });
(async () => {
  await initPlatform(render);
  await load();
  listen("nb://ask", load);
})();
