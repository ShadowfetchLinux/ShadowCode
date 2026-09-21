#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use anyhow::{Context, Result};
use serde_json::{json, Value};
use shadowcode_core::{
    cli,
    config::Config,
    paths::{self},
    service::Request,
};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
};
use tauri::{Emitter, Manager};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;

mod backend;
use backend::Backend;

#[derive(Default)]
struct Lifecycle {
    requested: AtomicBool,
    complete: AtomicBool,
}

#[tauri::command]
async fn api(
    request: Request,
    service: tauri::State<'_, Backend>,
) -> std::result::Result<Value, String> {
    service
        .dispatch(request)
        .await
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
async fn pick_directory(app: tauri::AppHandle) -> std::result::Result<Option<PathBuf>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut picker = app.dialog().file().set_title("Open a project folder");
        if let Some(window) = app.get_webview_window("main") {
            picker = picker.set_parent(&window);
        }
        picker
            .blocking_pick_folder()
            .map(|p| p.into_path())
            .transpose()
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn export_session(
    app: tauri::AppHandle,
    service: tauri::State<'_, Backend>,
    session_id: String,
    format: String,
) -> std::result::Result<Option<String>, String> {
    if !matches!(format.as_str(), "md" | "json")
        || !session_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Err("Invalid export request".into());
    }
    let export = service
        .dispatch(Request {
            method: "GET".into(),
            path: format!("/api/sessions/{session_id}/export?format={format}"),
            body: Value::Null,
        })
        .await
        .map_err(|e| format!("{e:#}"))?;
    tauri::async_runtime::spawn_blocking(move || -> std::result::Result<_, String> {
        let filename = export["filename"]
            .as_str()
            .ok_or("Export filename missing")?;
        let mut picker = app
            .dialog()
            .file()
            .set_title("Export conversation")
            .set_file_name(filename)
            .add_filter("Conversation", &[format.as_str()]);
        if let Some(window) = app.get_webview_window("main") {
            picker = picker.set_parent(&window);
        }
        let Some(file) = picker.blocking_save_file() else {
            return Ok(None);
        };
        let path = file.into_path().map_err(|e| e.to_string())?;
        let content = export["content"].as_str().ok_or("Export content missing")?;
        paths::atomic_write(&path, content.as_bytes(), false)
            .map_err(|e| format!("Could not save export: {e:#}"))?;
        Ok(Some(path.to_string_lossy().into_owned()))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn open_external(app: tauri::AppHandle, url: String) -> std::result::Result<(), String> {
    let parsed = tauri::Url::parse(&url).map_err(|e| e.to_string())?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err("Only HTTP and HTTPS links can be opened externally".into());
    }
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn desktop_quit(app: tauri::AppHandle) {
    request_shutdown(&app);
}

fn request_shutdown(app: &tauri::AppHandle) {
    let state = app.state::<Lifecycle>();
    if state.requested.swap(true, Ordering::AcqRel) {
        return;
    }
    let app = app.clone();
    let attached = matches!(app.state::<Backend>().inner(), Backend::Attached(_));
    let _ = app.emit("shadowcode:shutdown", json!({"status":"closing", "message": if attached { "Closing this window. Work continues in the shared engine." } else { "Stopping managed work and closing the engine…" }}));
    tauri::async_runtime::spawn(async move {
        match app.state::<Backend>().close().await {
            Ok(()) => {
                app.state::<Lifecycle>()
                    .complete
                    .store(true, Ordering::Release);
                app.exit(0);
            }
            Err(error) => {
                app.state::<Lifecycle>()
                    .requested
                    .store(false, Ordering::Release);
                let message = format!("{error:#}");
                let _ = app.emit(
                    "shadowcode:shutdown",
                    json!({"status":"error","message":message}),
                );
                eprintln!("Shutdown still needs attention: {message}");
            }
        }
    });
}

fn main() {
    if let Err(error) = run() {
        eprintln!("ShadowCode: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let extraction_parent = shadowcode_core::lifecycle::extraction_parent();
    let options = cli::Options::parse_args();
    if !options.desktop() {
        let json_output = options.json && !options.mcp_stdio();
        let events_output = options.events();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;
        let result = runtime.block_on(cli::run(options));
        runtime.shutdown_timeout(std::time::Duration::from_secs(2));
        let code = match result {
            Ok(code) => code,
            Err(error) => {
                if json_output || events_output {
                    use std::io::Write;
                    let value = json!({"ok":false,"error":format!("{error:#}")});
                    let value = if events_output {
                        json!({"type":"result","exit_code":1,"result":value})
                    } else {
                        value
                    };
                    let _ = writeln!(std::io::stdout(), "{value}");
                } else {
                    use std::io::Write;
                    let _ = writeln!(std::io::stderr(), "ShadowCode: {error:#}");
                }
                1
            }
        };
        std::process::exit(code);
    }
    let isolated = options.profile.is_some();
    let paths = options.paths()?;
    let workspace = options.workspace;
    let mut builder = tauri::Builder::default();
    if !isolated {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }));
    }
    builder = builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init());
    if !isolated {
        builder = builder.plugin(tauri_plugin_window_state::Builder::default().build());
    }
    let app = builder
        .manage(Lifecycle::default())
        .invoke_handler(tauri::generate_handler![
            api,
            pick_directory,
            export_session,
            open_external,
            desktop_quit
        ])
        .setup(move |app| {
            let webview_data = paths.data.join("webview");
            let backend = tauri::async_runtime::block_on(Backend::open(paths, workspace))?;
            let local_service = match &backend {
                Backend::Owned { service, .. } => Some(service.clone()),
                Backend::Attached(_) => None,
            };
            app.manage(backend);
            let config = app
                .config()
                .app
                .windows
                .first()
                .context("Window configuration missing")?;
            tauri::WebviewWindowBuilder::from_config(app, config)?
                .data_directory(webview_data)
                .on_navigation(|url| {
                    (url.scheme() == "tauri" && url.host_str() == Some("localhost"))
                        || (matches!(url.scheme(), "http" | "https")
                            && url.host_str() == Some("tauri.localhost"))
                        || (cfg!(debug_assertions)
                            && url.scheme() == "http"
                            && url.host_str() == Some("127.0.0.1")
                            && url.port() == Some(5174))
                })
                .build()?;
            let handle = app.handle().clone();
            if let Some(service) = local_service {
                let mut events = service.engine.subscribe();
                tauri::async_runtime::spawn(async move {
                    loop {
                        match events.recv().await {
                            Ok(event) => {
                                // Broadcast is only a wakeup. The UI fetches committed
                                // rows in SQLite cursor order, including after lag.
                                let _ = handle.emit(
                                    "shadowcode:events",
                                    json!({"session_id":event["session_id"]}),
                                );
                                if event["type"] == "agent.completed" {
                                    let enabled = Config::load(service.engine.paths(), None)
                                        .ok()
                                        .is_some_and(|c| c.ui["notify"].as_bool().unwrap_or(true));
                                    let focused = handle
                                        .get_webview_window("main")
                                        .is_some_and(|w| w.is_focused().unwrap_or(false));
                                    if enabled && !focused {
                                        let summary = event["payload"]["summary"]
                                            .as_str()
                                            .unwrap_or("Task finished");
                                        let _ = handle
                                            .notification()
                                            .builder()
                                            .title("ShadowCode · task finished")
                                            .body(summary.chars().take(180).collect::<String>())
                                            .show();
                                    }
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                                let _ = handle.emit("shadowcode:events", json!({}));
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        }
                    }
                });
            } else {
                // Attached windows have no in-process broadcast subscription.
                // Wake the existing durable reader; it fetches only while a job
                // is selected and active. No event payload or SQLite access is
                // duplicated in this process.
                tauri::async_runtime::spawn(async move {
                    let mut tick = tokio::time::interval(std::time::Duration::from_millis(250));
                    loop {
                        tick.tick().await;
                        if handle
                            .state::<Lifecycle>()
                            .requested
                            .load(Ordering::Acquire)
                        {
                            break;
                        }
                        let _ = handle.emit("shadowcode:events", json!({}));
                    }
                });
            }
            #[cfg(unix)]
            {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    shadowcode_core::lifecycle::interrupted(extraction_parent).await;
                    request_shutdown(&handle);
                });
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                request_shutdown(window.app_handle());
            }
        })
        .build(tauri::generate_context!())?;
    app.run(|app, event| {
        if let tauri::RunEvent::ExitRequested { api, .. } = event {
            if !app.state::<Lifecycle>().complete.load(Ordering::Acquire) {
                api.prevent_exit();
                request_shutdown(app);
            }
        }
    });
    Ok(())
}
