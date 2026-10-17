// 设置窗口：表单 ↔ config（Tauri 命令 get_config / save_config），改动即保存
"use strict";
let cfg = null, status = null, diag = null, recording = null;

const getK = (path) => path.split(".").reduce((o, k) => o?.[k], cfg);
const setK = (path, v) => { const ks = path.split("."); const last = ks.pop(); ks.reduce((o, k) => o[k], cfg)[last] = v; };

let saveTimer = null;
function save() {
  clearTimeout(saveTimer);
  saveTimer = setTimeout(async () => {
    try { await invoke("save_config", { cfg }); }
    catch (e) { toast("保存失败：" + e); }
    saveTimer = null;
  }, 250);
}
function change(path, v) { setK(path, v); render(); save(); }

let toastTimer = null;
function toast(msg) { const t = $("#toast"); t.textContent = msg; t.hidden = false; clearTimeout(toastTimer); toastTimer = setTimeout(() => (t.hidden = true), 2600); }

// ---------- 导航 ----------
function show(id) {
  id = id || (location.hash || "#general").slice(1);
  $$("section").forEach((s) => s.classList.toggle("on", s.id === id));
  $$(".nav a").forEach((a) => a.classList.toggle("on", a.getAttribute("href") === "#" + id));
  if (id === "about") loadDiag();
}
addEventListener("hashchange", () => show());

// ---------- 渲染 ----------
function render() {
  if (!cfg) return;
  $$("button.toggle[data-k]").forEach((b) => { const v = !!getK(b.dataset.k) && !b.disabled; b.classList.toggle("on", v); b.setAttribute("aria-checked", v); });
  $$("select[data-k]").forEach((s) => (s.value = String(getK(s.dataset.k))));
  $$(".seg[data-k]").forEach((g) => $$("button", g).forEach((b) => b.classList.toggle("on", b.dataset.v === getK(g.dataset.k))));
  $$("[data-radio]").forEach((g) => $$(".pick", g).forEach((r) => $(".radio", r).classList.toggle("on", r.dataset.v === getK(g.dataset.radio))));
  $$(".field[data-num]").forEach((f) => { const i = $("input", f); if (document.activeElement !== i) i.value = getK(f.dataset.num); });
  $$("input[data-range]").forEach((r) => (r.value = getK(r.dataset.range)));
  $$("input[data-time]").forEach((t) => { if (document.activeElement !== t) t.value = getK(t.dataset.time); });
  $$(".recorder[data-hk]").forEach((r) => { r.classList.toggle("rec", recording === r); renderKeys($(".keys", r), getK(r.dataset.hk)); });

  const w = cfg.wake;
  $("#holdVal").textContent = (w.hold_ms / 1000).toFixed(1) + " 秒";
  $("#holdRow").classList.toggle("dim", w.method !== "hotkey");
  $("#mouseRow").classList.toggle("dim", w.method !== "mouse_move");
  $("#rgbHost").textContent = cfg.lights.openrgb_host;
  $("#methodNote").textContent = {
    auto: "自动：能断电的断电，不能断电的用遮罩",
    power_off: "让显示器进入睡眠（断电）。屏幕被碰亮后会按下面的设置自动再关",
    overlay: "只铺黑色遮罩，背光仍亮着；适合关屏会让系统睡下去的电脑",
  }[cfg.display.method];

  // 定时
  const s = cfg.schedule;
  const days = $("#days");
  if (!days.children.length) DAYS.forEach((d, i) => { const b = h("button", "", DAY_ZH[i]); b.dataset.d = d; days.append(b); });
  $$("button", days).forEach((b) => b.classList.toggle("on", s.days.includes(b.dataset.d)));
  const ps = parseHM(s.start), pe = parseHM(s.end);
  if (ps != null && pe != null) {
    const dur = pe > ps ? pe - ps : pe + 1440 - ps;
    $("#schedDur").textContent = `${pe > ps ? "当天" : "次日"}结束 · 共 ${fmtDur(dur)}`;
  }
  drawTrack($("#track"), s.start, s.end, status ? status.now_min : 0);
  renderMonitors();
  renderStatus();
}

function renderStatus() {
  if (!status || !cfg) return;
  const sc = status.schedule, en = cfg.schedule.enabled;
  $("#schedDot").classList.toggle("standby", en && !sc.skipped);
  $("#skipBtn").hidden = !en || sc.skipped;
  let next = "关闭时只能手动息屏";
  if (en) {
    if (sc.skipped) next = "本次已跳过，下一个时段照常";
    else if (sc.in_window) next = `进行中，${cfg.schedule.end} 结束`;
    else if (sc.until_min != null) next = `下次：${fmtDur(sc.until_min)}后（${cfg.schedule.start}）开始，${cfg.schedule.end} 结束`;
    else next = "没有勾选任何一天";
  }
  $("#schedNext").textContent = next;
}

function renderMonitors() {
  const box = $("#monitors");
  const mons = diag ? diag.monitors : [];
  $("#monCount").textContent = mons.length ? `检测到 ${mons.length} 台` : "";
  const sel = cfg.display.mode === "selected";
  box.replaceChildren(...mons.map((name) => {
    const r = h("div", "r mon" + (sel ? "" : " dim"));
    const on = !sel || cfg.display.targets.includes(name);
    const lead = h("div", "lead-ic");
    lead.append(h("span", "check" + (on ? " on" : "")));
    const t = h("div", "t"); t.append(h("span", "", name), h("small", "", sel ? (on ? "息屏时盖上黑色遮罩" : "保持点亮") : "随「全部显示器」一起关闭"));
    lead.append(t); r.append(lead);
    r.dataset.mon = name;
    return r;
  }));
}

async function loadDiag() {
  try { diag = await invoke("get_diagnostics"); } catch (e) { return; }
  renderMonitors();
  $("#s0Row").hidden = !diag.modern_standby;
  // 能力列表
  $("#caps").replaceChildren(...diag.caps.map((c) => {
    const r = h("div", "r");
    const t = h("div", "t"); t.append(h("span", "", c.label), h("small", "mono", c.detail));
    const st = h("span", "status " + (c.status === "ok" ? "" : c.status)); st.append(h("span", "dot " + (c.status === "ok" ? "ok" : c.status === "bad" ? "bad" : "")), c.text);
    const cc = h("div", "c"); cc.append(st);
    r.append(t, cc); return r;
  }));
  const kbd = diag.caps.find((c) => c.id === "kbd");
  if (kbd && kbd.status !== "ok") { $("#kbdToggle").disabled = true; $("#kbdNote").textContent = kbd.detail; render(); }
  // 权限（仅 Mac）
  if (diag.perms) {
    $("#permBlock").hidden = false;
    const perm = (el, ok, which) => {
      const st = h("span", "status" + (ok ? "" : " bad")); st.append(h("span", "dot " + (ok ? "ok" : "bad")), ok ? "已授权" : "未授权");
      el.replaceChildren(st);
      if (!ok) { const b = h("button", "btn primary", "打开系统设置"); b.onclick = () => invoke("request_permission", { which }); el.append(b); }
    };
    perm($("#permAx"), diag.perms[0], "accessibility");
    perm($("#permIn"), diag.perms[1], "input");
  }
  // 上次息屏
  const ls = diag.last_session;
  if (ls) {
    const f = (u) => { const d = new Date(u * 1000); return `${d.getMonth() + 1}/${d.getDate()} ${pad2(d.getHours())}:${pad2(d.getMinutes())}`; };
    $("#lastRange").textContent = `${f(ls.start_unix)} – ${ls.end_unix ? f(ls.end_unix) : "现在"}`;
    $("#lastNote").textContent = `${ls.by_schedule ? "按计划" : "手动"}息屏 · ${ls.power_off ? "显示器断电" : "黑色遮罩"} · 期间屏幕被意外点亮 ${ls.lit} 次，都按设置重新息屏`;
  }
}

// ---------- 交互 ----------
document.addEventListener("click", (e) => {
  const t = e.target;
  const tog = t.closest("button.toggle[data-k]");
  if (tog && !tog.disabled) return change(tog.dataset.k, !getK(tog.dataset.k));
  const segB = t.closest(".seg[data-k] button");
  if (segB) return change(segB.parentElement.dataset.k, segB.dataset.v);
  const rec = t.closest(".recorder[data-hk]");
  if (rec && t.closest("button.x")) { recording = recording === rec ? null : rec; return render(); }
  const pick = t.closest("[data-radio] .pick");
  if (pick && !rec) return change(pick.parentElement.dataset.radio, pick.dataset.v);
  const step = t.closest(".field[data-num] .step button");
  if (step) {
    const f = step.closest(".field"), k = f.dataset.num, sv = +(f.dataset.stepv || 1);
    const v = Math.min(+f.dataset.max, Math.max(+f.dataset.min, getK(k) + sv * +step.dataset.d));
    return change(k, v);
  }
  const day = t.closest("#days button");
  if (day) {
    const d = day.dataset.d, list = cfg.schedule.days;
    const next = list.includes(d) ? list.filter((x) => x !== d) : DAYS.filter((x) => x === d || list.includes(x));
    return change("schedule.days", next);
  }
  const mon = t.closest(".r.mon");
  if (mon && cfg.display.mode === "selected") {
    const n = mon.dataset.mon, tg = cfg.display.targets;
    return change("display.targets", tg.includes(n) ? tg.filter((x) => x !== n) : [...tg, n]);
  }
});
document.addEventListener("change", (e) => {
  const t = e.target;
  if (t.matches("select[data-k]")) change(t.dataset.k, t.dataset.type === "num" ? +t.value : t.value);
  else if (t.matches(".field[data-num] input")) {
    const f = t.closest(".field");
    const v = Math.round(+t.value);
    change(f.dataset.num, Number.isFinite(v) ? Math.min(+f.dataset.max, Math.max(+f.dataset.min, v)) : getK(f.dataset.num));
  } else if (t.matches("input[data-time]")) {
    // 接受 "2330"、"7:30"、"07:30"，统一成 HH:MM
    const raw = t.value.trim().replace("：", ":");
    const m = /^(\d{1,2}):?(\d{2})$/.exec(raw);
    const ok = m && +m[1] < 24 && +m[2] < 60;
    t.classList.toggle("bad", !ok);
    if (ok) { t.blur(); change(t.dataset.time, `${pad2(+m[1])}:${m[2]}`); }
  }
});
document.addEventListener("keydown", (e) => { if (e.key === "Enter" && e.target.matches("input")) e.target.blur(); });
document.addEventListener("input", (e) => {
  if (e.target.matches("input[data-range]")) change(e.target.dataset.range, +e.target.value);
});
// 录制快捷键：按下组合键即保存，Esc 取消
document.addEventListener("keydown", (e) => {
  if (!recording) return;
  e.preventDefault();
  if (e.key === "Escape" && !e.ctrlKey && !e.altKey && !e.shiftKey && !e.metaKey) { recording = null; return render(); }
  const c = comboFromEvent(e);
  if (!c) return;
  const k = recording.dataset.hk;
  recording = null;
  change(k, c);
}, true);

$("#reveal").addEventListener("click", () => invoke("reveal_config"));
$("#skipBtn").addEventListener("click", async () => { await invoke("skip_tonight"); toast("今晚的计划已跳过"); });
$("#exportDiag").addEventListener("click", async () => {
  await loadDiag();
  const info = await invoke("get_info");
  const text = [`BlankRun ${info.version} (${info.os})`, `时间 ${new Date().toString()}`, "", "[能力]",
    ...diag.caps.map((c) => `- ${c.label}: ${c.text} · ${c.detail}`),
    diag.perms ? `\n[权限] 辅助功能=${diag.perms[0]} 输入监控=${diag.perms[1]}` : "",
    `\n[显示器] ${diag.monitors.join(", ")}`, `\n[上次息屏] ${JSON.stringify(diag.last_session)}`,
    "\n[配置]", JSON.stringify(cfg, null, 2)].join("\n");
  try { const p = await invoke("export_diagnostics", { text }); toast("已导出到 " + p); } catch (e) { toast("导出失败：" + e); }
});

(async () => {
  const info = await initPlatform();
  $("#cfgPath").textContent = info.config_path;
  $("#ver").textContent = info.version;
  [cfg, status] = await Promise.all([invoke("get_config"), invoke("get_status")]);
  show();
  render();
  loadDiag();
  listen("nb://status", (s) => { status = s; renderStatus(); drawTrack($("#track"), cfg.schedule.start, cfg.schedule.end, s.now_min); });
  // 外部编辑 config.toml 后同步
  listen("nb://config", async () => { if (saveTimer) return; cfg = await invoke("get_config"); render(); });
  listen("nb://nav", (id) => { location.hash = "#" + id; show(id); });
})();
