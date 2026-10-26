// 外观（跟随系统 / 浅色 / 深色）：放在 <head> 里同步执行，页面画出来之前就设好 <html data-theme>，不闪。
// 设置值记在 localStorage（各窗口同源共享）；common.js 读到配置后再校正一次，并在设置变化时实时切换。
"use strict";
(() => {
  const dark = window.matchMedia("(prefers-color-scheme: dark)");
  let pref = "system";
  try { pref = localStorage.getItem("nb.theme") || "system"; } catch (e) { /* 存储不可用：跟随系统 */ }
  const apply = () => { document.documentElement.dataset.theme = pref === "light" || pref === "dark" ? pref : dark.matches ? "dark" : "light"; };
  /** 设置外观：system | light | dark */
  window.nbSetTheme = (p) => {
    pref = p === "light" || p === "dark" ? p : "system";
    try { localStorage.setItem("nb.theme", pref); } catch (e) { /* 忽略 */ }
    apply();
  };
  dark.addEventListener("change", apply);
  apply();
})();
