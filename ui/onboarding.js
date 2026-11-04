// 首次使用引导（设置里也能重新打开）：欢迎 → 权限 → 显示器 → 唤醒 → 试一下 → 完成。
// 引导本身绝不自动息屏：只有在第 5 步点了「关屏 5 秒试试」才调用 trial_blank，引擎息屏 5 秒后自动恢复。
// 显示器的选择、只能遮罩的屏用遮罩还是跳过，都直接存进设置（和设置页同一份配置）。
"use strict";
const SVG_NS = "http://www.w3.org/2000/svg";
const STEPS = ["welcome", "perm", "displays", "wake", "try", "done"];
const WAKE = [["hotkey", "w.hotkey"], ["any_key", "w.any"], ["mouse_move", "w.mouse"], ["click", "w.click"], ["password", "w.pw"], ["schedule_only", "w.sched"]];
let step = 0, cfg = null, mons = [], status = null;

function check() {
  const s = document.createElementNS(SVG_NS, "svg");
  s.setAttribute("width", "12"); s.setAttribute("height", "12"); s.setAttribute("viewBox", "0 0 12 12"); s.setAttribute("fill", "none");
  const p = document.createElementNS(SVG_NS, "path");
  p.setAttribute("d", "M2 6.2 4.8 9 10 3"); p.setAttribute("stroke", "currentColor"); p.setAttribute("stroke-width", "1.6");
  p.setAttribute("stroke-linecap", "round"); p.setAttribute("stroke-linejoin", "round");
  s.append(p); return s;
}

// ---------- 翻页 ----------
function go(i) {
  step = Math.max(0, Math.min(STEPS.length - 1, i));
  $$(".page").forEach((p) => (p.hidden = p.dataset.step !== STEPS[step]));
  $("#stepNo").textContent = t("ob.of", { i: step + 1, n: STEPS.length });
  $("#back").hidden = step === 0;
  const last = step === STEPS.length - 1, name = STEPS[step];
  $("#next").textContent = t(last ? "ob.start" : "ob.next");
  $("#skip").hidden = !(name === "perm" || name === "try");
  $("#skip").textContent = t(name === "try" ? "ob.try_skip" : "ob.skip");
  if (name === "perm") refreshPerms();
  if (name === "displays") loadMons();
  if (name === "try") resetTry();
  renderKeysAll();
  $(".content").scrollTop = 0;
}
$("#back").addEventListener("click", () => go(step - 1));
$("#next").addEventListener("click", () => (step === STEPS.length - 1 ? invoke("finish_onboarding") : go(step + 1)));
$("#skip").addEventListener("click", () => go(step + 1));

async function saveCfg(mut) {
  cfg = await invoke("get_config");
  mut(cfg);
  await invoke("save_config", { cfg });
}

// ---------- 2 权限（Mac；Windows 不需要）。每秒刷新状态 ----------
function stepUI(el, ok, which) {
  if (ok) { const d = h("span", "done"); d.append(check(), t("ob.enabled")); el.replaceChildren(d); el.dataset.state = "ok"; return; }
  if (el.dataset.state === "ask") return;
  const b = h("button", "btn primary", t("btn.open_settings"));
  b.onclick = () => invoke("request_permission", { which });
  el.replaceChildren(b);
  el.dataset.state = "ask";
}
async function refreshPerms() {
  let p;
  try { p = await invoke("get_permissions"); } catch (e) { return; }
  $("#permMac").hidden = !p;
  $("#permNone").hidden = !!p;
  if (!p) return;
  const [ax, inp] = p;
  stepUI($("#s1"), ax, "accessibility");
  stepUI($("#s2"), inp, "input");
  $("#permNote").textContent = ax && inp ? t("ob.perm_ok") : t("ob.note");
}

// ---------- 3 显示器 ----------
async function loadMons() {
  try { [mons, cfg] = await Promise.all([invoke("get_monitors"), invoke("get_config")]); } catch (e) { return; }
  renderMons();
}
function ticked(m) { return !m.virtual && (cfg.display.mode !== "selected" || cfg.display.targets.includes(m.id)); }
function renderMons() {
  const box = $("#mons");
  if (!mons.length) { box.replaceChildren(h("p", "note", t("ob.disp_none"))); return; }
  box.replaceChildren(...mons.map((m) => {
    const on = ticked(m);
    const r = h("div", "mon" + (m.virtual ? " virtual" : ""));
    const box2 = h("button", "check" + (on ? " on" : ""));
    box2.setAttribute("role", "checkbox"); box2.setAttribute("aria-checked", on); box2.disabled = !!m.virtual;
    box2.onclick = () => toggleMon(m);
    const tx = h("div", "t");
    const name = h("b", "", m.builtin ? t("c.builtin") : m.label);
    if (m.virtual) name.append(h("i", "tag", t("ob.disp_virtual")));
    tx.append(name);
    tx.append(h("small", "", m.virtual ? t("b.how_virtual") : on ? t("b.how." + m.how) : t("b.mon_off_sel")));
    // 只能遮罩的屏：在这里就选好用遮罩还是跳过（记到设置里，以后不再问）
    if (on && m.fallback) {
      const ask = h("div", "ask");
      ask.append(h("small", "", t("ob.disp_ook", { why: t("c.why." + (m.reason || "no_power")) })));
      const seg = h("div", "seg");
      for (const [v, k] of [[true, "c.overlay"], [false, "c.skip"]]) {
        const b = h("button", m.overlay_ok === v ? "on" : "", t(k));
        b.onclick = async () => { await saveCfg((c) => { c.display.overlay_ok = { ...(c.display.overlay_ok || {}), [m.label]: v }; }); loadMons(); };
        seg.append(b);
      }
      ask.append(seg); tx.append(ask);
    }
    r.append(box2, tx);
    return r;
  }));
}
async function toggleMon(m) {
  const phys = mons.filter((x) => !x.virtual);
  const now = new Set(phys.filter(ticked).map((x) => x.id));
  if (now.has(m.id)) { if (now.size <= 1) return; now.delete(m.id); } else now.add(m.id);
  await saveCfg((c) => {
    // 全选时保持「全部」模式（以后接上的新屏也会关）；少选了才切到「只关所选」
    if (c.display.mode !== "selected" && now.size === phys.length) return;
    c.display.mode = "selected";
    c.display.targets = [...now];
  });
  loadMons();
}

// ---------- 4 唤醒 ----------
function renderWake() {
  if (!cfg) return;
  const sel = $("#wakeSel");
  sel.replaceChildren(...WAKE.map(([v, k]) => { const o = h("option", "", t(k)); o.value = v; o.selected = cfg.wake.method === v; return o; }));
  const hold = $("#wakeHold");
  if (cfg.wake.method === "hotkey") {
    const k = h("span", "keys"); renderKeys(k, cfg.wake.hotkey);
    hold.replaceChildren(k, t("ob.wake_hold", { s: secs(cfg.wake.hold_ms) }));
  } else hold.replaceChildren();
  const sum = $("#wakeSum");
  if (cfg.wake.method === "hotkey") { const k = h("span", "keys"); renderKeys(k, cfg.wake.hotkey); sum.replaceChildren(k, " ", t("ob.wake_hold", { s: secs(cfg.wake.hold_ms) })); }
  else sum.textContent = t("wake." + cfg.wake.method);
}
$("#wakeSel").addEventListener("change", (e) => saveCfg((c) => { c.wake.method = e.target.value; }).then(renderKeysAll));
$("#openWake").addEventListener("click", () => invoke("open_settings", { section: "wake" }));
function renderKeysAll() {
  if (!cfg) return;
  for (const id of ["#blankKeys", "#blankKeys2", "#blankKeys3"]) renderKeys($(id), cfg.general.blank_now_hotkey);
  for (const id of ["#emKeys", "#emKeys2"]) renderKeys($(id), cfg.safety.emergency_hotkey);
  renderWake();
}

// ---------- 5 试一下：只有点按钮才息屏；看到息屏结束后问「真的关了吗」 ----------
let trying = false, sawBlank = false, tryTimer = null;
function resetTry() {
  if (trying) return;
  $("#tryBtn").disabled = false; $("#tryWait").hidden = true; $("#tryAsk").hidden = true; $("#tryAns").textContent = "";
}
$("#tryBtn").addEventListener("click", async () => {
  if (trying || (status && status.state !== "idle")) return;
  trying = true; sawBlank = false;
  $("#tryBtn").disabled = true; $("#tryWait").hidden = false; $("#tryAsk").hidden = true;
  await invoke("trial_blank");
  // 没息屏（比如在确认窗里取消了、或所有屏都被跳过）：过一会儿把按钮还回来
  clearTimeout(tryTimer);
  tryTimer = setTimeout(() => { if (trying && !sawBlank) { trying = false; resetTry(); } }, 60000);
});
function onStatus(s) {
  status = s;
  if (!trying) return;
  if (s.state === "blank" || s.state === "countdown") sawBlank = true;
  else if (s.state === "idle" && sawBlank) {
    trying = false; clearTimeout(tryTimer);
    $("#tryWait").hidden = true; $("#tryBtn").disabled = false; $("#tryAsk").hidden = false;
  }
}
$("#tryYes").addEventListener("click", () => { $("#tryAns").textContent = t("ob.try_yes_a"); });
$("#tryNo").addEventListener("click", () => { $("#tryAns").textContent = t("ob.try_no_a"); });

(async () => {
  await initPlatform(() => { $$("#s1, #s2").forEach((e) => (e.dataset.state = "")); go(step); renderMons(); });
  cfg = await invoke("get_config");
  status = await invoke("get_status");
  go(0);
  listen("nb://config", async () => { cfg = await invoke("get_config"); renderKeysAll(); if (STEPS[step] === "displays") loadMons(); });
  listen("nb://status", onStatus);
  listen("nb://restart", () => { trying = false; go(0); });
  setInterval(() => { if (!document.hidden && STEPS[step] === "perm") refreshPerms(); }, 1000);
})();
