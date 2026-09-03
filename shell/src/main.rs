#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_state;
mod auth;
mod collections;
mod content;
mod downloads;
mod home_layout;
mod http;
mod ipc;
mod library;
mod metadata;
mod paths;
mod player;
mod progress;
mod settings;
mod skip_segments;
mod thumbnail;
mod updates;
mod watch_sync;

use std::sync::{Arc, Mutex};

use app_state::AppState;
use serde_json::Value;
use tauri::{AppHandle, Manager, State, Window, WindowEvent, window::Color};

/// The app background, matching the UI's own `--app-bg`.
///
/// A transparent window with nothing behind it shows the desktop through the
/// gaps, so the page must be given a colour to paint rather than simply being
/// left alpha.
const OPAQUE_BACKGROUND: Color = Color(8, 10, 13, 255);

type SharedState = Arc<Mutex<AppState>>;

/// The whole bridge is one command: the UI keeps speaking the existing
/// `{id, method, params}` envelope and gets back the batch of messages that
/// request produced. Splitting `ipc::handle`'s dispatch into forty typed Tauri
/// commands would touch every call site without changing behaviour.
///
/// The explicit async command mode forces this synchronous handler onto
/// Tauri's worker pool. The blocking operations below therefore cannot stall
/// the UI thread.
#[tauri::command]
async fn bridge(raw: String, window: Window, app: AppHandle) -> Result<Vec<Value>, String> {
    if let Ok(request) = serde_json::from_str::<ipc::RequestEnvelope>(&raw)
        && matches!(request.method.as_str(), "player.prepare" | "player.stop")
    {
        // Transparency is only wanted while something is playing behind the
        // page. It is cheap to ask for and expensive to have: a permanently
        // transparent webview composites with alpha, which drops WebView2 off
        // its fast path and made scrolling the library visibly slower than the
        // same UI in a browser. The window is still created transparent —
        // Windows decides that at creation — but it paints opaque until mpv is
        // actually behind it.
        //
        // Deliberately not an early return: the request still has to reach the
        // player below, so this only sets the colour on the way past.
        //
        // It has to be the *webview's* background, not the window's. mpv draws
        // into a child of the top-level window and is revealed by the webview
        // painting alpha over it, so clearing the native window's colour did
        // nothing to the sheet actually covering the video — playback came up
        // as a grey rectangle, which is the opaque webview, not a broken
        // player.
        if let Some(webview) = app.get_webview_window("main") {
            let _ = webview.set_background_color(if request.method == "player.stop" {
                Some(OPAQUE_BACKGROUND)
            } else {
                None
            });
        }
    }
    if let Ok(request) = serde_json::from_str::<ipc::RequestEnvelope>(&raw)
        && request.method == "window.setFullscreen"
    {
        let enabled = request
            .params
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let outcome = window
            .set_fullscreen(enabled)
            .map(|()| serde_json::json!({ "fullscreen": enabled }));
        return Ok(vec![to_value(ipc::OutboundMessage::Response(
            match outcome {
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
            },
        ))]);
    }

    let shared = Arc::clone(app.state::<SharedState>().inner());
    Ok(
        tauri::async_runtime::spawn_blocking(move || dispatch_bridge(&raw, &shared))
            .await
            .map_err(|error| format!("native bridge task failed: {error}"))?,
    )
}

/// Account restoration, addon calls, settings sync and thumbnail extraction
/// all use blocking APIs. Running them directly in Tauri's async command
/// context can panic when a dependency (notably Windows Credential Manager)
/// tears down its own runtime. Keep the async bridge responsive and isolate
/// that work on the runtime's blocking pool.
fn dispatch_bridge(raw: &str, shared: &SharedState) -> Vec<Value> {
    let messages = match ipc::handle_updates_shared(raw) {
        Some(messages) => messages,
        None => match ipc::handle_player_shared(&raw, shared) {
            Some(messages) => messages,
            None => match ipc::handle_downloads_shared(&raw, shared) {
                Some(messages) => messages,
                None => match ipc::handle_content_shared(&raw, shared) {
                    Some(messages) => messages,
                    None => match shared.lock() {
                        Ok(mut state) => ipc::handle(&raw, &mut state),
                        Err(_) => return Vec::new(),
                    },
                },
            },
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
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(SharedState::default())
        .invoke_handler(tauri::generate_handler![bridge])
        .setup(|app| {
            {
                let state: State<'_, SharedState> = app.state();
                if let Ok(state) = state.lock() {
                    downloads::resume_queue(&state.downloads);
                }
            }
            // mpv renders into a child of the top-level window, underneath the
            // transparent webview, so the player needs that window's handle
            // before any playback can start.
            #[cfg(windows)]
            {
                let window = app
                    .get_webview_window("main")
                    .ok_or("the main window is missing")?;
                // The bundle ICO is still used by Explorer/the executable,
                // while the live window gets the full-resolution PNG. Letting
                // Windows choose a small ICO frame here made the title/taskbar
                // icon visibly pixelated at scaled desktop DPIs.
                window.set_icon(tauri::image::Image::from_bytes(include_bytes!(
                    "../assets/Nuvio-icon.png"
                ))?)?;
                // Opaque until playback needs otherwise. See the note in
                // `bridge`: this is what keeps ordinary browsing off WebView2's
                // alpha compositing path.
                window.set_background_color(Some(OPAQUE_BACKGROUND))?;
                let hwnd = window.hwnd()?.0 as isize;
                let state: State<'_, SharedState> = app.state();
                if let Ok(state) = state.lock()
                    && let Ok(mut player) = state.player.lock()
                {
                    player.configure_window(hwnd);
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // Nuvio pulls whenever the app comes to the foreground
            // (AppForegroundMonitor → requestForegroundPull), which is why a
            // change made elsewhere shows up there within a second or two of
            // switching to it. Regaining focus is the desktop equivalent.
            //
            // This callback runs on Tauri's window event loop. Never wait for
            // AppState here: account restoration and profile refresh perform
            // network work while holding that mutex. Waiting for it from a
            // Focused event stalls Windows' non-client event processing too,
            // which makes the title bar refuse to drag while the app loads.
            if matches!(event, WindowEvent::Focused(true)) {
                let shared = Arc::clone(window.state::<SharedState>().inner());
                let app = window.app_handle().clone();
                let _ = tauri::async_runtime::spawn_blocking(move || {
                    if let Ok(mut state) = shared.lock() {
                        // Expire the cache; the next read re-pulls.
                        state.settings_loaded_at = None;
                    }
                    // The bridge only delivers events alongside a command
                    // response, so nudge the UI directly to re-read after the
                    // state is ready. `eval` lives on the webview rather than
                    // the window this closure receives.
                    if let Some(webview) = app.get_webview_window("main") {
                        let _ = webview.eval(
                            "window.__NUVIO_BRIDGE_DELIVER__ && window.__NUVIO_BRIDGE_DELIVER__({event:'sync.foreground',payload:{}});",
                        );
                    }
                });
            }
            if matches!(event, WindowEvent::CloseRequested { .. }) {
                // Never wait for account/bootstrap I/O from the Windows close
                // callback. This callback is on the native event loop, so a
                // blocking AppState lock made the title-bar X appear frozen
                // until whichever Supabase/addon request held it completed.
                //
                // When the state is immediately available, hand libmpv cleanup
                // to a detached worker as well: PlayerService::stop joins the
                // decode thread and mpv_destroy may take a moment on a network
                // stream, but neither operation is required before the window
                // itself can disappear. Process teardown remains the fallback
                // if a request currently owns AppState.
                let state: State<'_, SharedState> = window.state();
                if let Ok(state) = state.try_lock() {
                    let player = Arc::clone(&state.player);
                    drop(state);
                    let _ = std::thread::Builder::new()
                        .name("nuvio-player-shutdown".to_string())
                        .spawn(move || {
                            if let Ok(mut player) = player.lock() {
                                player.stop();
                            }
                        });
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("failed to start the Nuvio shell");
}
