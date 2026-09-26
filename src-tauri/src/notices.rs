//! Desktop notifications for engine events. `shadowcode_core::notify`
//! decides which events deserve one (and the ui settings that turn them
//! off); this module checks the window's focus and open conversation, shows
//! the notification and, when it is clicked, focuses the window and asks the
//! interface to open that conversation (`shadowcode:open-session`).
use serde_json::{json, Value};
use shadowcode_core::{config::Config, notify, paths::AppPaths};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};

/// The conversation the window shows (`set_visible_session`).
#[derive(Default)]
pub struct Visible(pub Mutex<String>);

#[tauri::command]
pub fn set_visible_session(state: tauri::State<'_, Visible>, session_id: String) {
    if let Ok(mut visible) = state.0.lock() {
        *visible = session_id.chars().take(128).collect();
    }
}

/// Conversations of automation runs still going (newest last, bounded):
/// their task's `agent.completed` is announced by `automation.finished`.
static AUTOMATION_SESSIONS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Called for every engine broadcast.
pub fn on_event(handle: &AppHandle, paths: &AppPaths, event: &Value) {
    if let Some(session) = notify::automation_session(event) {
        if let Ok(mut sessions) = AUTOMATION_SESSIONS.lock() {
            sessions.retain(|s| s != session);
            if event["type"] == "automation.started" {
                sessions.push(session.to_owned());
                let excess = sessions.len().saturating_sub(64);
                sessions.drain(..excess);
                return;
            }
        }
    }
    if event["type"] == "agent.completed" {
        let session = event["session_id"].as_str().unwrap_or("");
        if AUTOMATION_SESSIONS
            .lock()
            .is_ok_and(|sessions| sessions.iter().any(|s| s == session))
        {
            return;
        }
    }
    // Cheap check first: most events never notify.
    if notify::select(event, &notify::Prefs::default()).is_none() {
        return;
    }
    let prefs = Config::load(paths, None)
        .map(|config| notify::Prefs::from_ui(&config.ui))
        .unwrap_or_default();
    let Some(notice) = notify::select(event, &prefs) else {
        return;
    };
    let focused = handle
        .get_webview_window("main")
        .is_some_and(|w| w.is_focused().unwrap_or(false) && w.is_visible().unwrap_or(false));
    let visible = handle
        .state::<Visible>()
        .0
        .lock()
        .map(|v| v.clone())
        .unwrap_or_default();
    if notify::should_show(&notice, focused, &visible) {
        show(handle, notice, prefs.sound);
    }
}

fn open(handle: &AppHandle, session_id: &str) {
    if let Some(window) = handle.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
    if !session_id.is_empty() {
        let _ = handle.emit(
            "shadowcode:open-session",
            json!({ "session_id": session_id }),
        );
    }
}

#[cfg(target_os = "linux")]
fn show(handle: &AppHandle, notice: notify::Notice, sound: bool) {
    let handle = handle.clone();
    // Waiting for the click blocks until the notification closes.
    tauri::async_runtime::spawn_blocking(move || {
        let mut builder = notify_rust::Notification::new();
        builder
            .appname("ShadowCode")
            .summary(&notice.title)
            .body(&notice.body)
            .icon("shadowcode")
            .action("default", "Open");
        if sound {
            builder.sound_name("message-new-instant");
        }
        if matches!(
            notice.kind,
            notify::Kind::Approval | notify::Kind::ApprovalExpiring
        ) {
            builder.urgency(notify_rust::Urgency::Critical);
        }
        match builder.show() {
            Ok(shown) => shown.wait_for_action(|action| {
                if action == "default" || action == "Open" {
                    open(&handle, &notice.session_id);
                }
            }),
            Err(error) => eprintln!("Notification not shown: {error}"),
        }
    });
}

#[cfg(not(target_os = "linux"))]
fn show(handle: &AppHandle, notice: notify::Notice, _sound: bool) {
    use tauri_plugin_notification::NotificationExt;
    let _ = open;
    let _ = handle
        .notification()
        .builder()
        .title(notice.title)
        .body(notice.body)
        .show();
}
