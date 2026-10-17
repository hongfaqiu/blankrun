// 开始前提醒：N 分钟后按计划息屏；可以跳过今晚、推迟 30 分钟，或知道了
"use strict";
let status = null;
function render() {
  if (!status) return;
  const sc = status.schedule, n = sc.until_min != null ? sc.until_min : 0;
  $("#ttl").textContent = t("r.title", { n });
  $("#when").replaceChildren(sc.start, " → ", sc.end);
}
const close = () => invoke("hide_window");
$("#closeBtn").addEventListener("click", close);
$("#okBtn").addEventListener("click", close);
$("#skipBtn").addEventListener("click", () => invoke("skip_tonight"));
$("#laterBtn").addEventListener("click", () => invoke("postpone", { min: 30 }));
document.addEventListener("keydown", (e) => { if (e.key === "Escape") close(); });

(async () => {
  await initPlatform(render);
  status = await invoke("get_status"); render();
  listen("nb://status", (s) => { status = s; render(); });
  listen("nb://shown", async () => { status = await invoke("get_status"); render(); });
})();
