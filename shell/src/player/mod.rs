use serde::Serialize;
use std::sync::{Arc, Mutex};

#[allow(dead_code)]
mod ffi;
#[cfg(windows)]
mod native;

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerState {
    pub active: bool,
    pub loading: bool,
    pub paused: bool,
    pub position_ms: i64,
    pub duration_ms: i64,
    pub volume: i64,
    pub muted: bool,
    pub audio_track: i64,
    pub subtitle_track: i64,
    pub title: String,
    pub error: Option<String>,
}

pub struct PlayerService {
    prepared_media_id: Option<String>,
    parent_hwnd: isize,
    state: Arc<Mutex<PlayerState>>,
    #[cfg(windows)]
    commands: Option<std::sync::mpsc::Sender<native::PlayerCommand>>,
}

impl Default for PlayerService {
    fn default() -> Self {
        Self {
            prepared_media_id: None,
            parent_hwnd: 0,
            state: Arc::new(Mutex::new(PlayerState {
                volume: 100,
                ..Default::default()
            })),
            #[cfg(windows)]
            commands: None,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerCapabilities {
    pub backend: &'static str,
    pub direct_mpv_ready: bool,
    pub integration: &'static str,
}

impl PlayerService {
    pub fn configure_window(&mut self, parent_hwnd: isize) {
        self.parent_hwnd = parent_hwnd;
    }

    pub fn capabilities(&self) -> PlayerCapabilities {
        PlayerCapabilities {
            backend: "embedded native libmpv",
            direct_mpv_ready: cfg!(windows),
            integration: "Rust-owned child HWND inside the main Nuvio window",
        }
    }

    pub fn state(&self) -> PlayerState {
        self.state
            .lock()
            .map(|state| state.clone())
            .unwrap_or_default()
    }

    pub fn prepare(
        &mut self,
        media_id: String,
        url: Option<String>,
        request_headers: Vec<String>,
        start_position_ms: i64,
        on_progress: Box<dyn Fn(i64, i64) + Send + 'static>,
    ) -> anyhow::Result<String> {
        let url = url.filter(|value| value.starts_with("http://") || value.starts_with("https://"))
            .ok_or_else(|| anyhow::anyhow!("This source is not a direct HTTP stream. Torrent/debrid resolution is not ported yet."))?;
        anyhow::ensure!(self.parent_hwnd != 0, "main window handle is unavailable");
        self.stop();
        self.prepared_media_id = Some(media_id.clone());
        if let Ok(mut state) = self.state.lock() {
            *state = PlayerState {
                active: true,
                loading: true,
                volume: 100,
                title: media_id,
                ..Default::default()
            };
        }
        #[cfg(windows)]
        {
            self.commands = Some(native::launch(
                self.parent_hwnd,
                url,
                request_headers,
                start_position_ms,
                Arc::clone(&self.state),
                on_progress,
            )?);
        }
        #[cfg(not(windows))]
        anyhow::bail!("Direct libmpv playback is currently implemented for Windows only");
        Ok("Playing inside the main Nuvio window".to_string())
    }

    #[cfg(windows)]
    fn send(&self, command: native::PlayerCommand) -> anyhow::Result<()> {
        self.commands
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No active player"))?
            .send(command)
            .map_err(|_| anyhow::anyhow!("The native player is no longer running"))
    }

    pub fn toggle_pause(&self) -> anyhow::Result<()> {
        self.send(native::PlayerCommand::TogglePause)?;
        if let Ok(mut state) = self.state.lock() {
            state.paused = !state.paused;
        }
        Ok(())
    }
    pub fn seek(&self, position_ms: i64) -> anyhow::Result<()> {
        self.send(native::PlayerCommand::Seek(position_ms.max(0)))
    }
    pub fn seek_relative(&self, offset_ms: i64) -> anyhow::Result<()> {
        self.send(native::PlayerCommand::SeekRelative(offset_ms))
    }
    pub fn set_volume(&self, volume: i64) -> anyhow::Result<()> {
        let volume = volume.clamp(0, 100);
        self.send(native::PlayerCommand::Volume(volume))?;
        if let Ok(mut state) = self.state.lock() {
            state.volume = volume;
        }
        Ok(())
    }
    pub fn toggle_mute(&self) -> anyhow::Result<()> {
        self.send(native::PlayerCommand::ToggleMute)?;
        if let Ok(mut state) = self.state.lock() {
            state.muted = !state.muted;
        }
        Ok(())
    }
    pub fn cycle_audio(&self) -> anyhow::Result<()> {
        self.send(native::PlayerCommand::CycleAudio)
    }
    pub fn cycle_subtitle(&self) -> anyhow::Result<()> {
        self.send(native::PlayerCommand::CycleSubtitle)
    }

    pub fn stop(&mut self) {
        #[cfg(windows)]
        if let Some(commands) = self.commands.take() {
            let _ = commands.send(native::PlayerCommand::Stop);
        }
        self.prepared_media_id = None;
        if let Ok(mut state) = self.state.lock() {
            state.active = false;
            state.loading = false;
        }
    }
}
