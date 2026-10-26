// 黑着跑官网互动演示：HTML 复刻菜单栏面板和两块屏，访客可以直接点；看着不动时由假指针自动演示。
// 自动演示的调度方式参考 Strata Manager 官网（strata-manager.cosfoc.com）：只在可见、未悬停、未被访客接管时播放，
// 访客一操作就停下，安静一段时间后再继续；prefers-reduced-motion 时不自动播放、不做过渡。
(function () {
  'use strict';
  var $ = function (s, r) { return (r || document).querySelector(s); };
  var $$ = function (s, r) { return Array.prototype.slice.call((r || document).querySelectorAll(s)); };
  var root = document.documentElement;
  var stage = $('#stage');
  if (!stage) return;
  var reduce = window.matchMedia('(prefers-reduced-motion: reduce)');
  var motionOK = function () { return !reduce.matches; };

  // ---------- 文案 ----------
  var T = {
    zh: {
      pause: '暂停演示', resume: '继续演示', delay: '延迟', now: '立即', m1: '1 分钟', m5: '5 分钟', m15: '15 分钟',
      scope: '范围', all: '全部显示器', sel: '只关所选', d_in: '内建屏', d_ext: '外接显示器', disc_tag: '断开',
      keep: '键鼠照常用', keep_n: '只关所选的屏，键盘鼠标继续在亮着的屏上用', wake_by: '唤醒方式', wake: '唤醒',
      w_hold: '按住快捷键', w_key: '按任意键', w_move: '移动鼠标', w_click: '点击鼠标', w_sched: '只在定时结束时',
      st_idle: '待命', st_count: '倒计时中 · 已开始防睡眠', st_blank: '息屏中 · 防睡眠', st_part: '只关所选 · 防睡眠', st_plan: '按计划息屏中',
      aw_idle: '待命 · 未在防睡眠', aw_blank: '屏幕黑着，电脑醒着', aw_part: '内建屏已关，外接屏照常', aw_plan: '计划中 · 电脑醒着',
      bb: '立即息屏', bb_mac: '关掉 {n} 台显示器和键盘背光，电脑保持运行', bb_win: '关掉 {n} 台显示器，电脑保持运行',
      bb_sel: '只关所选的 {n} 台，电脑保持运行', bb_none: '先选要关的屏', bb_back: '恢复亮屏', bb_back_s: '在菜单栏点一下也能恢复',
      bb_cd: '后息屏 · 点按取消',
      h_hold: '按住 {k} 1 秒唤醒', h_key: '按任意键唤醒', h_move: '移动鼠标唤醒', h_click: '点击鼠标唤醒', h_sched: '{t} 自动唤醒 · 紧急恢复',
      do_key: '在键盘上按任意键', do_move: '把鼠标移到屏幕上', do_click: '点一下黑屏', do_sched: '等计划结束', em: '紧急 {k}',
      disc_mac: '已断开 · 像合盖一样断电', disc_win: 'DDC/CI 断电',
      plan_a: '工作日', plan_b: '周末', days: ['日', '一', '二', '三', '四', '五', '六'],
      desc: {
        blank: '点黑色按钮，两块屏和键盘背光一起关掉；下方的任务和时钟照常走。黑屏时点一下屏幕，只会亮一行提示。按住唤醒键 1 秒恢复，也可以用真键盘按住 {k}。',
        select: 'Mac mini 模式：只关所选的屏。点屏幕选择要关哪块，再点息屏。外接屏照常亮，键盘、鼠标、触控板照常用，光标不会跑进关掉的屏。MacBook 内建屏像合盖一样断电。',
        wake: '选一种唤醒方式，息屏后在演示里试：移动鼠标、点击、按键都会被拦下，只有选中的方式能唤醒。紧急恢复键任何时候都有效。',
        select_win: '只关所选的屏：点屏幕选择要关哪块，再点息屏。没选的屏照常亮，键盘和鼠标照常用。支持 DDC/CI 的显示器单独断电，不支持的盖黑色遮罩。',
        blank_win: '点黑色按钮，两块屏一起关掉；下方的任务和时钟照常走。黑屏时点一下屏幕，只会亮一行提示。按住唤醒键 1 秒恢复，也可以用真键盘按住 {k}。',
        sched: '可以设好几组定时，每组有自己的时间和星期，也能单独开关。拖动时间条、换一天，看屏幕什么时候黑。跨午夜的时段按开始那天算。'
      }
    },
    en: {
      pause: 'Pause demo', resume: 'Resume demo', delay: 'Delay', now: 'Now', m1: '1 min', m5: '5 min', m15: '15 min',
      scope: 'Scope', all: 'All displays', sel: 'Selected only', d_in: 'Built-in display', d_ext: 'External display', disc_tag: 'disconnect',
      keep: 'Keep keyboard & mouse', keep_n: 'Turn off only the selected screens and keep working on the others', wake_by: 'Wake by', wake: 'Wake',
      w_hold: 'Holding a shortcut', w_key: 'Any key', w_move: 'Moving the mouse', w_click: 'Clicking', w_sched: 'Only when the schedule ends',
      st_idle: 'Standing by', st_count: 'Counting down · sleep blocked', st_blank: 'Blank · sleep blocked', st_part: 'Selected only · sleep blocked', st_plan: 'Blank on schedule',
      aw_idle: 'Standing by · sleep not blocked', aw_blank: 'Screens dark, computer awake', aw_part: 'Built-in off, external on', aw_plan: 'Scheduled · computer awake',
      bb: 'Blank now', bb_mac: 'Turns off {n} displays and the keyboard backlight; the computer keeps running', bb_win: 'Turns off {n} displays; the computer keeps running',
      bb_sel: 'Turns off the {n} selected; the computer keeps running', bb_none: 'Pick a screen first', bb_back: 'Restore', bb_back_s: 'The menu bar icon restores too',
      bb_cd: 'until blank · click to cancel',
      h_hold: 'Hold {k} for 1 second to wake', h_key: 'Press any key to wake', h_move: 'Move the mouse to wake', h_click: 'Click to wake', h_sched: 'Wakes at {t} · emergency restore',
      do_key: 'Press any key', do_move: 'Move the mouse over a screen', do_click: 'Click the dark screen', do_sched: 'Wait for the schedule', em: 'Emergency {k}',
      disc_mac: 'Disconnected · powered down like a closed lid', disc_win: 'Powered off via DDC/CI',
      plan_a: 'Weekdays', plan_b: 'Weekends', days: ['S', 'M', 'T', 'W', 'T', 'F', 'S'],
      desc: {
        blank: 'Click the black button: both screens and the keyboard backlight go off while the job and the clock below keep going. Touch a dark screen and only a one-line hint appears. Hold the wake keys for a second to restore, or hold {k} on your real keyboard.',
        select: 'Mac mini mode: turn off only the selected screens. Click a screen to choose it, then blank. The external display stays on and the keyboard, mouse and trackpad keep working; the cursor stays off the dark screen. The MacBook display powers down like a closed lid.',
        wake: 'Pick a wake method, blank, then try it here. Mouse moves, clicks and keys are swallowed; only the chosen method wakes. The emergency key always works.',
        select_win: 'Turn off only the selected screens: click a screen to choose it, then blank. The others stay on and the keyboard and mouse keep working. Displays that support DDC/CI power off on their own; others get a black overlay.',
        blank_win: 'Click the black button: both screens go off while the job and the clock below keep going. Touch a dark screen and only a one-line hint appears. Hold the wake keys for a second to restore, or hold {k} on your real keyboard.',
        sched: 'Add several schedules, each with its own times and weekdays, and switch each on or off. Drag the time bar or pick another day to see when the screens go dark. Overnight slots count from the day they start.'
      }
    }
  };
  var lang = function () { return root.dataset.lang === 'en' ? 'en' : 'zh'; };
  var t = function (k, vars) {
    var s = T[lang()][k]; if (s == null) s = k;
    if (vars) Object.keys(vars).forEach(function (v) { s = s.replace('{' + v + '}', vars[v]); });
    return s;
  };

  // ---------- 状态 ----------
  var S = {
    tab: 'blank', plat: root.dataset.os === 'win' ? 'win' : 'mac',
    off: { ext: false, in: false }, countdown: 0, delay: 0,
    sel: { in: true, ext: false }, scope: 'sel', keep: true, wake: 'hold',
    plans: [{ s: 1410, e: 450, days: [1, 2, 3, 4, 5], on: true, name: 'plan_a' }, { s: 60, e: 600, days: [6, 0], on: true, name: 'plan_b' }],
    day: 1, minute: 1350, clock: 1410, job: 18, jobIdx: 0
  };
  var disp = { ext: $('#dispExt'), in: $('#dispIn') };
  var pop = $('#pop'), tabs = $$('.tab'), pointerEl = $('#pointer');
  var keysFor = function () { return S.plat === 'mac' ? ['⌃', '⌥', 'W'] : ['Ctrl', 'Alt', 'W']; };
  var emFor = function () { return S.plat === 'mac' ? '⌃⌥⇧esc' : 'Ctrl+Alt+Shift+Esc'; };
  var fmt = function (m) { m = ((m % 1440) + 1440) % 1440; return ('0' + Math.floor(m / 60)).slice(-2) + ':' + ('0' + Math.floor(m % 60)).slice(-2); };
  var anyOff = function () { return S.off.ext || S.off.in; };

  // ---------- 场景缩放 ----------
  var fit = $('#sceneFit'), scene = $('#scene');
  function fitScene() {
    var w = fit.clientWidth; if (!w) return;
    var k = Math.min(1.15, w / 760);
    scene.style.transform = 'scale(' + k + ')';
    fit.style.height = Math.ceil(420 * k) + 'px';
  }
  if (window.ResizeObserver) new ResizeObserver(fitScene).observe(fit); else window.addEventListener('resize', fitScene);
  fitScene();

  // ---------- 终端日志（黑屏时照样在跑） ----------
  var LOG = {
    ext: ['› reading src/auth/session.ts', '› editing src/auth/token.ts  +38 −12', '› pnpm test', '✓ 214 passed · 0 failed', '› editing src/api/login.ts  +21 −4', '› pnpm build', '✓ build ok in 41s', '› writing migrations/0042_sessions.sql', '› git commit -m "refactor: split session store"', '✓ committed 3f9c2e1', '› next task: rate limiter'],
    in: ['codex · task 3/12', '› downloading dataset  2.1/4.0 GB', '› tokenizing shard 17', '✓ shard 17 done', '› eval batch 4', '✓ acc 0.912', '› task 4/12']
  };
  var JOBS = ['agent · refactor auth', 'agent · add rate limiter', 'agent · write e2e tests'];
  var logPos = { ext: 0, in: 0 };
  function pushLog(which) {
    var body = $('[data-log="' + which + '"] .term-b'); if (!body) return;
    var line = LOG[which][logPos[which]++ % LOG[which].length];
    var d = document.createElement('div'); d.textContent = line;
    d.className = line.charAt(0) === '✓' ? 'ok' : (line.charAt(0) === '›' ? '' : 'dim');
    body.appendChild(d);
    while (body.children.length > (which === 'ext' ? 10 : 6)) body.removeChild(body.firstChild);
  }
  for (var i = 0; i < 8; i++) pushLog('ext');
  for (i = 0; i < 5; i++) pushLog('in');

  // ---------- 渲染 ----------
  function hintText() {
    if (S.tab === 'sched') return t('h_sched', { t: fmt(planEnd() || 450) });
    var m = S.tab === 'wake' ? S.wake : 'hold';
    if (m === 'hold') return t('h_hold', { k: keysFor().join('') });
    if (m === 'sched') return t('h_sched', { t: '07:30' });
    return t('h_' + m);
  }
  function targets() {
    if (S.tab === 'select' && S.scope === 'sel') return { ext: S.sel.ext, in: S.sel.in };
    return { ext: true, in: true };
  }
  function render() {
    stage.dataset.tab = S.tab; stage.dataset.plat = S.plat;
    stage.setAttribute('aria-labelledby', 'tab-' + S.tab);
    tabs.forEach(function (b) { var on = b.dataset.tab === S.tab; b.setAttribute('aria-selected', on); b.tabIndex = on ? 0 : -1; });
    $$('[data-plat]', $('.plat')).forEach(function (b) { b.setAttribute('aria-checked', b.dataset.plat === S.plat); });
    var D = T[lang()].desc; $('#tabDesc').textContent = (S.plat === 'win' && D[S.tab + '_win'] ? D[S.tab + '_win'] : D[S.tab]).replace('{k}', keysFor().join(' '));
    $$('[data-t]').forEach(function (el) { el.textContent = t(el.dataset.t); });
    $$('.pane, .wakebar, .delay').forEach(function (p) { p.hidden = (p.dataset.pane || '').split(' ').indexOf(S.tab) < 0; });

    ['ext', 'in'].forEach(function (k) {
      var d = disp[k];
      d.classList.toggle('is-off', S.off[k]);
      d.classList.toggle('is-picked', S.tab === 'select' && S.scope === 'sel' && S.sel[k] && !S.off[k]);
      $('.hint', d).textContent = hintText();
    });
    var disc = $('.disc', disp.in); disc.textContent = S.tab === 'select' ? t(S.plat === 'mac' ? 'disc_mac' : 'disc_win') : '';
    var part = S.off.in !== S.off.ext;
    disp.ext.classList.toggle('show-cursor', part && S.keep && !S.off.ext);
    $('.kb', disp.in).style.opacity = (S.plat === 'mac' && !S.off.in && root.dataset.theme === 'dark') ? '.55' : '0';

    // 面板
    var bt = $('#bbTitle'), bs = $('#bbSub');
    var n = targets(); var cnt = (n.ext ? 1 : 0) + (n.in ? 1 : 0);
    if (S.countdown > 0) { bt.className = 'bb-t num'; var simS = Math.round(S.countdown / S.delay * ({ 5: 1, 10: 5, 15: 15 }[S.delay] || 1) * 60); bt.textContent = Math.floor(simS / 60) + ':' + ('0' + simS % 60).slice(-2); bs.textContent = t('bb_cd'); }
    else if (anyOff()) { bt.className = 'bb-t'; bt.textContent = t('bb_back'); bs.textContent = t('bb_back_s'); }
    else {
      bt.className = 'bb-t'; bt.textContent = t('bb');
      bs.textContent = cnt === 0 ? t('bb_none') : (S.tab === 'select' && S.scope === 'sel') ? t('bb_sel', { n: cnt }) : t(S.plat === 'mac' ? 'bb_mac' : 'bb_win', { n: cnt });
    }
    $('#blankBtn .led').style.opacity = (S.countdown > 0 || anyOff()) ? '1' : '.35';
    $$('#delaySeg button').forEach(function (b) { b.setAttribute('aria-checked', Number(b.dataset.delay) === S.delay); });
    $$('#scopeSeg button').forEach(function (b) { b.setAttribute('aria-checked', b.dataset.scope === S.scope); });
    $$('[data-sel]').forEach(function (c) { c.checked = S.sel[c.dataset.sel]; c.disabled = S.scope !== 'sel'; });
    var kt = $('#keepTog'); kt.classList.toggle('on', S.keep); kt.setAttribute('aria-checked', S.keep);
    $$('#wakeList button').forEach(function (b) { b.textContent = t('w_' + b.dataset.wake); b.setAttribute('aria-checked', b.dataset.wake === S.wake); });

    var planLive = S.tab === 'sched' && planActive();
    var st = S.countdown > 0 ? 'st_count' : planLive ? 'st_plan' : !anyOff() ? 'st_idle' : (S.off.ext && S.off.in) ? 'st_blank' : 'st_part';
    var ps = $('#popState'); ps.textContent = '';
    var dot = document.createElement('i'); dot.className = 'dot' + (st === 'st_idle' ? '' : ' on'); ps.appendChild(dot); ps.appendChild(document.createTextNode(t(st)));
    var aw = st === 'st_idle' ? 'aw_idle' : st === 'st_count' ? 'aw_idle' : st === 'st_plan' ? 'aw_plan' : st === 'st_blank' ? 'aw_blank' : 'aw_part';
    if (st === 'st_count') aw = 'st_count';
    $('#awakeTxt').textContent = t(aw);
    $('#awakeDot').className = 'dot' + (st === 'st_idle' ? '' : ' on');

    // 唤醒栏
    var hk = $('#holdKeys'); hk.textContent = '';
    var hb = $('#holdBtn');
    var m = S.tab === 'wake' ? S.wake : 'hold';
    if (m === 'hold') keysFor().forEach(function (k) { var e = document.createElement('span'); e.className = 'kbd'; e.textContent = k; hk.appendChild(e); });
    else { var e = document.createElement('span'); e.className = 'do'; e.textContent = t('do_' + m); hk.appendChild(e); }
    hb.setAttribute('aria-label', m === 'hold' ? t('h_hold', { k: keysFor().join('') }) : t('do_' + m));
    hb.dataset.mode = m;
    var em = $('#emKeys'); em.textContent = '';
    var eb = document.createElement('button'); eb.type = 'button'; eb.id = 'emBtn'; eb.textContent = t('em', { k: emFor() }); em.appendChild(eb);

    renderPlans();
    paintClock(); paintJob();
    $('#demoToggle').querySelector('span').textContent = t(runner.paused ? 'resume' : 'pause');
    $('#demoToggle').setAttribute('aria-pressed', runner.paused);
  }
  function paintClock() {
    var v = fmt(S.tab === 'sched' ? S.minute : S.clock);
    $$('[data-clock]').forEach(function (c) { c.textContent = v; });
  }
  function paintJob() {
    $('#jobBar').style.width = S.job.toFixed(1) + '%';
    $('#jobPct').textContent = Math.floor(S.job) + '%';
    $('#jobName').textContent = JOBS[S.jobIdx % JOBS.length];
  }

  // ---------- 定时 ----------
  function covers(p, day, min) {
    if (!p.on) return false;
    var prev = (day + 6) % 7;
    if (p.s < p.e) return p.days.indexOf(day) >= 0 && min >= p.s && min < p.e;
    return (p.days.indexOf(day) >= 0 && min >= p.s) || (p.days.indexOf(prev) >= 0 && min < p.e);
  }
  function livePlan() { for (var j = 0; j < S.plans.length; j++) if (covers(S.plans[j], S.day, S.minute)) return S.plans[j]; return null; }
  function planActive() { return !!livePlan(); }
  function planEnd() { var p = livePlan(); return p ? p.e : 0; }
  function renderPlans() {
    var box = $('#plans'); box.textContent = '';
    var L = T[lang()];
    S.plans.forEach(function (p, idx) {
      var row = document.createElement('div'); row.className = 'plan' + (p.on ? '' : ' off') + (S.tab === 'sched' && covers(p, S.day, S.minute) ? ' live' : '');
      var pt = document.createElement('div'); pt.className = 'pt';
      var nm = document.createElement('small'); nm.textContent = L[p.name]; pt.appendChild(nm);
      pt.appendChild(document.createTextNode(fmt(p.s))); var ar = document.createElement('i'); ar.textContent = '→'; pt.appendChild(ar); pt.appendChild(document.createTextNode(fmt(p.e)));
      var tg = document.createElement('button'); tg.type = 'button'; tg.className = 'tog' + (p.on ? ' on' : ''); tg.setAttribute('role', 'switch'); tg.setAttribute('aria-checked', p.on);
      tg.setAttribute('aria-label', L[p.name]); tg.dataset.plan = idx;
      var pd = document.createElement('div'); pd.className = 'pd'; pd.setAttribute('aria-label', L[p.name]);
      [1, 2, 3, 4, 5, 6, 0].forEach(function (d) {
        var b = document.createElement('button'); b.type = 'button'; b.textContent = L.days[d]; b.dataset.plan = idx; b.dataset.pday = d;
        b.setAttribute('aria-pressed', p.days.indexOf(d) >= 0); pd.appendChild(b);
      });
      row.appendChild(pt); row.appendChild(tg); row.appendChild(pd); box.appendChild(row);
    });
    var ds = $('#daySeg'); ds.textContent = '';
    [1, 2, 3, 4, 5, 6, 0].forEach(function (d) {
      var b = document.createElement('button'); b.type = 'button'; b.setAttribute('role', 'radio'); b.textContent = L.days[d]; b.dataset.day = d;
      b.setAttribute('aria-checked', d === S.day); ds.appendChild(b);
    });
    var r = $('#timeRange'); r.value = S.minute; r.setAttribute('aria-valuetext', fmt(S.minute));
    r.setAttribute('aria-label', lang() === 'en' ? 'Time of day' : '时间');
  }
  function applySchedule() {
    var on = planActive();
    S.off.ext = on; S.off.in = on;
    render();
  }

  // ---------- 动作 ----------
  var cdTimer = null, hintTimer = {};
  function blankNow() {
    var n = targets(); if (!n.ext && !n.in) return;
    S.off.ext = n.ext; S.off.in = n.in; S.countdown = 0; clearInterval(cdTimer);
    if (S.clock < 1380 && S.clock > 420) S.clock = 1410;
    render();
  }
  function wakeAll() {
    S.off.ext = false; S.off.in = false; S.countdown = 0; clearInterval(cdTimer);
    ['ext', 'in'].forEach(function (k) { disp[k].classList.remove('show-hint'); });
    render();
  }
  function startBlank() {
    if (anyOff()) { wakeAll(); return; }
    if (S.countdown > 0) { S.countdown = 0; clearInterval(cdTimer); render(); return; }
    if (!S.delay || S.tab !== 'blank') { blankNow(); return; }
    S.countdown = S.delay; render();
    cdTimer = setInterval(function () { S.countdown -= 1; if (S.countdown <= 0) blankNow(); else render(); }, 1000);
  }
  function showHint(k) {
    var keys = k ? [k] : ['ext', 'in'];
    keys.forEach(function (key) {
      if (!S.off[key]) return;
      disp[key].classList.add('show-hint');
      clearTimeout(hintTimer[key]);
      hintTimer[key] = setTimeout(function () { disp[key].classList.remove('show-hint'); }, 3000);
    });
  }
  function tryWake(kind, where) {
    if (!anyOff()) return;
    if (kind === 'emergency') { wakeAll(); return; }
    if (S.tab === 'sched') { showHint(where); return; }
    var m = S.tab === 'wake' ? S.wake : 'hold';
    if (S.tab === 'select' && S.keep && (kind === 'move' || kind === 'click' || kind === 'key')) return; // 键鼠照常用，不唤醒
    if (kind === m) wakeAll(); else showHint(where);
  }

  // ---------- 访客操作 ----------
  tabs.forEach(function (b, idx) {
    b.addEventListener('click', function () { selectTab(b.dataset.tab); });
    b.addEventListener('keydown', function (e) {
      var d = e.key === 'ArrowRight' ? 1 : e.key === 'ArrowLeft' ? -1 : 0; if (!d) return;
      var nb = tabs[(idx + d + tabs.length) % tabs.length]; nb.focus(); selectTab(nb.dataset.tab); e.preventDefault();
    });
  });
  function selectTab(tab) {
    if (S.tab === tab) return;
    S.tab = tab; wakeAll();
    if (tab === 'sched') applySchedule(); else render();
  }
  $('.plat').addEventListener('click', function (e) { var b = e.target.closest('button'); if (!b) return; S.plat = b.dataset.plat; render(); });
  $('#blankBtn').addEventListener('click', startBlank);
  $('#delaySeg').addEventListener('click', function (e) { var b = e.target.closest('button'); if (!b) return; S.delay = Number(b.dataset.delay); render(); });
  $('#scopeSeg').addEventListener('click', function (e) { var b = e.target.closest('button'); if (!b || anyOff()) return; S.scope = b.dataset.scope; render(); });
  $$('[data-sel]').forEach(function (c) { c.addEventListener('change', function () { if (anyOff()) { c.checked = S.sel[c.dataset.sel]; return; } S.sel[c.dataset.sel] = c.checked; render(); }); });
  $('#keepTog').addEventListener('click', function () { S.keep = !S.keep; render(); });
  $('#wakeList').addEventListener('click', function (e) { var b = e.target.closest('button'); if (!b) return; S.wake = b.dataset.wake; wakeAll(); });
  $('#emKeys').addEventListener('click', function (e) { if (e.target.closest('button')) tryWake('emergency'); });
  $('#plans').addEventListener('click', function (e) {
    var b = e.target.closest('button'); if (!b) return;
    var p = S.plans[Number(b.dataset.plan)];
    if (b.dataset.pday != null) { var d = Number(b.dataset.pday), at = p.days.indexOf(d); if (at >= 0) p.days.splice(at, 1); else p.days.push(d); }
    else p.on = !p.on;
    applySchedule();
  });
  $('#daySeg').addEventListener('click', function (e) { var b = e.target.closest('button'); if (!b) return; S.day = Number(b.dataset.day); applySchedule(); });
  $('#timeRange').addEventListener('input', function (e) { S.minute = Number(e.target.value); applySchedule(); });

  ['ext', 'in'].forEach(function (k) {
    var scr = $('.scr', disp[k]);
    scr.addEventListener('click', function () {
      if (S.off[k]) { tryWake('click', k); return; }
      if (S.tab === 'select' && S.scope === 'sel' && !anyOff()) { S.sel[k] = !S.sel[k]; render(); }
    });
    var lastMove = 0;
    scr.addEventListener('pointermove', function (e) {
      if (e.pointerType !== 'mouse' || !S.off[k]) return;
      var now = Date.now(); if (now - lastMove < 400) return; lastMove = now;
      tryWake('move', k);
    });
  });

  // 按住唤醒：页面按钮或真键盘
  var holdBtn = $('#holdBtn'), holdTimer = null;
  function holdStart() {
    if (holdBtn.dataset.mode !== 'hold') { tryWake(holdBtn.dataset.mode === 'sched' ? 'sched' : holdBtn.dataset.mode); return; }
    if (!anyOff()) { holdBtn.classList.add('is-holding'); setTimeout(function () { holdBtn.classList.remove('is-holding'); }, 250); return; }
    holdBtn.classList.add('is-holding'); clearTimeout(holdTimer);
    holdTimer = setTimeout(function () { holdBtn.classList.remove('is-holding'); tryWake('hold'); }, 1000);
  }
  function holdEnd() { clearTimeout(holdTimer); holdBtn.classList.remove('is-holding'); }
  holdBtn.addEventListener('pointerdown', function (e) { e.preventDefault(); holdStart(); });
  ['pointerup', 'pointerleave', 'pointercancel'].forEach(function (ev) { holdBtn.addEventListener(ev, holdEnd); });
  holdBtn.addEventListener('keydown', function (e) { if ((e.key === ' ' || e.key === 'Enter') && !e.repeat) { e.preventDefault(); e.stopPropagation(); holdStart(); } });
  holdBtn.addEventListener('keyup', function (e) { if (e.key === ' ' || e.key === 'Enter') holdEnd(); });

  var stageVisible = false, keyHold = null;
  document.addEventListener('keydown', function (e) {
    if (!stageVisible || !anyOff() || e.repeat) return;
    var tag = (e.target && e.target.tagName) || '';
    if (/INPUT|TEXTAREA|SELECT/.test(tag) && e.target.type !== 'range') return;
    runner.interrupt();
    if (e.ctrlKey && e.altKey && e.shiftKey && e.code === 'Escape') { e.preventDefault(); tryWake('emergency'); return; }
    if (e.ctrlKey && e.altKey && e.code === 'KeyW') {
      e.preventDefault(); $$('.kbd', holdBtn).forEach(function (k) { k.classList.add('down'); });
      clearTimeout(keyHold); keyHold = setTimeout(function () { tryWake(S.tab === 'wake' && S.wake === 'key' ? 'key' : 'hold'); }, 1000);
      return;
    }
    if (/^(Control|Alt|Shift|Meta)$/.test(e.key)) return;
    tryWake('key');
  });
  document.addEventListener('keyup', function (e) {
    if (e.code === 'KeyW' || e.key === 'Control' || e.key === 'Alt') { clearTimeout(keyHold); $$('.kbd.down').forEach(function (k) { k.classList.remove('down'); }); }
  });

  // ---------- 时钟和任务 ----------
  var lastLog = 0;
  setInterval(function () {
    if (!stageVisible || document.hidden) return;
    var now = Date.now();
    if (S.tab !== 'sched') S.clock = (S.clock + (anyOff() ? 3 : 0.1)) % 1440;
    S.job += anyOff() ? 0.9 : 0.25;
    if (S.job >= 100) { S.job = 0; S.jobIdx += 1; }
    if (now - lastLog > (anyOff() ? 700 : 1300)) { lastLog = now; pushLog('ext'); if (Math.random() < 0.6) pushLog('in'); }
    paintClock(); paintJob();
  }, 120);

  // ---------- 自动演示 ----------
  var STOP = {};
  var sleep = function (ms) { return new Promise(function (r) { setTimeout(r, ms); }); };
  var runner = (function () {
    var gen = 0, running = false, hovered = false, held = false, timer = null;
    var r = {
      paused: false,
      active: function () { return stageVisible && !hovered && !held && !r.paused && !document.hidden && motionOK(); },
      wait: function (ms, g) {
        return (async function () {
          var left = ms, last = performance.now();
          while (left > 0) {
            if (g !== gen) throw STOP;
            await sleep(Math.min(50, left));
            var n = performance.now(); if (r.active()) left -= n - last; last = n;
          }
          if (g !== gen) throw STOP;
        })();
      },
      start: function () {
        if (running || !r.active()) return;
        running = true; var g = ++gen;
        (async function () {
          try { for (;;) await play(g); } catch (err) { if (err !== STOP) console.error(err); }
          finally { if (g === gen) running = false; pointer.hide(); }
        })();
      },
      stop: function () { gen += 1; running = false; pointer.hide(); },
      interrupt: function () {
        if (!motionOK()) return;
        r.stop(); held = true; clearTimeout(timer);
        timer = setTimeout(function () { held = false; r.start(); }, 12000);
      },
      setHover: function (h) { hovered = h; if (!h) setTimeout(r.start, 600); },
      toggle: function () { r.paused = !r.paused; if (r.paused) r.stop(); else { held = false; r.start(); } render(); }
    };
    return r;
  })();
  var pointer = {
    hide: function () { pointerEl.classList.remove('is-on'); },
    move: async function (el, g, ms) {
      if (!el || !el.getClientRects().length) return;
      var b = stage.getBoundingClientRect(), rc = el.getBoundingClientRect();
      var x = rc.left - b.left + rc.width * 0.5, y = rc.top - b.top + rc.height * 0.55;
      pointerEl.classList.add('is-on');
      pointerEl.style.transform = 'translate(' + x.toFixed(1) + 'px,' + y.toFixed(1) + 'px)';
      await runner.wait(ms || 800, g);
    },
    click: async function (el, g) {
      await pointer.move(el, g);
      pointerEl.classList.remove('is-click'); void pointerEl.offsetWidth; pointerEl.classList.add('is-click');
      el.classList.add('is-pressed'); await runner.wait(180, g); el.classList.remove('is-pressed');
      el.click();
      await runner.wait(350, g);
    },
    hold: async function (g) {
      await pointer.move(holdBtn, g);
      holdBtn.classList.add('is-holding');
      await runner.wait(1000, g);
      holdBtn.classList.remove('is-holding');
      tryWake('hold');
      await runner.wait(400, g);
    }
  };
  var autoTab = async function (tab, g) { if (S.tab !== tab) { selectTab(tab); await runner.wait(900, g); } };
  async function play(g) {
    // 01 立即息屏
    await autoTab('blank', g); wakeAll(); S.delay = 0; render();
    await runner.wait(1400, g);
    await pointer.click($('#blankBtn'), g);
    await runner.wait(3800, g);
    await pointer.click($('.off', disp.ext), g);
    await runner.wait(1600, g);
    await pointer.hold(g);
    await runner.wait(1800, g);
    // 02 只关所选
    await autoTab('select', g); S.scope = 'sel'; S.sel.in = false; S.sel.ext = false; S.keep = true; render();
    await runner.wait(900, g);
    await pointer.click($('.scr', disp.in), g);
    await runner.wait(500, g);
    await pointer.click($('#blankBtn'), g);
    for (var k = 0; k < 4; k++) { $('.cursor', disp.ext).style.transform = 'translate(' + (-200 + Math.random() * 260).toFixed(0) + 'px,' + (-60 + Math.random() * 120).toFixed(0) + 'px)'; await runner.wait(1100, g); }
    await pointer.hold(g);
    await runner.wait(1500, g);
    // 03 唤醒方式
    await autoTab('wake', g);
    await pointer.click($('#wakeList [data-wake="move"]'), g);
    await pointer.click($('#blankBtn'), g);
    await runner.wait(1500, g);
    await pointer.move($('.scr', disp.ext), g, 900); tryWake('move', 'ext');
    await runner.wait(1500, g);
    await pointer.click($('#wakeList [data-wake="hold"]'), g);
    await pointer.click($('#blankBtn'), g);
    await runner.wait(1000, g);
    await pointer.move($('.scr', disp.in), g, 900); tryWake('move', 'in');
    await runner.wait(1500, g);
    await pointer.hold(g);
    await runner.wait(1500, g);
    // 04 定时计划
    await autoTab('sched', g); S.day = 1; S.minute = 1320; applySchedule();
    await pointer.move($('#timeRange'), g);
    for (var m = 1320; m < 1440 + 480; m += 15) { S.minute = m % 1440; if (m >= 1440 && S.day === 1) S.day = 2; applySchedule(); await runner.wait(70, g); }
    await runner.wait(1200, g);
    await pointer.click($('#daySeg [data-day="6"]'), g);
    S.minute = 1380; applySchedule();
    for (m = 1380; m < 1440 + 660; m += 20) { S.minute = m % 1440; if (m >= 1440 && S.day === 6) S.day = 0; applySchedule(); await runner.wait(70, g); }
    await runner.wait(1800, g);
    pointer.hide();
  }

  stage.addEventListener('pointerenter', function (e) { if (e.pointerType === 'mouse') runner.setHover(true); });
  stage.addEventListener('pointerleave', function () { runner.setHover(false); });
  stage.addEventListener('pointerdown', function (e) { if (e.isTrusted) runner.interrupt(); }, true);
  $('.tabs').addEventListener('pointerdown', function (e) { if (e.isTrusted) runner.interrupt(); }, true);
  $('.plat').addEventListener('pointerdown', function (e) { if (e.isTrusted) runner.interrupt(); }, true);
  var toggle = $('#demoToggle');
  toggle.addEventListener('click', runner.toggle);
  if (motionOK()) toggle.hidden = false;
  if ('IntersectionObserver' in window) {
    new IntersectionObserver(function (en) { stageVisible = en[0].isIntersecting; if (stageVisible) runner.start(); else runner.stop(); }, { threshold: 0.3 }).observe(stage);
  } else { stageVisible = true; runner.start(); }
  document.addEventListener('visibilitychange', function () { if (!document.hidden) runner.start(); });
  reduce.addEventListener && reduce.addEventListener('change', function () { toggle.hidden = !motionOK(); if (motionOK()) runner.start(); else runner.stop(); });
  document.addEventListener('blankrun:lang', render);
  new MutationObserver(render).observe(root, { attributes: true, attributeFilter: ['data-theme'] });

  // ---------- 滚动淡入 ----------
  if (motionOK() && 'IntersectionObserver' in window) {
    root.classList.add('motion');
    var io = new IntersectionObserver(function (en) { en.forEach(function (x) { if (x.isIntersecting) { x.target.classList.add('is-visible'); io.unobserve(x.target); } }); }, { threshold: 0.12, rootMargin: '0px 0px -40px 0px' });
    $$('.reveal').forEach(function (el) { io.observe(el); });
  }

  render();
})();
