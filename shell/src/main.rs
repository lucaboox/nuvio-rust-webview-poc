#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_state;
mod assets;
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

use anyhow::{Context, Result};
use app_state::AppState;
#[cfg(windows)]
use tao::platform::windows::WindowExtWindows;
use tao::{
    dpi::{LogicalSize, Size},
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder},
    window::{Fullscreen, Icon, WindowBuilder},
};
use wry::WebViewBuilder;
#[cfg(windows)]
use wry::WebViewBuilderExtWindows;

enum UserEvent {
    Ipc(String),
    Deliver(Vec<ipc::OutboundMessage>),
}

fn main() -> Result<()> {
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let ipc_proxy = event_loop.create_proxy();
    let window_icon = load_window_icon()?;
    let window = WindowBuilder::new()
        .with_title("Nuvio · Rust architecture POC")
        .with_window_icon(Some(window_icon))
        .with_inner_size(Size::Logical(LogicalSize::new(1280.0, 820.0)))
        .with_min_inner_size(Size::Logical(LogicalSize::new(840.0, 600.0)))
        .build(&event_loop)
        .context("failed to create native window")?;

    let webview = WebViewBuilder::new()
        .with_default_context_menus(false)
        .with_custom_protocol("nuvio".into(), |_webview_id, request| {
            assets::serve(request)
        })
        .with_ipc_handler(move |request| {
            let _ = ipc_proxy.send_event(UserEvent::Ipc(request.body().clone()));
        })
        .with_devtools(cfg!(debug_assertions))
        .with_transparent(true)
        .with_url("nuvio://localhost/index.html")
        .build(&window)
        .context("failed to create WebView2")?;

    let mut app_state = AppState::default();
    #[cfg(windows)]
    app_state.player.configure_window(window.hwnd());
    let state = Arc::new(Mutex::new(app_state));
    let deliver_proxy = event_loop.create_proxy();
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::UserEvent(UserEvent::Ipc(raw)) => {
                if let Ok(request) = serde_json::from_str::<ipc::RequestEnvelope>(&raw) {
                    if request.method == "window.setFullscreen" {
                        let enabled = request
                            .params
                            .get("enabled")
                            .and_then(serde_json::Value::as_bool)
                            .unwrap_or(false);
                        window.set_fullscreen(enabled.then(|| Fullscreen::Borderless(None)));
                        let _ = deliver_proxy.send_event(UserEvent::Deliver(vec![
                            ipc::OutboundMessage::Response(ipc::ResponseEnvelope {
                                id: request.id,
                                ok: true,
                                result: Some(serde_json::json!({ "fullscreen": enabled })),
                                error: None,
                            }),
                        ]));
                        return;
                    }
                }
                let state = Arc::clone(&state);
                let deliver_proxy = deliver_proxy.clone();
                std::thread::spawn(move || {
                    let messages = match ipc::handle_content_shared(&raw, &state) {
                        Some(messages) => messages,
                        None => match state.lock() {
                            Ok(mut state) => ipc::handle(&raw, &mut state),
                            Err(_) => return,
                        },
                    };
                    let _ = deliver_proxy.send_event(UserEvent::Deliver(messages));
                });
            }
            Event::UserEvent(UserEvent::Deliver(messages)) => {
                for message in messages {
                    match message.to_script() {
                        Ok(script) => {
                            if let Err(error) = webview.evaluate_script(&script) {
                                eprintln!("failed to deliver IPC response: {error}");
                            }
                        }
                        Err(error) => eprintln!("failed to serialize IPC response: {error}"),
                    }
                }
            }
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                window_id,
                ..
            } if window_id == window.id() => {
                if let Ok(mut state) = state.lock() {
                    state.player.stop();
                }
                *control_flow = ControlFlow::Exit;
            }
            _ => {}
        }
    });
}

fn load_window_icon() -> Result<Icon> {
    let image = image::load_from_memory(include_bytes!("../assets/Nuvio-icon.png"))?.into_rgba8();
    let (width, height) = image.dimensions();
    Icon::from_rgba(image.into_raw(), width, height).context("Nuvio icon had invalid RGBA data")
}
