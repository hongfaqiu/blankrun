(function () {
  var root = document.documentElement, KEY = 'blankrun-lang';
  function set(l) {
    root.dataset.lang = l; root.lang = l === 'en' ? 'en' : 'zh-CN';
    document.title = l === 'en' ? 'BlankRun · Screens off, agents on' : '息屏跑 BlankRun · 夜里息屏，AI 接着跑';
  }
  var q = new URLSearchParams(location.search).get('lang');
  var saved = null; try { saved = localStorage.getItem(KEY); } catch (e) {}
  set(q === 'en' || q === 'zh' ? q : saved || (/^zh/i.test(navigator.language || '') ? 'zh' : 'en'));
  document.getElementById('langBtn').addEventListener('click', function () {
    var l = root.dataset.lang === 'en' ? 'zh' : 'en'; set(l);
    try { localStorage.setItem(KEY, l); } catch (e) {}
  });
})();
