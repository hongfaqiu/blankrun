// 下载按钮：识别访问者的系统，一键直接下载对应安装包（不跳发布页）。
// 版本号与大小先用写在页面里的值；GitHub API 可用时再刷新为最新发布（CSP connect-src 已放行 api.github.com）。
(function () {
  var REPO = 'hongfaqiu/blankrun';
  var BASE = 'https://github.com/' + REPO + '/releases/latest/download/';
  var FILES = { mac: 'BlankRun_universal.dmg', win: 'BlankRun_x64-setup.exe' };
  var info = { ver: '0.1.10', size: { mac: '3.8 MB', win: '1.4 MB' } }; // 发版时改这里（和页面里的 v0.1.10 / 大小）
  var root = document.documentElement;
  var nav = navigator, ua = nav.userAgent || '', uad = nav.userAgentData;
  var plat = (uad && uad.platform) || nav.platform || '';
  var mobile = (uad && uad.mobile) || /iPhone|iPad|iPod|Android/i.test(ua) || (/Mac/i.test(plat) && nav.maxTouchPoints > 1);
  var os = mobile ? 'mobile'
    : /mac/i.test(plat) || /Macintosh|Mac OS X/i.test(ua) ? 'mac'
    : /win/i.test(plat) || /Windows/i.test(ua) ? 'win'
    : /linux|x11|cros/i.test(plat + ' ' + ua) ? 'linux' : 'any';
  var arch = /ARM64|aarch64/i.test(ua) ? 'arm' : '';

  function bi(zh, en) {
    var f = document.createDocumentFragment(), a = document.createElement('span'), b = document.createElement('span');
    a.lang = 'zh'; a.textContent = zh; b.lang = 'en'; b.textContent = en; f.appendChild(a); f.appendChild(b); return f;
  }
  function fill(el, zh, en) { if (!el) return; el.textContent = ''; el.appendChild(bi(zh, en)); }

  function render() {
    root.dataset.os = os;
    var main = document.getElementById('dlMain'), label = document.getElementById('dlLabel'),
      meta = document.getElementById('dlMeta'), note = document.getElementById('dlNote');
    if (!main) return;
    var v = 'v' + info.ver;
    if (os === 'mac' || os === 'win') {
      main.href = BASE + FILES[os];
      main.setAttribute('download', '');
      fill(label, os === 'mac' ? '下载 Mac 版' : '下载 Windows 版', os === 'mac' ? 'Download for Mac' : 'Download for Windows');
      var ver = document.createElement('span'); ver.className = 'num ver'; ver.textContent = ' ' + v; label.appendChild(ver);
      if (os === 'mac') fill(meta, 'macOS 11+ · Apple 芯片与 Intel 通用 · ' + info.size.mac, 'macOS 11+ · Apple silicon and Intel · ' + info.size.mac);
      else fill(meta, 'Windows 10/11 x64 · ' + info.size.win, 'Windows 10/11 x64 · ' + info.size.win);
      if (os === 'win' && arch === 'arm') {
        fill(note, '这台电脑是 ARM 架构。黑着跑暂时只有 x64 安装包，Windows 11 会通过仿真运行它。', 'This PC uses an ARM processor. BlankRun ships an x64 installer for now; Windows 11 runs it under emulation.');
        note.hidden = false;
      }
    } else {
      main.href = '#download';
      main.removeAttribute('download');
      if (os === 'mobile') {
        fill(label, '在电脑上下载', 'Get it on your computer');
        fill(meta, 'macOS 与 Windows 电脑软件 · ' + v, 'Desktop app for macOS and Windows · ' + v);
      } else {
        fill(label, '免费下载', 'Download free');
        fill(meta, 'macOS 与 Windows · ' + v, 'macOS and Windows · ' + v);
        if (os === 'linux') { fill(note, '暂时没有 Linux 版，可以在下方选择 macOS 或 Windows 版。', 'There is no Linux build yet. macOS and Windows builds are below.'); note.hidden = false; }
      }
    }
    var vs = document.querySelectorAll('[data-ver]');
    for (var i = 0; i < vs.length; i++) vs[i].textContent = v;
    ['mac', 'win'].forEach(function (k) { var s = document.querySelector('[data-size="' + k + '"]'); if (s) s.textContent = info.size[k]; });
  }

  function mb(bytes) { return (bytes / 1048576).toFixed(1) + ' MB'; }
  function applyRelease(r) {
    if (!r || typeof r.tag_name !== 'string' || !/^v\d+\.\d+\.\d+$/.test(r.tag_name)) return;
    info.ver = r.tag_name.slice(1);
    (r.assets || []).forEach(function (a) {
      if (a.name === FILES.mac && a.size) info.size.mac = mb(a.size);
      if (a.name === FILES.win && a.size) info.size.win = mb(a.size);
    });
    render();
  }

  render();
  if (os === 'win' && uad && uad.getHighEntropyValues) {
    uad.getHighEntropyValues(['architecture', 'bitness']).then(function (h) {
      if (h && h.architecture === 'arm') { arch = 'arm'; render(); }
    }).catch(function () {});
  }
  try {
    var cached = JSON.parse(sessionStorage.getItem('blankrun-rel') || 'null');
    if (cached) { applyRelease(cached); return; }
  } catch (e) {}
  if (!window.fetch) return;
  var ctl = window.AbortController ? new AbortController() : null;
  if (ctl) setTimeout(function () { ctl.abort(); }, 5000);
  fetch('https://api.github.com/repos/' + REPO + '/releases/latest', { headers: { Accept: 'application/vnd.github+json' }, signal: ctl ? ctl.signal : undefined })
    .then(function (res) { return res.ok ? res.json() : null; })
    .then(function (r) {
      if (!r) return;
      var slim = { tag_name: r.tag_name, assets: (r.assets || []).map(function (a) { return { name: a.name, size: a.size }; }) };
      try { sessionStorage.setItem('blankrun-rel', JSON.stringify(slim)); } catch (e) {}
      applyRelease(slim);
    })
    .catch(function () {});
})();
