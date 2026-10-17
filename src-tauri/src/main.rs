// Windows 发布版不弹控制台
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod engine;
mod platform;

use std::sync::{mpsc, Arc, Mutex};

use engine::{Engine, Msg};
use nightblank_core::Config;
use platform::{Native, Platform};
use serde_json::{json, Value};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, PhysicalPosition, RunEvent, State, WindowEvent,
};

type Eng = Arc<Mutex<Engine>>;
struct Tx(Mutex<mpsc::Sender<Msg>>);

fn send(tx: &State<Tx>, m: Msg) { let _ = tx.0.lock().unwrap().send(m); }
fn lock<'a>(e: &'a State<'_, Eng>) -> std::sync::MutexGuard<'a, Engine> { e.lock().unwrap_or_else(|p| p.into_inner()) }

// 需要锁引擎的命令一律 async：同步命令跑在主线程上，而引擎线程持锁时可能要等主线程（查询显示器等），会死锁。
type R<T> = Result<T, String>;

#[tauri::command]
async fn get_info(e: State<'_, Eng>) -> R<Value> {
    Ok(json!({ "os": platform::OS, "version": env!("CARGO_PKG_VERSION"), "config_path": lock(&e).cfg_path }))
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
fn overlay_input(tx: State<Tx>) { send(&tx, Msg::OverlayInput) }
#[tauri::command]
fn wake_now(tx: State<Tx>) { send(&tx, Msg::Wake) }
#[tauri::command]
fn quit(tx: State<Tx>) { send(&tx, Msg::Quit) }

#[tauri::command]
async fn get_diagnostics(e: State<'_, Eng>, app: AppHandle) -> R<Value> {
    let monitors: Vec<String> = app.available_monitors().unwrap_or_default().iter()
        .enumerate().map(|(i, m)| engine::monitor_name(i, m)).collect();
    let mut g = lock(&e);
    Ok(json!({
        "caps": g.plat.capabilities(),
        "perms": g.plat.permissions(),
        "last_session": g.last_session,
        "monitors": monitors,
        "modern_standby": g.plat.power_off_unsafe(),
    }))
}
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
#[tauri::command]
fn open_settings(app: AppHandle, section: Option<String>) { show_settings(&app, section.as_deref()) }
#[tauri::command]
fn hide_window(w: tauri::WebviewWindow) { let _ = w.hide(); }
#[tauri::command]
async fn finish_onboarding(e: State<'_, Eng>, w: tauri::WebviewWindow) -> R<()> {
    let _ = std::fs::write(lock(&e).cfg_path.with_file_name("onboarded"), "1");
    let _ = w.hide();
    Ok(())
}

impl Engine {
    fn write_current_config(&self) -> Result<(), String> {
        if let Some(d) = self.cfg_path.parent() { std::fs::create_dir_all(d).map_err(|e| e.to_string())?; }
        std::fs::write(&self.cfg_path, self.cfg.to_toml()).map_err(|e| e.to_string())
    }
}

fn show_settings(app: &AppHandle, section: Option<&str>) {
    if let Some(p) = app.get_webview_window("popover") { let _ = p.hide(); }
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.show();
        let _ = w.set_focus();
        if let Some(s) = section { let _ = w.emit("nb://nav", s); }
    }
}

fn toggle_popover(app: &AppHandle, anchor: tauri::Rect) {
    let Some(w) = app.get_webview_window("popover") else { return };
    if w.is_visible().unwrap_or(false) { let _ = w.hide(); return; }
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

fn main() {
    std::panic::set_hook(Box::new(|info| {
        platform::emergency_restore();
        eprintln!("[nightblank] panic: {info}");
    }));

    let (tx, rx) = mpsc::channel::<Msg>();
    let input_tx = tx.clone();
    platform::set_input_sink(Box::new(move |i| { let _ = input_tx.send(Msg::Input(i)); }));

    let app = tauri::Builder::default()
        .setup(move |app| {
            // macOS：只在菜单栏显示，不出现在 Dock
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let cfg_path = app.path().app_config_dir()?.join("config.toml");
            let cfg = std::fs::read_to_string(&cfg_path).ok().and_then(|s| Config::from_toml(&s).ok()).unwrap_or_default();
            let mut plat = Native::default();
            // 上次异常退出（崩溃、强退、断电）留下的标记：还原背光、点亮屏幕
            let marker = cfg_path.with_file_name("blanking.state");
            if let Ok(saved) = std::fs::read_to_string(&marker) {
                if let Err(e) = plat.restore_on_startup(Some(&saved)) { eprintln!("[nightblank] restore: {e}"); }
                let _ = std::fs::remove_file(&marker);
            }
            let first_run = !cfg_path.with_file_name("onboarded").exists();
            let needs_perm = plat.permissions().is_some_and(|(a, b)| !(a && b));
            let eng: Eng = Arc::new(Mutex::new(Engine::new(cfg, cfg_path, plat)));
            app.manage(eng.clone());
            app.manage(Tx(Mutex::new(tx.clone())));
            let handle = app.handle().clone();
            std::thread::Builder::new().name("nb-engine".into()).spawn(move || engine::run(handle, eng, rx))?;

            if first_run && needs_perm {
                if let Some(w) = app.get_webview_window("onboarding") { let _ = w.show(); let _ = w.set_focus(); }
            }

            let blank = MenuItem::with_id(app, "blank", "立即息屏", true, None::<&str>)?;
            let settings = MenuItem::with_id(app, "settings", "设置…", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "退出 NightBlank", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&blank, &settings, &quit])?;
            let menu_tx = tx.clone();

            TrayIconBuilder::with_id("main")
                .icon(app.default_window_icon().cloned().expect("icon"))
                .tooltip("NightBlank")
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
            Ok(())
        })
        .on_window_event(|w, e| match e {
            // 设置等窗口关闭时只隐藏，应用继续驻留托盘；遮罩窗口由引擎 destroy
            WindowEvent::CloseRequested { api, .. } if !w.label().starts_with("overlay-") => { api.prevent_close(); let _ = w.hide(); }
            // 菜单栏面板失焦即收起
            WindowEvent::Focused(false) if w.label() == "popover" => { let _ = w.hide(); }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            get_info, get_config, save_config, get_status, blank_now, cancel, skip_tonight, overlay_input, wake_now, quit,
            get_diagnostics, request_permission, reveal_config, export_diagnostics, open_settings, hide_window,
            finish_onboarding
        ])
        .build(tauri::generate_context!())
        .expect("error while building NightBlank");

    app.run(|app, ev| match ev {
        // 没有可见窗口时不要退出（托盘常驻）
        RunEvent::ExitRequested { api, code: None, .. } => api.prevent_exit(),
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
