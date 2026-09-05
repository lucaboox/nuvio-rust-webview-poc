use serde::Serialize;
use std::sync::{Arc, Mutex};
mod languages;
pub use languages::preferred_languages;

#[allow(dead_code)]
mod ffi;
#[cfg(windows)]
mod native;
#[cfg(windows)]
pub use native::find_mpv;

/// Which audio and subtitle tracks to prefer, in mpv's own order of priority.
///
/// The settings existed and were parsed, but nothing ever handed them to mpv —
/// so a file with several audio tracks always came up on whichever one it
/// happened to list first, whatever the account said. Kept beside
/// `SubtitleStyle` and for the same reason: the player layer should not have to
/// know the sync schema.
#[derive(Clone, Debug, Default)]
pub struct TrackLanguages {
    /// Preferred first, then the secondary, as mpv reads them left to right.
    /// Real ISO codes only — the sentinels below are stripped by the caller,
    /// because mpv would otherwise hunt for a track tagged "none".
    pub audio: Vec<String>,
    pub subtitles: Vec<String>,
    /// Subtitle language set to "Off".
    pub subtitles_off: bool,
    /// Subtitle language set to "Forced": prefer a forced track outright.
    pub subtitles_forced_only: bool,
    /// Nuvio's "only preferred languages": with nothing matching, show none
    /// rather than falling back to a language that was not asked for.
    pub subtitles_only_preferred: bool,
    /// Nuvio's "Use forced subtitles" — "prefer forced subtitles when audio
    /// matches the subtitle language; if unavailable, select nothing". That is
    /// mpv's `subs-with-matching-audio` exactly: when the audio is already in a
    /// language you read, a full subtitle track is not wanted, but the forced
    /// one covering foreign dialogue still is.
    pub forced_with_matching_audio: bool,
}

/// Subtitle appearance pushed into mpv at load time. Kept separate from the
/// settings snapshot so the player layer does not depend on the sync schema.
#[derive(Clone, Debug)]
pub struct SubtitleStyle {
    pub font_size: i64,
    pub bold: bool,
    pub text_color: String,
    pub background_color: String,
    pub outline_enabled: bool,
    pub outline_color: String,
    pub outline_width: i64,
    pub bottom_offset: i64,
    pub use_libass: bool,
}

/// The four picture modes exposed by Nuvio. The Windows implementation maps
/// these to the same libmpv properties as the official desktop bridge.
#[derive(Clone, Copy, Debug, Default)]
pub enum ResizeMode {
    #[default]
    Fit,
    Fill,
    Zoom,
    Stretch,
}

impl ResizeMode {
    pub fn from_setting(value: &str) -> Self {
        match value {
            "Fill" => Self::Fill,
            "Zoom" => Self::Zoom,
            "Stretch" => Self::Stretch,
            _ => Self::Fit,
        }
    }
}

impl Default for SubtitleStyle {
    fn default() -> Self {
        // Matches Nuvio's SubtitleStyleState.DEFAULT.
        Self {
            font_size: 18,
            bold: false,
            text_color: "#FFFFFFFF".to_string(),
            background_color: "#00000000".to_string(),
            outline_enabled: true,
            outline_color: "#FF000000".to_string(),
            outline_width: 2,
            bottom_offset: 20,
            use_libass: false,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerState {
    pub active: bool,
    pub loading: bool,
    /// True only after libmpv reports a natural end-of-file. Stops, source
    /// changes and load failures must not trigger next-episode autoplay.
    pub ended: bool,
    pub paused: bool,
    pub position_ms: i64,
    pub duration_ms: i64,
    pub volume: i64,
    pub muted: bool,
    pub audio_track: i64,
    pub subtitle_track: i64,
    pub title: String,
    pub error: Option<String>,
    /// Non-fatal playback degradation, such as RTX VSR being unavailable.
    pub warning: Option<String>,
    /// Audio and subtitle tracks reported by mpv, for the in-player pickers.
    pub tracks: Vec<PlayerTrack>,
    pub diagnostics: Option<PlayerDiagnostics>,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerDiagnostics {
    pub rtx_requested: bool,
    pub gpu_api: Option<String>,
    pub hardware_decoder: Option<String>,
    pub video_filters: Option<String>,
    pub video_codec: Option<String>,
    pub source_width: Option<i64>,
    pub source_height: Option<i64>,
    pub output_width: Option<i64>,
    pub output_height: Option<i64>,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerTrack {
    pub id: i64,
    /// mpv's own naming: "audio" or "sub".
    pub kind: String,
    pub title: String,
    pub lang: String,
    pub selected: bool,
}

pub struct PlayerService {
    prepared_media_id: Option<String>,
    /// Kept so the thumbnailer can open the same stream independently.
    source: Option<(String, Vec<String>)>,
    parent_hwnd: isize,
    state: Arc<Mutex<PlayerState>>,
    #[cfg(windows)]
    runtime: Option<native::PlayerRuntime>,
}

impl Default for PlayerService {
    fn default() -> Self {
        Self {
            prepared_media_id: None,
            source: None,
            parent_hwnd: 0,
            state: Arc::new(Mutex::new(PlayerState {
                volume: 100,
                ..Default::default()
            })),
            #[cfg(windows)]
            runtime: None,
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
            direct_mpv_ready: direct_mpv_available(),
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
        subtitle_style: SubtitleStyle,
        resize_mode: ResizeMode,
        languages: TrackLanguages,
        rtx_super_resolution: bool,
        on_progress: Box<dyn Fn(i64, i64, bool) + Send + 'static>,
    ) -> anyhow::Result<String> {
        let url = url.ok_or_else(|| {
            anyhow::anyhow!(
                "This source is not a direct HTTP stream. Torrent/debrid resolution is not ported yet."
            )
        })?;
        let parsed = url::Url::parse(&url).map_err(|_| anyhow::anyhow!("Invalid stream URL"))?;
        if parsed.scheme() == "file" {
            let path = parsed
                .to_file_path()
                .map_err(|_| anyhow::anyhow!("Invalid local download path"))?;
            anyhow::ensure!(path.is_file(), "The downloaded media file is missing");
        } else {
            anyhow::ensure!(
                matches!(parsed.scheme(), "http" | "https") && parsed.host().is_some(),
                "This source is not a direct HTTP stream. Torrent/debrid resolution is not ported yet."
            );
            anyhow::ensure!(
                parsed.username().is_empty() && parsed.password().is_none(),
                "Stream URLs cannot contain embedded credentials"
            );
            crate::content::validate_addon_url(&parsed)?;
        }
        anyhow::ensure!(self.parent_hwnd != 0, "main window handle is unavailable");
        self.stop();
        self.prepared_media_id = Some(media_id.clone());
        self.source = Some((url.clone(), request_headers.clone()));
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
            let dll =
                native::find_mpv().ok_or_else(|| anyhow::anyhow!("libmpv-2.dll was not found"))?;
            self.runtime = Some(native::launch(
                self.parent_hwnd,
                dll,
                url,
                request_headers,
                start_position_ms,
                subtitle_style,
                resize_mode,
                languages,
                rtx_super_resolution,
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
        self.runtime
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No active player"))?
            .commands
            .send(command)
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
    pub fn set_audio_track(&self, id: i64) -> anyhow::Result<()> {
        self.send(native::PlayerCommand::SetAudio(id))
    }
    pub fn set_subtitle_track(&self, id: i64) -> anyhow::Result<()> {
        self.send(native::PlayerCommand::SetSubtitle(id))
    }
    /// The stream currently loaded, for callers that need to open it
    /// independently of playback.
    pub fn source(&self) -> Option<(String, Vec<String>)> {
        self.source.clone()
    }

    pub fn set_speed(&self, speed: f64) -> anyhow::Result<()> {
        self.send(native::PlayerCommand::SetSpeed(speed.clamp(0.25, 4.0)))
    }

    pub fn set_muted(&self, muted: bool) -> anyhow::Result<()> {
        self.send(native::PlayerCommand::SetMuted(muted))
    }

    /// Cycled from the player's own control, so it changes for this playback
    /// without rewriting the account's default.
    pub fn set_resize_mode(&self, mode: ResizeMode) -> anyhow::Result<()> {
        self.send(native::PlayerCommand::SetResizeMode(mode))
    }

    pub fn stop(&mut self) {
        #[cfg(windows)]
        if let Some(runtime) = self.runtime.take() {
            runtime.stop();
        }
        self.prepared_media_id = None;
        self.source = None;
        if let Ok(mut state) = self.state.lock() {
            state.active = false;
            state.loading = false;
        }
    }
}

fn direct_mpv_available() -> bool {
    #[cfg(windows)]
    {
        native::find_mpv().is_some()
    }
    #[cfg(not(windows))]
    {
        false
    }
}
