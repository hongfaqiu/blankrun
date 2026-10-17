// Windows 发布版不弹控制台
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[macro_use]
mod log;
mod engine;
mod platform;
mod update;

use std::sync::{mpsc, Arc, Mutex};

use engine::{Engine, Msg};
use blankrun_core::Config;
use platform::{Native, Platform};
use serde_json::{json, Value};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, PhysicalPosition, RunEvent, State, WindowEvent,
};

type Eng = Arc<Mutex<Engine>>;
/// 托盘菜单项，界面切换语言后由 apply_locale 改文字
struct Tray { blank: MenuItem<tauri::Wry>, settings: MenuItem<tauri::Wry>, quit: MenuItem<tauri::Wry> }

/// ui/i18n 里有哪些语言：build.rs 扫描得到 "en=English;zh-CN=简体中文"
fn locales() -> Vec<(&'static str, &'static str)> {
    env!("BLANKRUN_LOCALES").split(';').filter_map(|s| s.split_once('=')).collect()
}
/// 托盘菜单的初始文字（界面加载前用；之后以 ui/i18n 的词条为准）。没有的语言用英文。
fn tray_text(lang: &str) -> [&'static str; 4] {
    match lang.split('-').next().unwrap_or("") {
        "zh" => ["立即息屏", "设置…", "退出黑着跑", "黑着跑 BlankRun"],
        _ => ["Blank Screen Now", "Settings…", "Quit BlankRun", "BlankRun"],
    }
}
struct Tx(Mutex<mpsc::Sender<Msg>>);

fn send(tx: &State<Tx>, m: Msg) { let _ = tx.0.lock().unwrap().send(m); }
fn lock<'a>(e: &'a State<'_, Eng>) -> std::sync::MutexGuard<'a, Engine> { e.lock().unwrap_or_else(|p| p.into_inner()) }

// 需要锁引擎的命令一律 async：同步命令跑在主线程上，而引擎线程持锁时可能要等主线程（查询显示器等），会死锁。
type R<T> = Result<T, String>;

#[tauri::command]
async fn get_info(e: State<'_, Eng>) -> R<Value> {
    let locales: Vec<Value> = locales().iter().map(|(c, n)| json!({ "code": c, "name": n })).collect();
    Ok(json!({ "os": platform::OS, "version": env!("CARGO_PKG_VERSION"), "config_path": lock(&e).cfg_path,
               "system_locale": platform::system_locale(), "locales": locales }))
}
#[tauri::command]
async fn get_config(e: State<'_, Eng>) -> R<Config> { Ok(lock(&e).cfg.clone()) }
#[tauri::command]
async fn save_config(e: State<'_, Eng>, app: AppHandle, cfg: Config) -> R<()> {
    lock(&e).save_config(cfg)?;
    let _ = app.emit("nb://config", ());
    Ok(())
}
#[tauri::command]
async fn get_status(e: State<'_, Eng>) -> R<engine::Status> { Ok(lock(&e).status()) }
#[tauri::command]
fn blank_now(tx: State<Tx>, delay_s: u32) { send(&tx, Msg::Blank { delay_s }) }
#[tauri::command]
fn cancel(tx: State<Tx>) { send(&tx, Msg::Cancel) }
#[tauri::command]
fn skip_tonight(tx: State<Tx>) { send(&tx, Msg::SkipTonight) }
#[tauri::command]
fn unskip(tx: State<Tx>) { send(&tx, Msg::Unskip) }
#[tauri::command]
fn postpone(tx: State<Tx>, min: u32) { send(&tx, Msg::Postpone { min: min.clamp(1, 240) }) }
#[tauri::command]
fn overlay_input(tx: State<Tx>) { send(&tx, Msg::OverlayInput) }
#[tauri::command]
async fn get_history(e: State<'_, Eng>) -> R<blankrun_core::history::History> { Ok(lock(&e).history.clone()) }
#[tauri::command]
async fn clear_history(e: State<'_, Eng>) -> R<()> { lock(&e).clear_history(); Ok(()) }
#[tauri::command]
fn open_link(which: String) { update::open(&which) }
#[tauri::command]
async fn check_update(app: AppHandle) -> R<()> { update::check(&app, true).await; Ok(()) }
#[tauri::command]
async fn install_update(e: State<'_, Eng>, app: AppHandle) -> R<()> {
    let eng = e.inner().clone();
    update::install(app, move || eng.lock().map_or(true, |g| g.status().state != "idle")).await
}
#[tauri::command]
fn wake_now(tx: State<Tx>) { send(&tx, Msg::Wake) }
#[tauri::command]
fn quit(tx: State<Tx>) { send(&tx, Msg::Quit) }

#[tauri::command]
async fn get_diagnostics(e: State<'_, Eng>, app: AppHandle) -> R<Value> {
    let mut g = lock(&e);
    let monitors = g.monitor_report(&app);
    Ok(json!({
        "caps": g.plat.capabilities(),
        "perms": g.plat.permissions(),
        "last_session": g.last_session,
        "monitors": monitors,
        "modern_standby": g.plat.power_off_unsafe(),
    }))
}
#[tauri::command]
async fn get_monitors(e: State<'_, Eng>, app: AppHandle) -> R<Vec<Value>> { Ok(lock(&e).monitor_report(&app)) }
#[tauri::command]
async fn request_permission(e: State<'_, Eng>, which: String) -> R<()> { lock(&e).plat.request_permission(&which); Ok(()) }
#[tauri::command]
async fn reveal_config(e: State<'_, Eng>) -> R<()> {
    let g = lock(&e);
    if !g.cfg_path.exists() { g.write_current_config()?; }
    platform::reveal(&g.cfg_path);
    Ok(())
}
#[tauri::command]
async fn export_diagnostics(e: State<'_, Eng>, text: String) -> R<String> {
    let p = lock(&e).cfg_path.with_file_name("diagnostics.txt");
    std::fs::write(&p, text).map_err(|e| e.to_string())?;
    platform::reveal(&p);
    Ok(p.display().to_string())
}
/// 界面选好语言后把托盘菜单和窗口标题换成对应文字（切换语言时实时生效，不用重启）
#[tauri::command]
fn apply_locale(app: AppHandle, tray: State<Tray>, text: std::collections::HashMap<String, String>) {
    let get = |k: &str| text.get(k).map(|s| s.as_str()).filter(|s| !s.is_empty());
    if let Some(t) = get("tray.blank") { let _ = tray.blank.set_text(t); }
    if let Some(t) = get("tray.settings") { let _ = tray.settings.set_text(t); }
    if let Some(t) = get("tray.quit") { let _ = tray.quit.set_text(t); }
    if let (Some(t), Some(icon)) = (get("tray.tooltip"), app.tray_by_id("main")) { let _ = icon.set_tooltip(Some(t)); }
    for label in ["popover", "settings", "onboarding"] {
        if let (Some(t), Some(w)) = (get(&format!("title.{label}")), app.get_webview_window(label)) { let _ = w.set_title(t); }
    }
}
#[tauri::command]
fn open_settings(app: AppHandle, section: Option<String>) { show_settings(&app, section.as_deref()) }
#[tauri::command]
fn hide_window(w: tauri::WebviewWindow) { let _ = w.hide(); }
#[tauri::command]
async fn finish_onboarding(e: State<'_, Eng>, w: tauri::WebviewWindow) -> R<()> {
    let _ = std::fs::write(lock(&e).cfg_path.with_file_name("onboarded"), "1");
    let _ = w.hide();
    // 欢迎页关掉后把面板在托盘图标旁打开一次，让人知道以后去哪里找它
    let app = w.app_handle().clone();
    let _ = w.run_on_main_thread(move || {
        if let Some(r) = app.tray_by_id("main").and_then(|t| t.rect().ok().flatten()) { toggle_popover(&app, r); }
    });
    Ok(())
}

impl Engine {
    fn write_current_config(&self) -> Result<(), String> {
        if let Some(d) = self.cfg_path.parent() { std::fs::create_dir_all(d).map_err(|e| e.to_string())?; }
        std::fs::write(&self.cfg_path, self.cfg.to_toml()).map_err(|e| e.to_string())
    }
}

/// 托盘图标是否真的看得见。macOS 菜单栏放不下时，图标会被挤到刘海后面（系统不报错），
/// 这时直接打开设置窗口并提示，免得用户找不到入口。
fn check_tray(app: AppHandle) {
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(1500));
        let app2 = app.clone();
        let _ = app.run_on_main_thread(move || {
            let Some(tray) = app2.tray_by_id("main") else { nblog!("tray: not created"); return };
            let rect = tray.rect().ok().flatten();
            let scale = app2.primary_monitor().ok().flatten().map(|m| m.scale_factor()).unwrap_or(1.0);
            let pos = rect.map(|r| (r.position.to_logical::<f64>(scale), r.size.to_logical::<f64>(scale)));
            #[cfg(target_os = "macos")]
            let notch = platform::notch_span();
            #[cfg(not(target_os = "macos"))]
            let notch: Option<(f64, f64)> = None;
            let hidden = match (pos, notch) {
                (None, _) => true,
                (Some((p, sz)), Some((l, r))) => p.x + sz.width > l && p.x < r,
                (Some((p, _)), None) => p.x < 0.0,
            };
            nblog!("tray: rect={:?} notch={notch:?} hidden={hidden}", pos.map(|(p, s)| (p.x, p.y, s.width, s.height)));
            if hidden {
                show_settings(&app2, None);
                let _ = app2.emit("nb://tray_hidden", ());
            }
        });
    });
}

fn show_settings(app: &AppHandle, section: Option<&str>) {
    if let Some(p) = app.get_webview_window("popover") { let _ = p.hide(); }
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.show();
        let _ = w.set_focus();
        if let Some(s) = section { let _ = w.emit("nb://nav", s); }
    }
}

/// 面板：点外面（失焦）就收起。刚弹出的 300ms 内的失焦不算（Windows 上点托盘图标那一下会先抢走焦点），
/// 面板里开着原生下拉框时也不算。
static POP_SHOWN: Mutex<Option<std::time::Instant>> = Mutex::new(None);
static POP_BLUR_HIDDEN: Mutex<Option<std::time::Instant>> = Mutex::new(None);
static POP_HOLD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[tauri::command]
fn popover_hold(on: bool) { POP_HOLD.store(on, std::sync::atomic::Ordering::Relaxed); }

fn popover_blur(w: &tauri::Window) {
    if POP_HOLD.load(std::sync::atomic::Ordering::Relaxed) { return; }
    if POP_SHOWN.lock().ok().and_then(|g| *g).is_some_and(|t| t.elapsed() < std::time::Duration::from_millis(300)) { return; }
    if w.is_visible().unwrap_or(false) {
        let _ = w.hide();
        if let Ok(mut g) = POP_BLUR_HIDDEN.lock() { *g = Some(std::time::Instant::now()); }
    }
}

fn toggle_popover(app: &AppHandle, anchor: tauri::Rect) {
    let Some(w) = app.get_webview_window("popover") else { return };
    if w.is_visible().unwrap_or(false) { let _ = w.hide(); return; }
    // 面板开着时点托盘图标：按下那一刻面板已因失焦收起，这次点击就只是「关」，别再打开
    if POP_BLUR_HIDDEN.lock().ok().and_then(|mut g| g.take()).is_some_and(|t| t.elapsed() < std::time::Duration::from_millis(400)) { return; }
    POP_HOLD.store(false, std::sync::atomic::Ordering::Relaxed);
    if let Ok(mut g) = POP_SHOWN.lock() { *g = Some(std::time::Instant::now()); }
    let scale = w.scale_factor().unwrap_or(1.0);
    let (pos, size) = (anchor.position.to_physical::<f64>(scale), anchor.size.to_physical::<f64>(scale));
    let pw = w.outer_size().map(|s| (s.width as f64, s.height as f64)).unwrap_or((320.0 * scale, 480.0 * scale));
    let x = pos.x + size.width / 2.0 - pw.0 / 2.0;
    // Mac 菜单栏在上：面板在图标下方；Windows 任务栏通常在下：面板在图标上方
    let y = if pos.y < pw.1 { pos.y + size.height + 4.0 } else { pos.y - pw.1 - 8.0 };
    let _ = w.set_position(PhysicalPosition::new(x.max(0.0), y.max(0.0)));
    let _ = w.show();
    let _ = w.set_focus();
    let _ = w.emit("nb://shown", ());
}

/// 命令行（测试和脚本用）：
/// `--blank-now[=秒]` 启动后立即（或延迟）息屏；`--wake-after=秒` 息屏后自动恢复；`--quit-after=秒` 到时退出。
fn cli(tx: mpsc::Sender<Msg>) {
    let num = |name: &str| std::env::args().find_map(|a| {
        let v = a.strip_prefix(name)?;
        if v.is_empty() { Some(0) } else { v.strip_prefix('=')?.parse::<u64>().ok() }
    });
    let after = |secs: u64, m: Msg| {
        let tx = tx.clone();
        std::thread::spawn(move || { std::thread::sleep(std::time::Duration::from_secs(secs)); let _ = tx.send(m); });
    };
    let delay = num("--blank-now");
    if let Some(d) = delay { after(1, Msg::Blank { delay_s: d as u32 }); }
    if let Some(w) = num("--wake-after") { after(1 + delay.unwrap_or(0) + w, Msg::Wake); }
    if let Some(q) = num("--quit-after") { after(q, Msg::Quit); }
}

/// 托盘图标：和 App 图标同一个圆环（site/favicon.svg 的形状），运行时画出来，不带 PNG 解码器。
/// Mac 用模板图（系统按菜单栏深浅自动着色）；Windows 按任务栏主题选深色或浅色。
fn tray_icon() -> (tauri::image::Image<'static>, bool) {
    let n: u32 = if cfg!(target_os = "macos") { 44 } else { 32 };
    #[cfg(windows)]
    let rgb = if platform::taskbar_light() { [0x1E, 0x1E, 0x28] } else { [0xEC, 0xEC, 0xEE] };
    #[cfg(not(windows))]
    let rgb = [0u8, 0, 0];
    let s = n as f32 / 32.0;
    let mut px = Vec::with_capacity((n * n * 4) as usize);
    for y in 0..n {
        for x in 0..n {
            // 4×4 超采样抗锯齿：外圆 (16,16) r=14 减去内圆 (17,14.5) r=5.5
            let mut hit = 0;
            for sy in 0..4 { for sx in 0..4 {
                let (fx, fy) = ((x as f32 + (sx as f32 + 0.5) / 4.0) / s, (y as f32 + (sy as f32 + 0.5) / 4.0) / s);
                let outer = (fx - 16.0).powi(2) + (fy - 16.0).powi(2) <= 14.0f32.powi(2);
                let inner = (fx - 17.0).powi(2) + (fy - 14.5).powi(2) <= 5.5f32.powi(2);
                if outer && !inner { hit += 1; }
            } }
            px.extend_from_slice(&[rgb[0], rgb[1], rgb[2], (hit * 255 / 16) as u8]);
        }
    }
    (tauri::image::Image::new_owned(px, n, n), cfg!(target_os = "macos"))
}

fn main() {
    std::panic::set_hook(Box::new(|info| {
        platform::emergency_restore();
        nblog!("panic: {info}");
    }));

    let (tx, rx) = mpsc::channel::<Msg>();
    let input_tx = tx.clone();
    platform::set_input_sink(Box::new(move |i| { let _ = input_tx.send(Msg::Input(i)); }));
    let hk_tx = tx.clone();
    platform::set_hotkey_sink(Box::new(move || { let _ = hk_tx.send(Msg::Hotkey); }));

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(move |app| {
            // macOS：只在菜单栏显示，不出现在 Dock
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let cfg_path = app.path().app_config_dir()?.join("config.toml");
            // 单实例：已经在运行就请它把设置窗口拿出来，自己退出（Windows 上避免出现两个托盘图标）
            if let Some(d) = cfg_path.parent() { let _ = std::fs::create_dir_all(d); }
            if !platform::instance_lock(&cfg_path.with_file_name("instance.lock")) {
                let _ = std::fs::write(cfg_path.with_file_name("show.request"), "1");
                std::process::exit(0);
            }
            log::init(cfg_path.with_file_name("blankrun.log"));
            let cfg = std::fs::read_to_string(&cfg_path).ok().and_then(|s| Config::from_toml(&s).map_err(|e| nblog!("config.toml ignored, using defaults: {e}")).ok()).unwrap_or_default();
            let codes: Vec<&str> = locales().iter().map(|(c, _)| *c).collect();
            let tt = tray_text(&blankrun_core::locale::resolve(&cfg.general.language, &platform::system_locale(), &codes));
            let lang = blankrun_core::locale::resolve(&cfg.general.language, &platform::system_locale(), &codes);
            nblog!("start v{} os={} system_locale={:?} lang={lang} args={:?}", env!("CARGO_PKG_VERSION"), platform::OS,
                platform::system_locale(), std::env::args().skip(1).collect::<Vec<_>>());
            let mut plat = Native::default();
            // 上次异常退出（崩溃、强退、断电）留下的标记：还原背光、点亮屏幕
            let marker = cfg_path.with_file_name("blanking.state");
            if let Ok(saved) = std::fs::read_to_string(&marker) {
                nblog!("found {marker:?} from an abnormal exit: restoring ({saved:?})");
                if let Err(e) = plat.restore_on_startup(Some(&saved)) { nblog!("restore after abnormal exit failed: {e}"); }
                let _ = std::fs::remove_file(&marker);
            }
            let first_run = !cfg_path.with_file_name("onboarded").exists();
            for c in plat.capabilities() { nblog!("cap {} {} {}", c.id, c.status, c.tech); }
            nblog!("permissions (accessibility, input monitoring) = {:?}; modern_standby = {}; fullscreen_video = {}; launch_at_login = {:?}",
                plat.permissions(), plat.power_off_unsafe(), plat.fullscreen_video(), plat.launch_at_login());
            for (i, m) in app.available_monitors().unwrap_or_default().iter().enumerate() {
                nblog!("monitor {} {:?} at {:?}", engine::monitor_name(i, m), m.size(), m.position());
            }
            update::start(app.handle().clone(), cfg_path.with_file_name("update.check"));
            let eng: Eng = Arc::new(Mutex::new(Engine::new(cfg, cfg_path, plat)));
            app.manage(eng.clone());
            app.manage(Tx(Mutex::new(tx.clone())));
            let handle = app.handle().clone();
            std::thread::Builder::new().name("nb-engine".into()).spawn(move || engine::run(handle, eng, rx))?;

            cli(tx.clone());
            // 第一次运行：欢迎页（Mac 缺权限时顺带引导授权）。之后手动打开（开始菜单、启动台、安装完成页）就显示设置；
            // 只有登录时自动启动（--autostart）和测试参数才静默待在托盘里。
            let quiet = std::env::args().skip(1).any(|a| a == "--autostart" || a.starts_with("--blank-now") || a.starts_with("--quit-after"));
            if first_run {
                nblog!("first run: showing welcome");
                if let Some(w) = app.get_webview_window("onboarding") { let _ = w.show(); let _ = w.set_focus(); }
            } else if !quiet {
                show_settings(app.handle(), None);
            }

            let blank = MenuItem::with_id(app, "blank", tt[0], true, None::<&str>)?;
            let settings = MenuItem::with_id(app, "settings", tt[1], true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", tt[2], true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&blank, &settings, &quit])?;
            app.manage(Tray { blank: blank.clone(), settings: settings.clone(), quit: quit.clone() });
            let menu_tx = tx.clone();

            #[cfg(target_os = "macos")]
            platform::prefer_tray_position();
            let (icon, template) = tray_icon();
            TrayIconBuilder::with_id("main")
                .icon(icon)
                .icon_as_template(template)
                .tooltip(tt[3])
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_tray_icon_event(|tray, e| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, rect, .. } = e {
                        toggle_popover(tray.app_handle(), rect);
                    }
                })
                .on_menu_event(move |app, e| match e.id.as_ref() {
                    "blank" => { let _ = menu_tx.send(Msg::Blank { delay_s: 0 }); }
                    "settings" => show_settings(app, None),
                    "quit" => { let _ = menu_tx.send(Msg::Quit); }
                    _ => {}
                })
                .build(app)?;
            check_tray(app.handle().clone());
            // Windows：任务栏在浅色/深色之间切换时（设置里改主题、定时切换）换托盘图标颜色。读注册表很便宜，几秒看一次
            #[cfg(windows)]
            {
                let h = app.handle().clone();
                let _ = std::thread::Builder::new().name("nb-theme".into()).spawn(move || {
                    let mut light = platform::taskbar_light();
                    loop {
                        std::thread::sleep(std::time::Duration::from_secs(3));
                        let now = platform::taskbar_light();
                        if now == light { continue; }
                        light = now;
                        nblog!("taskbar theme changed: light={now}");
                        let h2 = h.clone();
                        let _ = h.run_on_main_thread(move || {
                            if let Some(t) = h2.tray_by_id("main") { let _ = t.set_icon(Some(tray_icon().0)); }
                        });
                    }
                });
            }
            Ok(())
        })
        .on_window_event(|w, e| match e {
            // 设置等窗口关闭时只隐藏，应用继续驻留托盘；遮罩窗口由引擎 destroy
            WindowEvent::CloseRequested { api, .. } if !w.label().starts_with("overlay-") => { api.prevent_close(); let _ = w.hide(); }
            WindowEvent::Focused(false) if w.label() == "popover" => popover_blur(w),
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            get_info, get_config, popover_hold, get_history, clear_history, open_link, check_update, install_update, save_config, get_status, blank_now, cancel, skip_tonight, unskip, postpone, overlay_input, wake_now, quit,
            get_diagnostics, get_monitors, request_permission, reveal_config, export_diagnostics, open_settings, hide_window,
            finish_onboarding, apply_locale
        ])
        .build(tauri::generate_context!())
        .expect("error while building BlankRun");

    app.run(|app, ev| match ev {
        // 没有可见窗口时不要退出（托盘常驻）
        RunEvent::ExitRequested { api, code: None, .. } => api.prevent_exit(),
        // 再次打开 App（Finder / 程序坞 / 聚焦搜索）：打开设置窗口，菜单栏图标被挡住时也不会找不到
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => { nblog!("reopen: showing settings"); show_settings(app, None) }
        RunEvent::Exit => {
            // 主线程上不能阻塞等锁（引擎线程可能正在等主线程）：拿不到锁就做无锁的紧急恢复
            match app.try_state::<Eng>().map(|e| e.inner().clone()) {
                Some(e) => match e.try_lock() {
                    Ok(mut g) => g.shutdown(app),
                    Err(_) => platform::emergency_restore(),
                },
                None => platform::emergency_restore(),
            }
        }
        _ => {}
    });
}
