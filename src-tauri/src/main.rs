// Windows 发布版不弹控制台
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod platform;

use std::{path::PathBuf, sync::Mutex};

use nightblank_core::{state::Action, Config, Event, StateMachine};
use platform::{Native, Platform};
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    Manager, State,
};

struct App { sm: StateMachine, plat: Native, cfg: Config, cfg_path: PathBuf }

fn apply(app: &mut App, ev: Event) {
    for a in app.sm.handle(ev) {
        let p = &mut app.plat;
        let r = match a {
            Action::StartKeepAwake => p.keep_awake(true),
            Action::StopKeepAwake => p.keep_awake(false),
            Action::DisplayOff => p.display_off(true),
            Action::DisplayRestore => p.display_off(false),
            Action::BacklightOff => p.keyboard_backlight(true),
            Action::BacklightRestore => p.keyboard_backlight(false),
            Action::InputHookOn => p.input_hook(true),
            Action::InputHookOff => p.input_hook(false),
            Action::LockScreen => p.lock_screen(),
        };
        if let Err(e) = r { eprintln!("[nightblank] {a:?} failed: {e}"); }
    }
}

fn load_config(path: &PathBuf) -> Config {
    std::fs::read_to_string(path).ok().and_then(|s| Config::from_toml(&s).ok()).unwrap_or_default()
}

#[tauri::command]
fn get_config(st: State<Mutex<App>>) -> String { st.lock().unwrap().cfg.to_toml() }

#[tauri::command]
fn save_config(st: State<Mutex<App>>, toml: String) -> Result<(), String> {
    let cfg = Config::from_toml(&toml)?;
    let mut app = st.lock().unwrap();
    if let Some(dir) = app.cfg_path.parent() { std::fs::create_dir_all(dir).map_err(|e| e.to_string())?; }
    std::fs::write(&app.cfg_path, cfg.to_toml()).map_err(|e| e.to_string())?;
    app.sm.lock_on_wake = cfg.wake.lock_on_wake;
    app.cfg = cfg;
    Ok(())
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            // macOS：只在菜单栏显示，不出现在 Dock
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let cfg_path = app.path().app_config_dir()?.join("config.toml");
            let cfg = load_config(&cfg_path);
            let mut plat = Native::default();
            let _ = plat.restore_on_startup();
            app.manage(Mutex::new(App { sm: StateMachine::new(cfg.wake.lock_on_wake), plat, cfg, cfg_path }));

            let blank = MenuItem::with_id(app, "blank", "立即息屏", true, None::<&str>)?;
            let settings = MenuItem::with_id(app, "settings", "设置…", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&blank, &settings, &quit])?;

            TrayIconBuilder::new()
                .icon(app.default_window_icon().cloned().expect("icon"))
                .tooltip("NightBlank")
                .menu(&menu)
                .on_menu_event(|app, e| match e.id.as_ref() {
                    "blank" => {
                        let st = app.state::<Mutex<App>>();
                        let mut a = st.lock().unwrap();
                        apply(&mut a, Event::Arm);
                        // TODO(M1): 倒计时 + 计划调度（nightblank_core::schedule），这里先直接进入息屏
                        apply(&mut a, Event::Blank);
                    }
                    "settings" => {
                        if let Some(w) = app.get_webview_window("settings") { let _ = w.show(); let _ = w.set_focus(); }
                    }
                    "quit" => {
                        let st = app.state::<Mutex<App>>();
                        let mut a = st.lock().unwrap();
                        apply(&mut a, Event::WakeRequested);
                        apply(&mut a, Event::RestoreDone);
                        apply(&mut a, Event::Cancel);
                        app.exit(0);
                    }
                    _ => {}
                })
                .build(app)?;
            Ok(())
        })
        .on_window_event(|w, e| {
            // 关闭设置窗口只隐藏，应用继续驻留托盘
            if let tauri::WindowEvent::CloseRequested { api, .. } = e { api.prevent_close(); let _ = w.hide(); }
        })
        .invoke_handler(tauri::generate_handler![get_config, save_config])
        .run(tauri::generate_context!())
        .expect("error while running NightBlank");
}
