#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
use clipforge::{
    picker::{Picker, Snapshot},
    platform,
    service::{self, Command, Update},
    settings::{self, Hotkeys},
};
use std::{
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Sender},
    },
    time::Duration,
};
use tauri::{Emitter, Manager, State};

fn smoke_report(message: &str) {
    eprintln!("{message}");
}

struct Runtime {
    settings_path: Mutex<Option<std::path::PathBuf>>,
    settings_update: Mutex<()>,
    picker: Mutex<Picker>,
    tx: Sender<Command>,
    smoke: bool,
    frontend_ready: AtomicBool,
}
impl Runtime {
    fn lock(&self) -> std::sync::MutexGuard<'_, Picker> {
        self.picker.lock().unwrap_or_else(|e| e.into_inner())
    }
    fn send(&self, command: Command) -> Result<(), String> {
        self.tx
            .send(command)
            .map_err(|_| "Clipboard service is unavailable".into())
    }
}
fn publish(app: &tauri::AppHandle) {
    let snapshot = app.state::<Runtime>().lock().snapshot();
    let _ = app.emit("clipforge-state", snapshot);
}
fn hide(app: &tauri::AppHandle, commit: bool) -> Result<(), String> {
    hide_with_focus(app, commit, true)
}
fn hide_with_focus(
    app: &tauri::AppHandle,
    commit: bool,
    restore_focus: bool,
) -> Result<(), String> {
    let state = app.state::<Runtime>();
    if let Some(window) = app.get_webview_window("main") {
        window.hide().map_err(|e| e.to_string())?;
    }
    let (target, text) = {
        let mut p = state.lock();
        let target = p.target;
        (target, p.dismiss(commit))
    };

    if restore_focus {
        platform::focus(target);
    }
    if let Some(text) = text {
        state.send(Command::Load(text))?;
    }
    publish(app);
    Ok(())
}
fn show(app: &tauri::AppHandle, copy: bool, target: usize) {
    let state = app.state::<Runtime>();
    if state.lock().pinned {
        return;
    }
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    if let Err(error) = window.show() {
        state.lock().status = error.to_string();
        publish(app);
        return;
    }
    {
        let mut picker = state.lock();
        if picker.pinned {
            return;
        }
        picker.show(copy, target);
    }
    publish(app);

    #[cfg(windows)]
    if let Ok(hwnd) = window.hwnd() {
        platform::reveal_picker(hwnd.0 as usize, target);
    }
    #[cfg(not(windows))]
    let _ = window.center();
    if let Err(error) = window.set_focus() {
        state.lock().status = format!("Could not focus menu: {error}");
        publish(app);
    }
}
fn apply_update(app: &tauri::AppHandle, update: Update) {
    let state = app.state::<Runtime>();
    match update {
        Update::Snapshot(engine) => {
            state.lock().refresh(*engine);
            publish(app);
        }
        Update::Clipboard(text) => {
            state.lock().current_clipboard = text;
            publish(app);
        }
        Update::Status(status) => {
            state.lock().status = status;
            publish(app);
        }
        Update::Show { copy, target } => show(app, copy, target),
        Update::Toggle { target } => {
            let (visible, pinned) = {
                let picker = state.lock();
                (picker.visible, picker.pinned)
            };

            if visible {
                if pinned {
                    let _ = app.emit("clipforge-toggle", ());
                } else if let Err(error) = hide(app, false) {
                    state.lock().status = error.to_string();
                    publish(app);
                }
            } else {
                show(app, false, target);
            }
        }
        Update::Dismiss { target, commit } => {
            let should_hide = {
                let p = state.lock();
                p.shortcut_can_dismiss(target)
            };
            if should_hide {
                let _ = hide(app, commit);
            } else if state.lock().pinned && !commit {
                // The global hook consumes Escape while shortcut modifiers are held.
                let _ = app.emit("clipforge-cancel-edit", ());
            }
        }
    }
}
#[tauri::command]
fn snapshot(state: State<'_, Runtime>) -> Snapshot {
    if !state.frontend_ready.swap(true, Ordering::Relaxed) && state.smoke {
        smoke_report("PASS: Tauri frontend initialized and invoked Rust snapshot");
    }
    state.lock().snapshot()
}
#[tauri::command]
fn load_text(app: tauri::AppHandle, state: State<'_, Runtime>, text: String) -> Result<(), String> {
    state.send(Command::Load(text))?;
    hide(&app, false)
}
#[tauri::command]
async fn set_clipboard(
    app: tauri::AppHandle,
    state: State<'_, Runtime>,
    text: String,
) -> Result<(), String> {
    service::validate_text(&text)?;
    let (reply, result) = mpsc::channel();
    state.send(Command::SetClipboard { text, reply })?;
    tauri::async_runtime::spawn_blocking(move || {
        result
            .recv()
            .map_err(|_| "Clipboard service is unavailable".to_owned())?
    })
    .await
    .map_err(|error| error.to_string())??;
    state.lock().clear_selection();
    publish(&app);
    Ok(())
}
#[tauri::command]
fn save_text(
    app: tauri::AppHandle,
    state: State<'_, Runtime>,
    register: char,
    text: String,
) -> Result<(), String> {
    clipforge::core::register_index(register).map_err(|e| e.to_string())?;
    state.lock().clear_selection();
    state.send(Command::SaveText { register, text })?;
    publish(&app);
    Ok(())
}
#[tauri::command]
fn begin_edit(app: tauri::AppHandle, state: State<'_, Runtime>) {
    let needs_show = {
        let mut picker = state.lock();
        let hidden = !picker.visible;
        picker.visible = true;
        picker.pin_editor();
        hidden
    };
    if needs_show && let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
    publish(&app);
}
#[tauri::command]
fn end_edit(app: tauri::AppHandle, state: State<'_, Runtime>) {
    state.lock().end_editor();
    publish(&app);
}
#[tauri::command]
fn edit_register(
    app: tauri::AppHandle,
    state: State<'_, Runtime>,
    register: char,
    name: String,
    text: String,
) -> Result<(), String> {
    service::validate_edit(register, &name, &text)?;
    state.send(Command::EditRegister {
        register,
        name,
        text,
    })?;
    state.lock().clear_selection();
    publish(&app);
    Ok(())
}
#[tauri::command]
fn clear_register(
    app: tauri::AppHandle,
    state: State<'_, Runtime>,
    register: char,
) -> Result<(), String> {
    clipforge::core::register_index(register).map_err(|e| e.to_string())?;
    state.send(Command::ClearRegister(register))?;
    state.lock().clear_selection();
    publish(&app);
    Ok(())
}
#[tauri::command]
fn save_current(state: State<'_, Runtime>, register: char) -> Result<(), String> {
    clipforge::core::register_index(register).map_err(|e| e.to_string())?;
    state.send(Command::SaveCurrent(register))
}
#[tauri::command]
fn select_register(
    app: tauri::AppHandle,
    state: State<'_, Runtime>,
    register: char,
) -> Result<(), String> {
    clipforge::core::register_index(register).map_err(|e| e.to_string())?;
    let (copy, target) = {
        let p = state.lock();
        (p.copy, p.target)
    };
    state.send(Command::Select {
        register,
        copy,
        target,
    })?;
    hide(&app, false)
}
#[tauri::command]
fn navigate(app: tauri::AppHandle, state: State<'_, Runtime>, backwards: bool) {
    state.lock().navigate(backwards);
    publish(&app);
}
#[tauri::command]
fn dismiss(app: tauri::AppHandle, commit: bool) -> Result<(), String> {
    hide(&app, commit)
}
#[tauri::command]
fn dismiss_on_blur(app: tauri::AppHandle) -> Result<(), String> {
    // A pending autosave may finish after the user has returned to the menu.
    if let Some(window) = app.get_webview_window("main")
        && !window.is_focused().map_err(|e| e.to_string())?
    {
        hide_with_focus(&app, false, false)?;
    }
    Ok(())
}
#[tauri::command]
fn clear_history(app: tauri::AppHandle, state: State<'_, Runtime>) -> Result<(), String> {
    state.lock().clear_selection();
    state.send(Command::ClearHistory)?;
    publish(&app);
    Ok(())
}
#[tauri::command]
fn clear_all(app: tauri::AppHandle, state: State<'_, Runtime>) -> Result<(), String> {
    state.lock().clear_selection();
    state.send(Command::Clear)?;
    publish(&app);
    Ok(())
}
#[tauri::command]
fn quit(app: tauri::AppHandle) {
    app.exit(0);
}

#[tauri::command]
async fn save_hotkeys(app: tauri::AppHandle, hotkeys: Hotkeys) -> Result<Hotkeys, String> {
    let hotkeys = hotkeys.normalized()?;
    #[cfg(not(windows))]
    let handle = app.clone();
    let configure = move || -> Result<Hotkeys, String> {
        let state = app.state::<Runtime>();
        let _guard = state.settings_update.lock().map_err(|e| e.to_string())?;
        let path = state
            .settings_path
            .lock()
            .map_err(|e| e.to_string())?
            .clone()
            .ok_or("Hotkey settings are unavailable in smoke mode")?;
        platform::configure(&hotkeys, &path)?;
        {
            let mut picker = state.lock();
            picker.hotkeys = hotkeys.clone();
            picker.status = "Hotkeys saved".into();
        }
        publish(&app);
        Ok(hotkeys)
    };
    #[cfg(windows)]
    {
        tauri::async_runtime::spawn_blocking(configure)
            .await
            .map_err(|e| e.to_string())?
    }
    #[cfg(not(windows))]
    {
        let (reply, result) = mpsc::channel();
        handle
            .run_on_main_thread(move || {
                let _ = reply.send(configure());
            })
            .map_err(|e| e.to_string())?;
        tauri::async_runtime::spawn_blocking(move || result.recv().map_err(|e| e.to_string())?)
            .await
            .map_err(|e| e.to_string())?
    }
}

fn open_from_tray(app: &tauri::AppHandle) {
    if app.state::<Runtime>().lock().visible {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.show();
            let _ = window.set_focus();
        }
    } else {
        show(app, false, platform::target());
    }
}
fn create_tray(app: &tauri::App) -> tauri::Result<()> {
    use tauri::{
        menu::{Menu, MenuItem},
        tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    };
    let open = MenuItem::with_id(app, "open", "Open ClipForge", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    TrayIconBuilder::with_id("clipforge")
        .icon(tauri::include_image!("icons/icon.png"))
        .tooltip("ClipForge — clipboard registers and history")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                open_from_tray(tray.app_handle());
            }
        })
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => open_from_tray(app),
            "quit" => {
                if app.state::<Runtime>().lock().pinned {
                    let _ = app.emit("clipforge-quit", ());
                } else {
                    app.exit(0);
                }
            }
            _ => {}
        })
        .build(app)?;
    Ok(())
}

fn main() {
    let smoke_background = std::env::args().any(|arg| arg == "--smoke-background");
    let smoke_ui = smoke_background || std::env::args().any(|arg| arg == "--smoke-ui");
    let manual = std::env::args().any(|arg| arg == "--show");
    if smoke_ui {
        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_secs(20));
            smoke_report("FAILED: Tauri webview smoke test exceeded 20 seconds");
            std::process::exit(1);
        });
    }
    #[cfg(windows)]
    let _instance = if smoke_ui {
        None
    } else {
        match platform::acquire_instance() {
            Ok(instance) => Some(instance),
            Err(message) => {
                platform::startup_message(&message);
                eprintln!("{message}");
                return;
            }
        }
    };
    let (tx, commands) = mpsc::channel();
    let (updates, raw_updates) = mpsc::channel();
    let (events_tx, events_rx) = mpsc::channel();
    let target = platform::target();
    let initially_visible = (smoke_ui && !smoke_background) || manual;
    let mut picker = Picker::new(target);
    if smoke_ui {
        picker.current_clipboard =
            Some("A recent clipboard entry\nDrag here or edit to change the host clipboard".into());
        picker
            .engine
            .save_register('x', "ssh dev@lab-host\nA command for your VM")
            .unwrap();
        picker
            .engine
            .save_register(
                'k',
                "Keep this snippet handy\nSecond line of a named register",
            )
            .unwrap();
        picker
            .engine
            .observe("A recent clipboard entry\nDrag onto a register to save the complete text");
    }
    let event_commands = tx.clone();
    let immediate_updates = updates.clone();
    std::thread::spawn(move || {
        while let Ok(event) = events_rx.recv() {
            let immediate = match event {
                platform::Event::Show {
                    copy: false,
                    target,
                } => Some(Update::Show {
                    copy: false,
                    target,
                }),
                platform::Event::Toggle { target } => Some(Update::Toggle { target }),
                platform::Event::Dismiss { target, commit } => {
                    Some(Update::Dismiss { target, commit })
                }
                _ => None,
            };
            if let Some(update) = immediate {
                let _ = immediate_updates.send(update);
            } else if event_commands.send(Command::Platform(event)).is_err() {
                break;
            }
        }
    });
    if !smoke_ui {
        std::thread::spawn(move || service::run(commands, updates));
    }
    let runtime = Runtime {
        settings_path: Mutex::new(None),
        settings_update: Mutex::new(()),
        picker: Mutex::new(picker),
        tx,
        smoke: smoke_ui,
        frontend_ready: AtomicBool::new(false),
    };
    let result = tauri::Builder::default()
        .manage(runtime)
        .invoke_handler(tauri::generate_handler![
            snapshot,
            load_text,
            set_clipboard,
            save_text,
            save_current,
            begin_edit,
            end_edit,
            edit_register,
            clear_register,
            select_register,
            navigate,
            dismiss,
            dismiss_on_blur,
            clear_all,
            clear_history,
            save_hotkeys,
            quit
        ])
        .setup(move |app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            let mut builder = tauri::WebviewWindowBuilder::from_config(
                app.handle(),
                &app.config().app.windows[0],
            )?;
            if smoke_ui {
                let executable = std::env::current_exe()?;
                let profile = executable
                    .parent()
                    .ok_or("Could not locate smoke executable directory")?
                    .join("webview-smoke");
                builder = builder.data_directory(profile);
            }
            builder.build()?;

            let mut startup_error = false;
            if !smoke_ui {
                let path = app.path().app_config_dir()?.join("hotkeys.json");
                let state = app.state::<Runtime>();
                let hotkeys = match settings::load(&path) {
                    Ok(keys) => keys,
                    Err(error) => {
                        state.lock().status = error;
                        startup_error = true;
                        Hotkeys::default()
                    }
                };
                state.lock().hotkeys = hotkeys.clone();
                *state.settings_path.lock().unwrap() = Some(path);
                if let Err(error) = platform::start(events_tx, &hotkeys) {
                    let mut picker = state.lock();
                    if !picker.status.is_empty() {
                        picker.status.push_str("; ");
                    }
                    picker.status.push_str(&error);
                    startup_error = true;
                }
                if let Err(error) = create_tray(app) {
                    state.lock().status = format!("Could not create ClipForge tray icon: {error}");
                    startup_error = true;
                }
            }

            if smoke_ui {
                create_tray(app)?;
                if app.tray_by_id("clipforge").is_none() {
                    return Err("ClipForge tray icon was not retained".into());
                }
                smoke_report("PASS: ClipForge tray icon and menu created");
                smoke_report("PASS: Tauri webview created");
            }
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                while let Ok(update) = raw_updates.recv() {
                    let ui = handle.clone();
                    if handle
                        .run_on_main_thread(move || apply_update(&ui, update))
                        .is_err()
                    {
                        break;
                    }
                }
            });
            if initially_visible || startup_error {
                show(app.handle(), false, target);
            }
            if smoke_ui {
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(500));
                    if smoke_background {
                        let ui = handle.clone();
                        let _ = handle.run_on_main_thread(move || {
                            let visible = ui
                                .get_webview_window("main")
                                .is_some_and(|w| w.is_visible().unwrap_or(true));
                            if visible {
                                smoke_report("FAILED: Tauri picker appeared at startup");
                                ui.exit(1);
                                return;
                            }
                            apply_update(&ui, Update::Status("Background test error".into()));
                        });
                    }
                    std::thread::sleep(Duration::from_secs(3));
                    let ui = handle.clone();
                    let _ = handle.run_on_main_thread(move || {
                        let state = ui.state::<Runtime>();
                        let ready = state.frontend_ready.load(Ordering::Relaxed);
                        let hidden = !ui
                            .get_webview_window("main")
                            .is_some_and(|w| w.is_visible().unwrap_or(true));
                        if smoke_background && hidden {
                            smoke_report("PASS: Tauri startup and error status remain hidden");
                        }
                        ui.exit(if ready && (!smoke_background || hidden) {
                            0
                        } else {
                            smoke_report("FAILED: Tauri frontend initialization or visibility");
                            1
                        });
                    });
                });
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            match event {
                tauri::WindowEvent::CloseRequested { api, .. } => {
                    api.prevent_close();
                    let app = window.app_handle();
                    if app.state::<Runtime>().lock().pinned {
                        let _ = app.emit("clipforge-toggle", ());
                    } else {
                        let _ = hide(app, false);
                    }
                }
                tauri::WindowEvent::Focused(false) => {
                    let app = window.app_handle();
                    let (visible, pinned) = {
                        let state = app.state::<Runtime>();
                        let picker = state.lock();
                        (picker.visible, picker.pinned)
                    };
                    if visible {
                        if pinned {
                            let _ = app.emit("clipforge-blur", ());
                        } else {
                            // Leave focus with the application the user clicked.
                            let _ = hide_with_focus(app, false, false);
                        }
                    }
                }
                _ => {}
            }
        })
        .run(tauri::generate_context!());
    if let Err(error) = result {
        eprintln!("ClipForge startup failed: {error}");
        std::process::exit(1);
    }
}
