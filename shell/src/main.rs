#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_state;
mod auth;
mod collections;
mod content;
mod home_layout;
mod ipc;
mod library;
mod metadata;
mod player;
mod progress;
mod settings;
mod skip_segments;
mod thumbnail;

use std::sync::{Arc, Mutex};

use app_state::AppState;
use serde_json::Value;
use tauri::{Manager, State, Window, WindowEvent};

type SharedState = Arc<Mutex<AppState>>;

/// The whole bridge is one command: the UI keeps speaking the existing
/// `{id, method, params}` envelope and gets back the batch of messages that
/// request produced. Splitting `ipc::handle`'s dispatch into forty typed Tauri
/// commands would touch every call site without changing behaviour.
///
/// Tauri runs synchronous commands on its own thread pool, so the blocking
/// Supabase and addon HTTP work inside `ipc::handle` does not stall the UI —
/// this replaces the manual `std::thread::spawn` plus `EventLoopProxy` hop the
/// wry version needed.
#[tauri::command]
fn bridge(raw: String, window: Window, state: State<'_, SharedState>) -> Vec<Value> {
    if let Ok(request) = serde_json::from_str::<ipc::RequestEnvelope>(&raw) {
        if request.method == "window.setFullscreen" {
            let enabled = request
                .params
                .get("enabled")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let outcome = window
                .set_fullscreen(enabled)
                .map(|()| serde_json::json!({ "fullscreen": enabled }));
            return vec![to_value(ipc::OutboundMessage::Response(match outcome {
                Ok(result) => ipc::ResponseEnvelope {
                    id: request.id,
                    ok: true,
                    result: Some(result),
                    error: None,
                },
                Err(error) => ipc::ResponseEnvelope {
                    id: request.id,
                    ok: false,
                    result: None,
                    error: Some(ipc::ErrorBody {
                        code: "window_command_failed",
                        message: error.to_string(),
                    }),
                },
            }))];
        }
    }

    let shared = state.inner();
    let messages = match ipc::handle_content_shared(&raw, shared) {
        Some(messages) => messages,
        None => match shared.lock() {
            Ok(mut state) => ipc::handle(&raw, &mut state),
            Err(_) => return Vec::new(),
        },
    };
    messages.into_iter().map(to_value).collect()
}

fn to_value(message: ipc::OutboundMessage) -> Value {
    match message {
        ipc::OutboundMessage::Response(response) => serde_json::to_value(response),
        ipc::OutboundMessage::Event(event) => serde_json::to_value(event),
    }
    .unwrap_or(Value::Null)
}

fn main() {
    tauri::Builder::default()
        .manage(SharedState::default())
        .invoke_handler(tauri::generate_handler![bridge])
        .setup(|app| {
            // mpv renders into a child of the top-level window, underneath the
            // transparent webview, so the player needs that window's handle
            // before any playback can start.
            #[cfg(windows)]
            {
                let window = app
                    .get_webview_window("main")
                    .ok_or("the main window is missing")?;
                let hwnd = window.hwnd()?.0 as isize;
                let state: State<'_, SharedState> = app.state();
                if let Ok(mut state) = state.lock() {
                    state.player.configure_window(hwnd);
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if matches!(event, WindowEvent::CloseRequested { .. }) {
                // libmpv holds a child window and a decode thread; dropping the
                // process without stopping it can wedge on exit.
                let state: State<'_, SharedState> = window.state();
                if let Ok(mut state) = state.lock() {
                    state.player.stop();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("failed to start the Nuvio shell");
}
