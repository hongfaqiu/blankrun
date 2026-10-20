// 设置窗口：表单 ↔ config（Tauri 命令 get_config / save_config），改动即保存
"use strict";
let cfg = null, status = null, diag = null, recording = null;

const getK = (path) => path.split(".").reduce((o, k) => o?.[k], cfg);
const setK = (path, v) => { const ks = path.split("."); const last = ks.pop(); ks.reduce((o, k) => o[k], cfg)[last] = v; };

let saveTimer = null;
function save() {
  clearTimeout(saveTimer);
  saveTimer = setTimeout(async () => {
    try { await invoke("save_config", { cfg }); refreshMonitors(); }
    catch (e) { toast(t("toast.save_fail", { e })); }
    saveTimer = null;
  }, 250);
}
function change(path, v) { setK(path, v); render(); save(); }

let toastTimer = null;
function toast(msg) { const el = $("#toast"); el.textContent = msg; el.hidden = false; clearTimeout(toastTimer); toastTimer = setTimeout(() => (el.hidden = true), 2600); }

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
  // 「键鼠照常用」只在「只关所选」时有意义
  $("#keepToggle").disabled = cfg.display.mode !== "selected";
  $("#keepRow").classList.toggle("dim", cfg.display.mode !== "selected");
  $("#builtinRow").hidden = OS !== "mac";
  $$("button.toggle[data-k]").forEach((b) => { const v = !!getK(b.dataset.k) && !b.disabled; b.classList.toggle("on", v); b.setAttribute("aria-checked", v); });
  $$("select[data-k]").forEach((s) => (s.value = String(getK(s.dataset.k))));
  $$(".seg[data-k]").forEach((g) => $$("button", g).forEach((b) => b.classList.toggle("on", b.dataset.v === getK(g.dataset.k))));
  $$("[data-radio]").forEach((g) => $$(".pick", g).forEach((r) => $(".radio", r).classList.toggle("on", r.dataset.v === getK(g.dataset.radio))));
  $$(".field[data-num]").forEach((f) => { const i = $("input", f); if (document.activeElement !== i) i.value = getK(f.dataset.num); });
  $$("input[data-range]").forEach((r) => (r.value = getK(r.dataset.range)));
  $$(".tp").forEach((tp) => {
    const [hh, mm] = getK(tp.dataset.time).split(":");
    const m = $(".mm", tp);
    // 分钟按 5 分钟一档；配置里是别的分钟数（手改 config.toml）时也要能显示
    if (![...m.options].some((o) => o.value === mm)) m.append(new Option(mm, mm));
    $(".hh", tp).value = hh; m.value = mm;
  });
  $$(".recorder[data-hk]").forEach((r) => { r.classList.toggle("rec", recording === r); renderKeys($(".keys", r), getK(r.dataset.hk)); });

  const w = cfg.wake;
  $("#holdVal").textContent = t("unit.s", { s: (w.hold_ms / 1000).toFixed(1) });
  $("#holdRow").classList.toggle("dim", w.method !== "hotkey");
  $("#mouseRow").classList.toggle("dim", w.method !== "mouse_move");
  $("#rgbHost").textContent = cfg.lights.openrgb_host;
  $("#methodNote").textContent = t("b.note." + cfg.display.method);

  // 定时
  const s = cfg.schedule;
  const days = $("#days");
  if (!days.children.length) DAYS.forEach((d) => { const b = h("button"); b.dataset.d = d; b.dataset.i18n = "day." + d; days.append(b); });
  $$("button", days).forEach((b) => (b.textContent = t(b.dataset.i18n)));
  $$("button", days).forEach((b) => b.classList.toggle("on", s.days.includes(b.dataset.d)));
  const ps = parseHM(s.start), pe = parseHM(s.end);
  if (ps != null && pe != null) {
    const dur = pe > ps ? pe - ps : pe + 1440 - ps;
    $("#schedDur").textContent = t(pe > ps ? "s.dur_same" : "s.dur_next", { d: fmtDur(dur) });
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
  $("#unskipBtn").hidden = !en || !sc.skipped;
  const he = status.hotkey_error;
  $("#hkErr").hidden = !he;
  if (he) $("#hkErr").textContent = /already used/.test(he) ? t("g.hk_taken") : /needs/.test(he) ? t("g.hk_mod") : t("g.hk_bad", { e: he });
  const v = { start: cfg.schedule.start, end: cfg.schedule.end };
  let next = t("s.next_off");
  if (en) {
    if (sc.skipped) next = t("s.next_skipped");
    else if (sc.in_window) next = t("s.next_in", v);
    else if (sc.until_min != null) next = t("s.next_until", { ...v, d: fmtDur(sc.until_min) });
    else next = t("s.next_nodays");
  }
  $("#schedNext").textContent = next;
  renderUpdate();
  if (status.state !== lastState) { lastState = status.state; if (status.state === "idle") loadHistory(); }
}
let lastState = null, history = [];
function renderUpdate() {
  const u = status.update || {}, ph = u.phase || "idle";
  const busy = ph === "checking" || ph === "downloading" || ph === "installing";
  const blank = status.state !== "idle";
  $("#updText").textContent =
    ph === "available" ? t("a.upd_new", { v: u.version }) :
    ph === "downloading" ? t("a.upd_downloading", { p: u.progress || 0 }) :
    ph === "installing" ? t("a.upd_installing") :
    ph === "checking" ? t("a.upd_checking") :
    ph === "latest" ? t("a.upd_latest") :
    ph === "error" ? t("a.upd_error") : t("a.upd_idle", { v: INFO ? INFO.version : "" });
  const notes = [];
  if (ph === "error" && u.error) notes.push(u.error);
  if (ph === "available") notes.push(t(blank ? "a.upd_blank" : OS === "mac" ? "a.upd_mac" : "a.upd_win"));
  if (u.updated_from) notes.push(t(OS === "mac" ? "a.upd_done_mac" : "a.upd_done", { v: u.updated_from }));
  $("#updNote").textContent = notes.join(" · ");
  $("#updNotes").hidden = ph !== "available";
  $("#updInstall").hidden = ph !== "available" && ph !== "downloading" && ph !== "installing";
  $("#updInstall").disabled = busy || blank;
  $("#updCheck").hidden = ph === "available" || ph === "downloading" || ph === "installing";
  $("#updCheck").disabled = busy;
}
const TRIG = { manual: "a.tr_manual", hotkey: "a.tr_hotkey", schedule: "a.tr_schedule" };
async function loadHistory() {
  try { history = (await invoke("get_history")).sessions || []; } catch (e) { return; }
  renderHistory();
}
function renderHistory() {
  const box = $("#hist");
  $("#histClear").hidden = !history.length;
  if (!history.length) { box.replaceChildren(h("div", "r", "")); box.firstChild.append(h("div", "t", "")); box.firstChild.firstChild.append(h("small", "", t("a.hist_none"))); return; }
  const day = (u) => new Date(u * 1000).toLocaleDateString(LANG, { month: "numeric", day: "numeric", weekday: "short" });
  const hm = (u) => new Date(u * 1000).toLocaleTimeString(LANG, { hour: "2-digit", minute: "2-digit", hour12: false });
  const dur = (s) => { const m = Math.round(s / 60); return m < 60 ? t("a.dur_m", { m }) : t("a.dur_hm", { h: Math.floor(m / 60), m: m % 60 }); };
  box.replaceChildren(...history.slice().reverse().map((s) => {
    const r = h("div", "r");
    const tt = h("div", "t");
    tt.append(h("span", "num", `${day(s.start)}  ${hm(s.start)} – ${s.ended === "running" ? t("a.now") : hm(s.end)}`));
    const bits = [t(TRIG[s.trigger] || "a.tr_manual"), t("a.end_" + s.ended)];
    if (s.touches) bits.push(t("a.h_touches", { n: s.touches }));
    if (s.reblanks) bits.push(t("a.h_reblanks", { n: s.reblanks }));
    tt.append(h("small", "", bits.join(" · ")));
    const c = h("div", "c"); c.append(h("span", "num muted", s.ended === "running" ? "" : dur(s.end - s.start)));
    if (s.ended === "crash") c.firstChild.classList.add("warn");
    r.append(tt, c); return r;
  }));
}
$("#histClear").addEventListener("click", async () => {
  if (!confirm(t("a.hist_confirm"))) return;
  await invoke("clear_history"); history = []; renderHistory();
});
$("#updCheck").addEventListener("click", () => invoke("check_update"));
$("#updInstall").addEventListener("click", () => invoke("install_update").catch(() => {}));
document.querySelectorAll("[data-link]").forEach((b) => b.addEventListener("click", () => invoke("open_link", { which: b.dataset.link })));

function renderMonitors() {
  const box = $("#monitors");
  const mons = diag ? diag.monitors : [];
  $("#monCount").textContent = mons.length ? t("b.mon_count", { n: mons.length }) : "";
  const sel = cfg.display.mode === "selected";
  box.replaceChildren(...mons.map((m) => {
    const r = h("div", "r mon" + (sel && !m.virtual ? "" : " dim"));
    const on = !m.virtual && (!sel || cfg.display.targets.includes(m.id));
    const lead = h("div", "lead-ic");
    lead.append(h("span", "check" + (on ? " on" : "") + (m.virtual ? " off" : "")));
    const name = h("span", "", m.label);
    if (m.primary) name.append(h("i", "tag", t("b.mon_primary")));
    if (m.virtual) name.append(h("i", "tag", t("b.mon_virtual")));
    const how = m.virtual ? t("b.how_virtual") : on ? t("b.how." + m.how) + (m.unresponsive && m.how === "overlay" ? " · " + t("b.unresponsive") : "") : t("b.mon_off_sel");
    const tx = h("div", "t"); tx.append(name, h("small", "", [m.width && `${m.width} × ${m.height}`, how].filter(Boolean).join(" · ")));
    lead.append(tx); r.append(lead);
    if (!m.virtual) {
      // 每台单独指定：自动 / 仅遮罩 / 断电（只有能单独断电的屏才有）
      const ps = h("select"); ps.dataset.pref = m.label;
      for (const v of m.power ? ["auto", "overlay", "off"] : ["auto", "overlay"]) {
        const o = h("option", "", t("b.pref." + v)); o.value = v; o.selected = (m.pref || "auto") === v; ps.append(o);
      }
      const wrap = h("span", "select"); wrap.append(ps);
      const c = h("div", "c"); c.append(wrap); r.append(c);
    }
    r.dataset.mon = m.id;
    if (m.virtual) r.dataset.virtual = "1";
    return r;
  }));
  // 「键鼠照常用」的说明：没选「只关所选」/ 所选包含全部物理屏时说明为什么不生效
  const phys = mons.filter((m) => !m.virtual), tg = cfg.display.targets;
  $("#keepNote").textContent = !sel ? t("b.keep_need_sel")
    : phys.length && phys.every((m) => tg.includes(m.id)) ? t("b.keep_all_sel")
    : t("b.keep_note");
  // 有物理屏只能用遮罩时，提示去显示器菜单里打开 DDC/CI
  $("#ddcHint").hidden = !mons.some((m) => !m.virtual && (!m.power || m.unresponsive));
}
async function refreshMonitors() {
  try { const m = await invoke("get_monitors"); if (diag) { diag.monitors = m; renderMonitors(); } } catch (e) {}
}

async function loadDiag() {
  try { diag = await invoke("get_diagnostics"); } catch (e) { return; }
  renderMonitors();
  $("#s0Row").hidden = !diag.modern_standby;
  // 能力列表
  renderCaps();
  const kbd = diag.caps.find((c) => c.id === "kbd");
  if (kbd && kbd.status !== "ok") { $("#kbdToggle").disabled = true; render(); }
  // 权限（仅 Mac）
  if (diag.perms) {
    $("#permBlock").hidden = false;
    const perm = (el, ok, which) => {
      const st = h("span", "status" + (ok ? "" : " bad")); st.append(h("span", "dot " + (ok ? "ok" : "bad")), t(ok ? "st.granted" : "st.denied"));
      el.replaceChildren(st);
      if (!ok) { const b = h("button", "btn primary", t("btn.open_settings")); b.onclick = () => invoke("request_permission", { which }); el.append(b); }
    };
    perm($("#permAx"), diag.perms[0], "accessibility");
    perm($("#permIn"), diag.perms[1], "input");
  }
  // 上次息屏
  loadHistory();
}

/** 能力列表：名称 / 状态 / 说明来自词条 cap.<key>.*，tech 是接口名和原始错误，不翻译 */
function renderCaps() {
  if (!diag) return;
  $("#caps").replaceChildren(...diag.caps.map((c) => {
    const r = h("div", "r");
    const note = lookup(`cap.${c.key}.note`);
    const sm = h("small"); if (note) sm.append(note + " · "); sm.append(h("span", "mono", c.tech));
    const tt = h("div", "t"); tt.append(h("span", "", t(`cap.${c.key}.name`)), sm);
    const st = h("span", "status " + (c.status === "ok" ? "" : c.status)); st.append(h("span", "dot " + (c.status === "ok" ? "ok" : c.status === "bad" ? "bad" : "")), t(`cap.${c.key}.state`));
    const cc = h("div", "c"); cc.append(st);
    r.append(tt, cc); return r;
  }));
  const kbd = diag.caps.find((c) => c.id === "kbd");
  $("#kbdNote").textContent = kbd && kbd.status !== "ok" ? t(`cap.${kbd.key}.note`) : t("b.kbd_note");
}
/** 语言下拉：跟随系统 + ui/i18n 里的每种语言（用各自的语言写名字） */
function renderLangs() {
  const sel = $("#langSel");
  const opts = [["auto", t("g.lang_auto")], ...INFO.locales.map((l) => [l.code, l.name])];
  sel.replaceChildren(...opts.map(([v, n]) => { const o = h("option", "", n); o.value = v; return o; }));
  if (cfg) sel.value = cfg.general.language;
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
  if (mon && cfg.display.mode === "selected" && !mon.dataset.virtual && !t.closest(".select")) {
    const n = mon.dataset.mon, tg = cfg.display.targets;
    return change("display.targets", tg.includes(n) ? tg.filter((x) => x !== n) : [...tg, n]);
  }
});
document.addEventListener("change", (e) => {
  const t = e.target;
  if (t.matches("select[data-pref]")) {
    const l = t.dataset.pref, prefs = { ...(cfg.display.prefs || {}) };
    if (t.value === "auto") delete prefs[l]; else prefs[l] = t.value;
    // 手动选「断电」：重新尝试，忘掉之前「不响应」的判断
    if (t.value === "off") setK("display.no_ddc", (cfg.display.no_ddc || []).filter((x) => x !== l));
    return change("display.prefs", prefs);
  }
  if (t.matches("select[data-k]")) change(t.dataset.k, t.dataset.type === "num" ? +t.value : t.value);
  else if (t.matches(".field[data-num] input")) {
    const f = t.closest(".field");
    const v = Math.round(+t.value);
    change(f.dataset.num, Number.isFinite(v) ? Math.min(+f.dataset.max, Math.max(+f.dataset.min, v)) : getK(f.dataset.num));
  } else if (t.closest(".tp")) {
    const tp = t.closest(".tp");
    change(tp.dataset.time, `${$(".hh", tp).value}:${$(".mm", tp).value}`);
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
$("#skipBtn").addEventListener("click", async () => { await invoke("skip_tonight"); toast(t("toast.skipped")); });
$("#unskipBtn").addEventListener("click", async () => { await invoke("unskip"); toast(t("toast.unskipped")); });
$("#exportDiag").addEventListener("click", async () => {
  await loadDiag();
  const info = await invoke("get_info");
  // 诊断信息固定用英文，方便贴到 issue 里
  const text = [`BlankRun ${info.version} (${info.os}) · ui=${LANG} · system=${info.system_locale}`, `time ${new Date().toString()}`, "", "[capabilities]",
    ...diag.caps.map((c) => `- ${c.id}: ${c.key} (${c.status}) · ${c.tech}`),
    diag.perms ? `\n[permissions] accessibility=${diag.perms[0]} input_monitoring=${diag.perms[1]}` : "",
    `\n[displays] ${diag.monitors.map((m) => `${m.id} "${m.label}" ${m.width}x${m.height}${m.primary ? " primary" : ""}${m.virtual ? " virtual" : ""} power=${m.power || "-"} how=${m.how}`).join("; ")}`, `\n[last session] ${JSON.stringify(diag.last_session)}`,
    "\n[config]", JSON.stringify(cfg, null, 2)].join("\n");
  try { const p = await invoke("export_diagnostics", { text }); toast(t("toast.exported", { p })); } catch (e) { toast(t("toast.export_fail", { e })); }
});

// 时间选择：小时 00–23，分钟 5 分钟一档
$$(".tp").forEach((tp) => {
  for (let i = 0; i < 24; i++) $(".hh", tp).append(new Option(pad2(i), pad2(i)));
  for (let i = 0; i < 60; i += 5) $(".mm", tp).append(new Option(pad2(i), pad2(i)));
});
listen("nb://tray_hidden", () => { $("#trayHidden").hidden = false; });

(async () => {
  const info = await initPlatform(() => { renderLangs(); render(); renderCaps(); renderHistory(); });
  $("#cfgPath").textContent = info.config_path;
  $("#ver").textContent = info.version;
  [cfg, status] = await Promise.all([invoke("get_config"), invoke("get_status")]);
  renderLangs();
  show();
  render();
  loadDiag();
  listen("nb://status", (s) => { status = s; renderStatus(); drawTrack($("#track"), cfg.schedule.start, cfg.schedule.end, s.now_min); });
  // 外部编辑 config.toml 后同步
  listen("nb://config", async () => { if (saveTimer) return; cfg = await invoke("get_config"); render(); refreshMonitors(); });
  // 窗口重新打开时显示器列表可能已变（插拔、息屏中自动识别出不响应断电的屏）
  addEventListener("focus", () => refreshMonitors());
  listen("nb://nav", (id) => { location.hash = "#" + id; show(id); });
})();
