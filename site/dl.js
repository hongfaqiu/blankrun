// 主下载按钮跟随访问者的系统：Mac → 通用版 dmg，Windows → x64 安装包，其他系统显示两个链接
(function () {
  var base = 'https://github.com/hongfaqiu/blankrun/releases/latest/download/';
  var files = { mac: 'BlankRun_universal.dmg', win: 'BlankRun_x64-setup.exe' };
  var ua = navigator.userAgent || '', p = (navigator.userAgentData && navigator.userAgentData.platform) || navigator.platform || '';
  var mobile = /iPhone|iPad|iPod|Android/i.test(ua) || (/Mac/i.test(p) && navigator.maxTouchPoints > 1);
  var os = mobile ? 'any' : /mac/i.test(p) || /Macintosh|Mac OS X/i.test(ua) ? 'mac' : /win/i.test(p) || /Windows/i.test(ua) ? 'win' : 'any';
  document.documentElement.dataset.os = os;
  if (files[os]) document.getElementById('dlMain').href = base + files[os];
})();
