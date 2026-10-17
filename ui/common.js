// 所有页面共用：Tauri 调用、平台样式、快捷键显示。无框架、无打包。
"use strict";
const T = window.__TAURI__;
const invoke = (cmd, args) => T.core.invoke(cmd, args);
const listen = (ev, cb) => T.event.listen(ev, (e) => cb(e.payload));
const $ = (sel, root = document) => root.querySelector(sel);
const $$ = (sel, root = document) => Array.from(root.querySelectorAll(sel));
const h = (tag, cls, text) => { const e = document.createElement(tag); if (cls) e.className = cls; if (text != null) e.textContent = text; return e; };

let OS = "mac";
async function initPlatform() {
  const info = await invoke("get_info");
  OS = info.os;
  document.body.classList.add(OS === "win" ? "win" : OS === "mac" ? "mac" : "linux");
  return info;
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
  return hh ? (mm ? `${hh} 小时 ${mm} 分` : `${hh} 小时`) : `${mm} 分钟`;
}
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
const DAYS = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];
const DAY_ZH = ["一", "二", "三", "四", "五", "六", "日"];
const WAKE_TEXT = { any_key: "按任意键", mouse_move: "移动鼠标", click: "点击鼠标", password: "按任意键并输入密码", schedule_only: "只在定时结束时" };
