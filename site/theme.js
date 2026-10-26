// 外观：跟随系统 / 浅色 / 深色。放在 <head> 同步执行，避免首屏闪烁（CSP 不允许内联脚本）。
(function () {
  var root = document.documentElement, KEY = 'blankrun-theme';
  var mq = window.matchMedia ? window.matchMedia('(prefers-color-scheme: dark)') : null;
  var pref = 'system';
  try { var s = localStorage.getItem(KEY); if (s === 'light' || s === 'dark') pref = s; } catch (e) {}
  function apply() {
    var dark = pref === 'dark' || (pref === 'system' && mq && mq.matches);
    root.dataset.theme = dark ? 'dark' : 'light';
    root.dataset.themePref = pref;
  }
  apply();
  if (mq && mq.addEventListener) mq.addEventListener('change', apply);
  window.blankrunTheme = {
    get: function () { return pref; },
    set: function (p) {
      pref = p === 'light' || p === 'dark' ? p : 'system';
      try { if (pref === 'system') localStorage.removeItem(KEY); else localStorage.setItem(KEY, pref); } catch (e) {}
      apply();
    }
  };
  document.addEventListener('DOMContentLoaded', function () {
    var seg = document.getElementById('themeSeg');
    if (!seg) return;
    var btns = seg.querySelectorAll('button');
    function paint() { for (var i = 0; i < btns.length; i++) btns[i].setAttribute('aria-checked', String(btns[i].dataset.themePref === pref)); }
    paint();
    seg.addEventListener('click', function (e) {
      var b = e.target.closest('button'); if (!b) return;
      window.blankrunTheme.set(b.dataset.themePref); paint();
    });
  });
})();
