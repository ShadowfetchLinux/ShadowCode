#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use anyhow::{Context, Result};
use serde_json::{json, Value};
use shadowcode_core::{
    config::Config,
    paths::{self, AppPaths},
    service::{Request, Service},
};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
};
use tauri::{Emitter, Manager};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;

#[derive(Default)]
struct Lifecycle {
    requested: AtomicBool,
    complete: AtomicBool,
}

#[tauri::command]
async fn api(
    request: Request,
    service: tauri::State<'_, Service>,
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
    service: tauri::State<'_, Service>,
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
    let service = app.state::<Service>().inner().clone();
    let _ = app.emit("shadowcode:shutdown", json!({"status":"closing"}));
    tauri::async_runtime::spawn(async move {
        match service.engine.shutdown().await {
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
    let mut profile = None;
    let mut workspace = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--version" | "-V" => {
                println!(
                    "ShadowCode {} · native Rust desktop",
                    shadowcode_core::VERSION
                );
                return Ok(());
            }
            "--help" | "-h" => {
                println!("ShadowCode {}\n\nUsage: shadowcode [--workspace PATH] [--profile PATH]\n\n  --workspace PATH  Open this project\n  --profile PATH    Use an isolated data profile\n  --version         Print version\n  --help            Show help",shadowcode_core::VERSION);
                return Ok(());
            }
            "--profile" => {
                profile = Some(PathBuf::from(
                    args.next().context("--profile requires a path")?,
                ))
            }
            "--workspace" => {
                workspace = Some(PathBuf::from(
                    args.next().context("--workspace requires a path")?,
                ))
            }
            _ => anyhow::bail!("Unknown argument: {arg}. Use --help for available options."),
        }
    }
    let isolated = profile.is_some();
    let paths = match profile {
        Some(path) => AppPaths::isolated(&path)?,
        None => AppPaths::discover()?,
    };
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
            let service = Service::open(paths, workspace)?;
            let mut events = service.engine.subscribe();
            app.manage(service.clone());
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
            #[cfg(unix)]
            {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    if let Ok(mut terminate) =
                        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    {
                        tokio::select! {_=tokio::signal::ctrl_c()=>{},_=terminate.recv()=>{}}
                        request_shutdown(&handle);
                    }
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
