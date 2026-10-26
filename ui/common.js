// 所有页面共用：Tauri 调用、平台样式、多语言、快捷键显示。无框架、无打包。
"use strict";
const T = window.__TAURI__;
const invoke = (cmd, args) => T.core.invoke(cmd, args);
const listen = (ev, cb) => T.event.listen(ev, (e) => cb(e.payload));
const $ = (sel, root = document) => root.querySelector(sel);
const $$ = (sel, root = document) => Array.from(root.querySelectorAll(sel));
const h = (tag, cls, text) => { const e = document.createElement(tag); if (cls) e.className = cls; if (text != null) e.textContent = text; return e; };

let OS = "mac", INFO = null;
/** 平台样式 + 按配置加载界面语言。onLang 在之后每次切换语言时调用（不用重启） */
async function initPlatform(onLang) {
  // 配置里声明的窗口可能在引擎状态注册之前就加载完页面（Windows 上很常见），这时命令会报错：稍等重试
  let info, c;
  for (let i = 0; ; i++) {
    try { [info, c] = await Promise.all([invoke("get_info"), invoke("get_config")]); break; }
    catch (e) { if (i >= 100) throw e; await new Promise((r) => setTimeout(r, 100)); }
  }
  INFO = info; OS = info.os;
  document.body.classList.add(OS === "win" ? "win" : OS === "mac" ? "mac" : "linux");
  setTheme(c.general.theme);
  await setLocale(c.general.language);
  listen("nb://config", async () => { const c2 = await invoke("get_config"); setTheme(c2.general.theme); if (await setLocale(c2.general.language) && onLang) onLang(); });
  return info;
}

/** 外观：theme.js 已在 <head> 里按上次的设置画好，这里按配置校正（设置页改了会实时切换所有窗口） */
function setTheme(p) { if (window.nbSetTheme) window.nbSetTheme(p); }

// ---------- 多语言 ----------
// 词条在 ui/i18n/<代码>.js，每个文件调用一次 NB_LOCALE(代码, {...})。新增语言只要加一个文件（build.rs 会扫描到）。
// 词条可以按平台细分：先找 "key.mac" / "key.win"，再找 "key"，最后 "key.win"；值也可以是 { one, other } 复数形式。
// 找不到的词条回落到英文，再回落到 key 本身。
const LOCALES = {};
window.NB_LOCALE = (code, dict) => { LOCALES[code] = dict; };
let LANG = "en";
function loadLocale(code) {
  if (LOCALES[code]) return Promise.resolve();
  return new Promise((ok) => { const s = document.createElement("script"); s.src = `i18n/${code}.js`; s.onload = s.onerror = () => ok(); document.head.append(s); });
}
/** 与 crates/core/src/locale.rs 的 resolve 规则一致 */
function pickLocale(pref, codes) {
  const lc = codes.map((c) => c.toLowerCase());
  const one = (want) => {
    const w = (want || "").replace(/_/g, "-").toLowerCase(); if (!w) return null;
    let i = lc.indexOf(w); if (i >= 0) return codes[i];
    const main = w.split("-")[0];
    if (main === "zh") { i = lc.indexOf(/hant|-(tw|hk|mo)$/.test(w) ? "zh-tw" : "zh-cn"); if (i >= 0) return codes[i]; }
    i = lc.findIndex((c) => c.split("-")[0] === main); return i >= 0 ? codes[i] : null;
  };
  const sys = [INFO && INFO.system_locale, ...(navigator.languages || [navigator.language])];
  const order = pref && pref !== "auto" ? [pref, ...sys] : sys;
  for (const w of order) { const c = one(w); if (c) return c; }
  return codes.includes("en") ? "en" : codes[0] || "en";
}
/** 切到配置里的语言；语言真的变了返回 true */
async function setLocale(pref) {
  const codes = (INFO && INFO.locales || []).map((l) => l.code);
  const code = pickLocale(pref, codes.length ? codes : ["en"]);
  const changed = code !== LANG || !LOCALES[code];
  if (!changed) return false;
  await Promise.all([loadLocale("en"), loadLocale(code)]);
  LANG = code;
  document.documentElement.lang = code;
  applyI18n();
  return true;
}
function lookup(key) {
  for (const d of [LOCALES[LANG], LOCALES.en]) {
    if (!d) continue;
    for (const k of [`${key}.${OS}`, key, `${key}.win`]) if (d[k] != null) return d[k];
  }
  return null;
}
/** t("p.min", { n: 5 }) → "5 分钟"；{n} 存在时自动选复数形式 */
function t(key, vars) {
  let v = lookup(key);
  if (v == null) return key;
  if (typeof v === "object") v = v[vars && vars.n != null ? new Intl.PluralRules(LANG).select(vars.n) : "other"] || v.other || "";
  return vars ? v.replace(/\{(\w+)\}/g, (m, k) => (vars[k] != null ? vars[k] : m)) : v;
}
/** 把页面上 data-i18n（文字）、data-i18n-aria（aria-label）的元素换成当前语言；data-n 作为 {n} */
function applyI18n(root = document) {
  $$("[data-i18n]", root).forEach((e) => { e.textContent = t(e.dataset.i18n, e.dataset.n != null ? { n: +e.dataset.n } : undefined); });
  $$("[data-i18n-aria]", root).forEach((e) => e.setAttribute("aria-label", t(e.dataset.i18nAria)));
}

const MAC_SYM = { CTRL: "⌃", CONTROL: "⌃", ALT: "⌥", OPTION: "⌥", SHIFT: "⇧", CMD: "⌘", COMMAND: "⌘", META: "⌘", WIN: "⌘", SUPER: "⌘", ESC: "esc", ESCAPE: "esc", ENTER: "↩", SPACE: "space", TAB: "⇥", BACKSPACE: "⌫" };
const WIN_NAME = { CTRL: "Ctrl", CONTROL: "Ctrl", ALT: "Alt", OPTION: "Alt", SHIFT: "Shift", CMD: "Win", COMMAND: "Win", META: "Win", WIN: "Win", SUPER: "Win", ESC: "Esc", ESCAPE: "Esc" };
/** "Ctrl+Alt+W" → ["⌃","⌥","W"]（Mac）或 ["Ctrl","Alt","W"] */
function keyParts(combo) {
  return (combo || "").split("+").map((p) => p.trim()).filter(Boolean).map((p) => {
    const u = p.toUpperCase();
    if (OS === "mac") return MAC_SYM[u] || (p.length === 1 ? u : p);
    return WIN_NAME[u] || (p.length === 1 ? u : p);
  });
}
/** 把快捷键渲染成一组 .kbd（或遮罩上的 .k） */
function renderKeys(el, combo, cls = "kbd") {
  el.replaceChildren(...keyParts(combo).map((k) => h("span", cls, k)));
}
/** 键盘事件 → "Ctrl+Alt+W"；只按了修饰键时返回 null */
function comboFromEvent(e) {
  const k = e.key;
  if (["Control", "Alt", "Shift", "Meta", "OS"].includes(k)) return null;
  let main;
  if (/^Key[A-Z]$/.test(e.code)) main = e.code.slice(3);
  else if (/^Digit[0-9]$/.test(e.code)) main = e.code.slice(5);
  else if (/^F([1-9]|1[0-2])$/.test(k)) main = k;
  else main = { Escape: "Esc", " ": "Space", Enter: "Enter", Tab: "Tab", Backspace: "Backspace" }[k];
  if (!main) return null;
  const mods = [];
  if (e.ctrlKey) mods.push("Ctrl");
  if (e.altKey) mods.push("Alt");
  if (e.shiftKey) mods.push("Shift");
  if (e.metaKey) mods.push(OS === "mac" ? "Cmd" : "Win");
  return mods.length ? [...mods, main].join("+") : null;
}

const pad2 = (n) => String(n).padStart(2, "0");
const parseHM = (s) => { const m = /^(\d{1,2}):(\d{2})$/.exec(s || ""); return m ? (+m[1]) * 60 + (+m[2]) : null; };
function fmtDur(min) {
  const hh = Math.floor(min / 60), mm = min % 60;
  return hh ? (mm ? t("dur.hm", { h: hh, m: mm }) : t("dur.h", { h: hh, n: hh })) : t("dur.m", { m: mm, n: mm });
}
/** 1000 → "1"，1500 → "1.5"（秒） */
const secs = (ms) => (ms / 1000).toFixed(1).replace(/\.0$/, "");
/** 在 24 小时轨道上画出时段（跨午夜拆成两段）和当前时刻 */
function drawTrack(track, start, end, nowMin) {
  track.replaceChildren();
  const s = parseHM(start), e = parseHM(end);
  if (s != null && e != null) {
    const seg = (a, b, cls) => { const b0 = h("span", "band " + (cls || "")); b0.style.left = (a / 14.4) + "%"; b0.style.width = ((b - a) / 14.4) + "%"; track.append(b0); };
    if (s < e) seg(s, e, "solo"); else { seg(s, 1440, "tail"); seg(0, e, "head"); }
  }
  const n = h("span", "now"); n.style.left = (nowMin / 14.4) + "%"; track.append(n);
}
/** 画一天里的多段时段：segs = [[开始分钟, 结束分钟], …]（status.schedule.today，已合并） */
function drawSegs(track, segs, nowMin) {
  track.replaceChildren();
  for (const [a, b] of segs || []) {
    if (b <= a) continue;
    const cls = a === 0 && b < 1440 ? "head" : b === 1440 && a > 0 ? "tail" : "solo";
    const el = h("span", "band " + cls); el.style.left = (a / 14.4) + "%"; el.style.width = ((b - a) / 14.4) + "%"; track.append(el);
  }
  const n = h("span", "now"); n.style.left = (nowMin / 14.4) + "%"; track.append(n);
}
const DAYS = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];
