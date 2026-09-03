use std::{
    ffi::{CString, c_char, c_void},
    path::PathBuf,
    ptr,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use libloading::Library;
use windows_sys::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage},
};

use super::{PlayerState, PlayerTrack, ResizeMode, SubtitleStyle, TrackLanguages};

const PROGRESS_CHECKPOINT_INTERVAL: Duration = Duration::from_secs(15);

/// mpv's geometry for each picture mode, matching the official client's
/// `applyResizeMode`.
///
/// Zoom is a half pan-and-scan, not a full one. Treating it as 1.0 — which is
/// Fill — made the two modes identical and made Zoom crop far harder than it
/// should, so cycling appeared not to work: two of the steps looked the same
/// and the third was extreme.
fn panscan_for(mode: ResizeMode) -> (&'static str, &'static str) {
    match mode {
        ResizeMode::Fit => ("yes", "0.0"),
        ResizeMode::Zoom => ("yes", "0.5"),
        ResizeMode::Fill => ("yes", "1.0"),
        // The one the official client leaves to its surface rather than mpv;
        // on this shell mpv owns the surface, so the aspect lock comes off.
        ResizeMode::Stretch => ("no", "0.0"),
    }
}

fn should_emit_progress_checkpoint(
    elapsed: Duration,
    pause_checkpoint_pending: bool,
    paused: bool,
    position_ms: i64,
    duration_ms: i64,
) -> bool {
    position_ms > 0
        && duration_ms > 0
        && (pause_checkpoint_pending || (!paused && elapsed >= PROGRESS_CHECKPOINT_INTERVAL))
}

#[derive(Clone, Debug)]
pub enum PlayerCommand {
    TogglePause,
    Seek(i64),
    SeekRelative(i64),
    Volume(i64),
    ToggleMute,
    SetMuted(bool),
    CycleAudio,
    CycleSubtitle,
    SetAudio(i64),
    SetSubtitle(i64),
    SetSpeed(f64),
    SetResizeMode(ResizeMode),
    Stop,
}

#[derive(Default)]
struct PendingCommands {
    stop: bool,
    toggle_pause: bool,
    seek: Option<PlayerCommand>,
    volume: Option<i64>,
    toggle_mute: bool,
    muted: Option<bool>,
    audio: Option<PlayerCommand>,
    subtitle: Option<PlayerCommand>,
    speed: Option<f64>,
    resize_mode: Option<ResizeMode>,
}

/// A bounded, coalescing mailbox for UI controls. Slider input can generate
/// hundreds of seek/volume requests per second; retaining only the newest
/// value prevents stale commands from being replayed after the user lets go.
#[derive(Clone, Default)]
pub struct PlayerCommands {
    pending: Arc<Mutex<PendingCommands>>,
}

impl PlayerCommands {
    pub fn send(&self, command: PlayerCommand) -> Result<()> {
        let mut pending = self
            .pending
            .lock()
            .map_err(|_| anyhow::anyhow!("the native player command mailbox was interrupted"))?;
        if pending.stop {
            bail!("the native player is stopping");
        }
        match command {
            PlayerCommand::Stop => {
                *pending = PendingCommands {
                    stop: true,
                    ..Default::default()
                };
            }
            PlayerCommand::TogglePause => pending.toggle_pause = !pending.toggle_pause,
            command @ (PlayerCommand::Seek(_) | PlayerCommand::SeekRelative(_)) => {
                pending.seek = Some(command);
            }
            PlayerCommand::Volume(value) => pending.volume = Some(value),
            PlayerCommand::ToggleMute => pending.toggle_mute = !pending.toggle_mute,
            PlayerCommand::SetMuted(value) => pending.muted = Some(value),
            command @ (PlayerCommand::CycleAudio | PlayerCommand::SetAudio(_)) => {
                pending.audio = Some(command);
            }
            command @ (PlayerCommand::CycleSubtitle | PlayerCommand::SetSubtitle(_)) => {
                pending.subtitle = Some(command);
            }
            PlayerCommand::SetSpeed(value) => pending.speed = Some(value),
            PlayerCommand::SetResizeMode(mode) => pending.resize_mode = Some(mode),
        }
        Ok(())
    }

    fn drain(&self) -> Vec<PlayerCommand> {
        let Ok(mut pending) = self.pending.lock() else {
            return vec![PlayerCommand::Stop];
        };
        if pending.stop {
            return vec![PlayerCommand::Stop];
        }
        let mut commands = Vec::with_capacity(6);
        if std::mem::take(&mut pending.toggle_pause) {
            commands.push(PlayerCommand::TogglePause);
        }
        if let Some(command) = pending.seek.take() {
            commands.push(command);
        }
        if let Some(value) = pending.volume.take() {
            commands.push(PlayerCommand::Volume(value));
        }
        if std::mem::take(&mut pending.toggle_mute) {
            commands.push(PlayerCommand::ToggleMute);
        }
        if let Some(value) = pending.muted.take() {
            commands.push(PlayerCommand::SetMuted(value));
        }
        if let Some(command) = pending.audio.take() {
            commands.push(command);
        }
        if let Some(command) = pending.subtitle.take() {
            commands.push(command);
        }
        if let Some(value) = pending.speed.take() {
            commands.push(PlayerCommand::SetSpeed(value));
        }
        if let Some(mode) = pending.resize_mode.take() {
            commands.push(PlayerCommand::SetResizeMode(mode));
        }
        commands
    }
}

pub struct PlayerRuntime {
    pub commands: PlayerCommands,
    join: Option<thread::JoinHandle<()>>,
}

impl PlayerRuntime {
    pub fn stop(mut self) {
        let _ = self.commands.send(PlayerCommand::Stop);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

#[repr(C)]
struct MpvEvent {
    event_id: i32,
    error: i32,
    reply_userdata: u64,
    data: *mut c_void,
}
#[repr(C)]
struct MpvEventEndFile {
    reason: i32,
    error: i32,
}
type MpvCreate = unsafe extern "C" fn() -> *mut c_void;
type MpvSetOptionString = unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> i32;
type MpvSetOption = unsafe extern "C" fn(*mut c_void, *const c_char, i32, *mut c_void) -> i32;
type MpvInitialize = unsafe extern "C" fn(*mut c_void) -> i32;
type MpvCommand = unsafe extern "C" fn(*mut c_void, *const *const c_char) -> i32;
type MpvCommandAsync = unsafe extern "C" fn(*mut c_void, u64, *const *const c_char) -> i32;
type MpvWaitEvent = unsafe extern "C" fn(*mut c_void, f64) -> *const MpvEvent;
type MpvGetProperty = unsafe extern "C" fn(*mut c_void, *const c_char, i32, *mut c_void) -> i32;
type MpvFree = unsafe extern "C" fn(*mut c_void);
type MpvDestroy = unsafe extern "C" fn(*mut c_void);

struct MpvHandle {
    raw: *mut c_void,
    destroy: MpvDestroy,
}

impl Drop for MpvHandle {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            unsafe { (self.destroy)(self.raw) };
            self.raw = ptr::null_mut();
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn launch(
    parent_hwnd: isize,
    dll: PathBuf,
    url: String,
    request_headers: Vec<String>,
    start_position_ms: i64,
    subtitle_style: SubtitleStyle,
    resize_mode: ResizeMode,
    languages: TrackLanguages,
    rtx_super_resolution: bool,
    state: Arc<Mutex<PlayerState>>,
    on_progress: Box<dyn Fn(i64, i64, bool) + Send + 'static>,
) -> Result<PlayerRuntime> {
    let commands = PlayerCommands::default();
    let player_commands = commands.clone();
    let join = thread::Builder::new()
        .name("nuvio-embedded-mpv".to_string())
        .spawn(move || {
            if let Err(error) = run_player(
                parent_hwnd as HWND,
                &dll,
                &url,
                &request_headers,
                start_position_ms,
                &subtitle_style,
                resize_mode,
                &languages,
                rtx_super_resolution,
                &state,
                player_commands,
                on_progress,
            ) {
                if let Ok(mut current) = state.lock() {
                    current.active = false;
                    current.loading = false;
                    current.error = Some(error.to_string());
                }
                eprintln!("embedded native player failed: {error:#}");
            }
        })
        .context("could not start embedded native player thread")?;
    Ok(PlayerRuntime {
        commands,
        join: Some(join),
    })
}

pub fn find_mpv() -> Option<PathBuf> {
    std::env::var_os("NUVIO_LIBMPV_PATH")
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .or_else(|| {
            let executable = std::env::current_exe().ok()?;
            let directory = executable.parent()?;
            [
                directory.join("libmpv-2.dll"),
                directory.join("resources/libmpv-2.dll"),
            ]
            .into_iter()
            .find(|path| path.is_file())
        })
        .or_else(|| {
            let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../../composeApp/src/desktopMain/native/windows/runtime/libmpv-2.dll");
            path.is_file().then_some(path)
        })
        .or_else(|| {
            let path = PathBuf::from(r"C:\Program Files\Nuvio\app\native\libmpv-2.dll");
            path.is_file().then_some(path)
        })
}

#[allow(clippy::too_many_arguments)]
fn run_player(
    parent: HWND,
    dll_path: &PathBuf,
    source: &str,
    request_headers: &[String],
    start_position_ms: i64,
    subtitle_style: &SubtitleStyle,
    resize_mode: ResizeMode,
    languages: &TrackLanguages,
    rtx_super_resolution: bool,
    state: &Arc<Mutex<PlayerState>>,
    commands: PlayerCommands,
    on_progress: Box<dyn Fn(i64, i64, bool) + Send + 'static>,
) -> Result<()> {
    unsafe {
        let library = Library::new(dll_path).context("could not load libmpv-2.dll")?;
        let mpv_create = *library.get::<MpvCreate>(b"mpv_create\0")?;
        let mpv_set_option_string =
            *library.get::<MpvSetOptionString>(b"mpv_set_option_string\0")?;
        let mpv_set_option = *library.get::<MpvSetOption>(b"mpv_set_option\0")?;
        let mpv_initialize = *library.get::<MpvInitialize>(b"mpv_initialize\0")?;
        let mpv_command = *library.get::<MpvCommand>(b"mpv_command\0")?;
        let mpv_command_async = *library.get::<MpvCommandAsync>(b"mpv_command_async\0")?;
        let mpv_wait_event = *library.get::<MpvWaitEvent>(b"mpv_wait_event\0")?;
        let mpv_get_property = *library.get::<MpvGetProperty>(b"mpv_get_property\0")?;
        let mpv_free = *library.get::<MpvFree>(b"mpv_free\0")?;
        let mpv_destroy = *library.get::<MpvDestroy>(b"mpv_terminate_destroy\0")?;

        // Match Nuvio's Windows bridge: mpv renders into the same native host
        // whose WebView2 controller supplies transparent controls. A separate
        // sibling video HWND cannot show through a windowed WebView2 surface.
        let hwnd = parent;

        let raw_handle = mpv_create();
        if raw_handle.is_null() {
            bail!("mpv_create failed");
        }
        let handle_guard = MpvHandle {
            raw: raw_handle,
            destroy: mpv_destroy,
        };
        let handle = handle_guard.raw;
        set_option(mpv_set_option_string, handle, "config", "no")?;
        set_option(mpv_set_option_string, handle, "osc", "no")?;
        set_option(
            mpv_set_option_string,
            handle,
            "input-default-bindings",
            "yes",
        )?;
        set_option(mpv_set_option_string, handle, "input-vo-keyboard", "no")?;
        set_option(mpv_set_option_string, handle, "keep-open", "yes")?;
        let (keep_aspect, panscan) = panscan_for(resize_mode);
        // Track preferences, in mpv's own priority order. Set as options rather
        // than chosen afterwards so the first frame already has the right audio
        // — switching after load is audible.
        if !languages.audio.is_empty() {
            set_option(
                mpv_set_option_string,
                handle,
                "alang",
                &languages.audio.join(","),
            )?;
        }
        if !languages.subtitles.is_empty() {
            set_option(
                mpv_set_option_string,
                handle,
                "slang",
                &languages.subtitles.join(","),
            )?;
        }
        // "Only preferred languages": mpv would otherwise fall back to any
        // subtitle track when none matches, which is the opposite of what the
        // setting asks for.
        if languages.subtitles_only_preferred && !languages.subtitles.is_empty() {
            set_option(mpv_set_option_string, handle, "subs-fallback", "no")?;
        }
        if languages.subtitles_off {
            set_option(mpv_set_option_string, handle, "sid", "no")?;
        } else if languages.subtitles_forced_only {
            // The subtitle language itself set to "Forced": take a forced track
            // over a full one wherever there is one.
            set_option(mpv_set_option_string, handle, "subs-fallback-forced", "always")?;
        }
        // "Use forced subtitles": when the audio is already in a language you
        // read, a full subtitle track is not wanted — but the forced one
        // covering foreign dialogue still is.
        //
        // `forced`, not `no`. They are different answers to the same question:
        // `no` selects nothing at all when the audio matches, which throws away
        // the forced track this setting exists to keep, while `forced` selects
        // only that one and falls back to nothing — which is the setting's own
        // description word for word.
        //
        // Not fatal if the value is refused: this is a subtitle preference, and
        // an older libmpv that only knows the yes/no form should not stop the
        // film from playing.
        if languages.forced_with_matching_audio
            && set_option(
                mpv_set_option_string,
                handle,
                "subs-with-matching-audio",
                "forced",
            )
            .is_err()
        {
            let _ = set_option(mpv_set_option_string, handle, "subs-with-matching-audio", "no");
        }
        set_option(mpv_set_option_string, handle, "keepaspect", keep_aspect)?;
        set_option(mpv_set_option_string, handle, "keepaspect-window", "no")?;
        set_option(mpv_set_option_string, handle, "video-aspect-override", "no")?;
        set_option(mpv_set_option_string, handle, "panscan", panscan)?;
        set_option(mpv_set_option_string, handle, "video-unscaled", "no")?;
        set_option(mpv_set_option_string, handle, "video-zoom", "0")?;
        set_option(mpv_set_option_string, handle, "video-align-x", "0")?;
        set_option(mpv_set_option_string, handle, "video-align-y", "0")?;
        set_option(mpv_set_option_string, handle, "vo", "gpu-next")?;
        if let Some(warning) =
            configure_video_pipeline(mpv_set_option_string, handle, rtx_super_resolution)?
            && let Ok(mut current) = state.lock()
        {
            current.warning = Some(warning);
        }
        set_option(mpv_set_option_string, handle, "hwdec-codecs", "all")?;
        set_option(
            mpv_set_option_string,
            handle,
            "vd-lavc-software-fallback",
            "yes",
        )?;
        set_option(mpv_set_option_string, handle, "vd-lavc-threads", "4")?;
        set_option(
            mpv_set_option_string,
            handle,
            "target-colorspace-hint",
            "yes",
        )?;
        set_option(mpv_set_option_string, handle, "tone-mapping", "auto")?;
        set_option(mpv_set_option_string, handle, "dither-depth", "auto")?;
        set_option(
            mpv_set_option_string,
            handle,
            "demuxer-seekable-cache",
            "yes",
        )?;
        set_option(mpv_set_option_string, handle, "demuxer-max-bytes", "512MiB")?;
        set_option(
            mpv_set_option_string,
            handle,
            "demuxer-max-back-bytes",
            "256MiB",
        )?;
        set_option(mpv_set_option_string, handle, "cache-secs", "36000")?;
        set_option(mpv_set_option_string, handle, "hr-seek", "no")?;
        apply_subtitle_style(mpv_set_option_string, handle, subtitle_style)?;
        if !request_headers.is_empty() {
            set_option(
                mpv_set_option_string,
                handle,
                "http-header-fields",
                &request_headers
                    .iter()
                    .map(|header| header.replace('\\', "\\\\").replace(',', "\\,"))
                    .collect::<Vec<_>>()
                    .join(","),
            )?;
        }
        let wid_name = CString::new("wid")?;
        let mut wid = hwnd as isize as i64;
        if mpv_set_option(handle, wid_name.as_ptr(), 4, (&mut wid as *mut i64).cast()) < 0
            || mpv_initialize(handle) < 0
        {
            bail!("libmpv initialization failed");
        }
        // `start` is applied as an option rather than a loadfile argument: the
        // position of loadfile's index/options parameters changed between mpv
        // releases, so passing them positionally silently drops the resume point
        // on the version that does not expect them.
        if start_position_ms > 0 {
            set_option(
                mpv_set_option_string,
                handle,
                "start",
                &format!("{:.3}", start_position_ms as f64 / 1000.0),
            )?;
        }
        command(mpv_command, handle, &["loadfile", source])?;

        let mut position = start_position_ms as f64 / 1000.0;
        let mut duration = 0.0f64;
        let mut sample_tick = 0u32;
        let mut last_progress_checkpoint = Instant::now();
        let mut checkpoint_clock_started = false;
        let mut was_paused = false;
        let mut pause_checkpoint_pending = false;
        let mut stopped = false;
        let mut reached_eof = false;
        let mut track_count = -1i64;
        let mut tracks: Vec<PlayerTrack> = Vec::new();
        while !stopped {
            let mut message: MSG = std::mem::zeroed();
            while PeekMessageW(&mut message, ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            for next in commands.drain() {
                match next {
                    PlayerCommand::TogglePause => {
                        let _ = command(mpv_command, handle, &["cycle", "pause"]);
                    }
                    PlayerCommand::Seek(ms) => {
                        let _ = command(
                            mpv_command,
                            handle,
                            &[
                                "seek",
                                &format!("{:.3}", ms as f64 / 1000.0),
                                "absolute+exact",
                            ],
                        );
                    }
                    PlayerCommand::SeekRelative(ms) => {
                        let _ = command(
                            mpv_command,
                            handle,
                            &[
                                "seek",
                                &format!("{:.3}", ms as f64 / 1000.0),
                                "relative+keyframes",
                            ],
                        );
                    }
                    PlayerCommand::Volume(value) => {
                        let _ =
                            command(mpv_command, handle, &["set", "volume", &value.to_string()]);
                    }
                    PlayerCommand::ToggleMute => {
                        let _ = command(mpv_command, handle, &["cycle", "mute"]);
                    }
                    PlayerCommand::CycleAudio => {
                        // Track changes may reopen a decoder or fetch another remote
                        // stream. Submit them asynchronously so state polling and the
                        // rest of the controls never wait for that work to finish.
                        let _ = command_async(mpv_command_async, handle, 1, &["cycle", "aid"]);
                    }
                    PlayerCommand::CycleSubtitle => {
                        let _ = command_async(mpv_command_async, handle, 2, &["cycle", "sid"]);
                    }
                    PlayerCommand::SetAudio(id) => {
                        let _ = command_async(
                            mpv_command_async,
                            handle,
                            3,
                            &["set", "aid", &track_value(id)],
                        );
                    }
                    PlayerCommand::SetSubtitle(id) => {
                        let _ = command_async(
                            mpv_command_async,
                            handle,
                            4,
                            &["set", "sid", &track_value(id)],
                        );
                    }
                    PlayerCommand::SetSpeed(speed) => {
                        let _ = command_async(
                            mpv_command_async,
                            handle,
                            5,
                            &["set", "speed", &format!("{speed:.3}")],
                        );
                    }
                    PlayerCommand::SetMuted(value) => {
                        // Stated rather than toggled: mpv's mute is separate
                        // from its volume, so raising the slider while muted
                        // left it muted and the next poll put the UI back.
                        let _ = command_async(
                            mpv_command_async,
                            handle,
                            8,
                            &["set", "mute", if value { "yes" } else { "no" }],
                        );
                    }
                    PlayerCommand::SetResizeMode(mode) => {
                        // The same pair the initial options set, so switching
                        // at runtime lands on exactly the geometry a fresh
                        // launch in that mode would have produced.
                        let (keep_aspect, panscan) = panscan_for(mode);
                        let _ = command_async(
                            mpv_command_async,
                            handle,
                            6,
                            &["set", "keepaspect", keep_aspect],
                        );
                        let _ = command_async(
                            mpv_command_async,
                            handle,
                            7,
                            &["set", "panscan", panscan],
                        );
                    }
                    PlayerCommand::Stop => stopped = true,
                }
            }
            let event = mpv_wait_event(handle, 0.0);
            if !event.is_null() {
                match (*event).event_id {
                    1 => stopped = true,
                    7 => {
                        let end = (*event).data.cast::<MpvEventEndFile>();
                        // MPV_END_FILE_REASON_EOF. Distinguishes "watched to the
                        // end" from "closed early", which is what decides whether
                        // Nuvio marks the episode finished.
                        if !end.is_null() && (*end).reason == 0 {
                            reached_eof = true;
                            if let Ok(mut current) = state.lock() {
                                current.ended = true;
                            }
                        }
                        if !end.is_null()
                            && (*end).error < 0
                            && let Ok(mut current) = state.lock()
                        {
                            current.loading = false;
                            current.error = Some(mpv_error_message((*end).error));
                        }
                    }
                    // MPV_EVENT_PLAYBACK_RESTART occurs after playback actually
                    // has a frame ready. MPV_EVENT_FILE_LOADED (8) is too early
                    // and caused the loading cover to reveal a blank video plane.
                    21 => {
                        if let Ok(mut current) = state.lock() {
                            current.loading = false;
                            current.ended = false;
                            current.error = None;
                        }
                    }
                    _ => {}
                }
            }
            sample_tick += 1;
            if sample_tick >= 20 {
                sample_tick = 0;
                let _ = get_double(mpv_get_property, handle, "time-pos", &mut position);
                let _ = get_double(mpv_get_property, handle, "duration", &mut duration);
                let mut paused = 0i32;
                let mut muted = 0i32;
                let mut volume = 100.0;
                let _ = get_flag(mpv_get_property, handle, "pause", &mut paused);
                let _ = get_flag(mpv_get_property, handle, "mute", &mut muted);
                let _ = get_double(mpv_get_property, handle, "volume", &mut volume);

                // The full track list is only re-read when the count changes;
                // the selected ids are cheap enough to poll every sample.
                let count = get_int(mpv_get_property, handle, "track-list/count").unwrap_or(0);
                if count != track_count {
                    track_count = count;
                    tracks = read_tracks(mpv_get_property, mpv_free, handle, count);
                }
                let audio_id = get_int(mpv_get_property, handle, "aid").unwrap_or(-1);
                let subtitle_id = get_int(mpv_get_property, handle, "sid").unwrap_or(-1);
                for track in &mut tracks {
                    track.selected = match track.kind.as_str() {
                        "audio" => track.id == audio_id,
                        _ => track.id == subtitle_id,
                    };
                }
                let is_paused = paused != 0;
                if is_paused && !was_paused {
                    pause_checkpoint_pending = true;
                } else if !is_paused {
                    // A pause that ended before mpv reported a valid duration
                    // no longer needs a delayed pause checkpoint.
                    pause_checkpoint_pending = false;
                }
                let position_ms = (position * 1000.0) as i64;
                let duration_ms = (duration * 1000.0) as i64;
                let now = Instant::now();
                if !checkpoint_clock_started && position_ms > 0 && duration_ms > 0 {
                    last_progress_checkpoint = now;
                    checkpoint_clock_started = true;
                }
                if should_emit_progress_checkpoint(
                    now.duration_since(last_progress_checkpoint),
                    pause_checkpoint_pending,
                    is_paused,
                    position_ms,
                    duration_ms,
                ) {
                    // The callback only appends to an in-memory channel. HTTP
                    // sync runs on the named progress worker in ipc.rs.
                    on_progress(position_ms, duration_ms, false);
                    last_progress_checkpoint = now;
                    pause_checkpoint_pending = false;
                } else if is_paused {
                    // Paused wall-clock time does not count toward the next
                    // fifteen seconds of active-playback checkpointing.
                    last_progress_checkpoint = now;
                }
                was_paused = is_paused;
                if let Ok(mut current) = state.lock() {
                    current.active = true;
                    current.paused = is_paused;
                    current.position_ms = position_ms;
                    current.duration_ms = duration_ms;
                    current.volume = volume.round() as i64;
                    current.muted = muted != 0;
                    current.audio_track = audio_id;
                    current.subtitle_track = subtitle_id;
                    current.tracks = tracks.clone();
                }
            }
            thread::sleep(Duration::from_millis(15));
        }
        // Release the native decoder/window before the final enqueue. The
        // callback is nonblocking and dropping it closes the reporter channel;
        // the worker drains the final EOF checkpoint before exiting.
        drop(handle_guard);
        if let Ok(mut current) = state.lock() {
            current.active = false;
            current.loading = false;
        }
        if position > 0.0 && duration > 0.0 {
            let position_ms = (position * 1000.0) as i64;
            let duration_ms = (duration * 1000.0) as i64;
            on_progress(position_ms, duration_ms, reached_eof);
        }
        Ok(())
    }
}

/// mpv wants `#AARRGGBB` too, so the stored value passes through unchanged —
/// only the alpha-less form needs padding.
fn mpv_color(value: &str) -> String {
    let body = value.trim_start_matches('#');
    if body.len() == 6 {
        format!("#FF{}", body.to_uppercase())
    } else {
        format!("#{}", body.to_uppercase())
    }
}

unsafe fn apply_subtitle_style(
    set: MpvSetOptionString,
    handle: *mut c_void,
    style: &SubtitleStyle,
) -> Result<()> {
    unsafe {
        // `sub-ass-override=force` is what makes these apply to ASS/SSA tracks;
        // without it styled subtitles ignore every option below.
        set_option(
            set,
            handle,
            "sub-ass-override",
            if style.use_libass { "no" } else { "force" },
        )?;
        set_option(set, handle, "sub-font-size", &style.font_size.to_string())?;
        set_option(
            set,
            handle,
            "sub-bold",
            if style.bold { "yes" } else { "no" },
        )?;
        set_option(set, handle, "sub-color", &mpv_color(&style.text_color))?;
        set_option(
            set,
            handle,
            "sub-back-color",
            &mpv_color(&style.background_color),
        )?;
        set_option(
            set,
            handle,
            "sub-border-size",
            &if style.outline_enabled {
                style.outline_width
            } else {
                0
            }
            .to_string(),
        )?;
        set_option(
            set,
            handle,
            "sub-border-color",
            &mpv_color(&style.outline_color),
        )?;
        // mpv measures this in 0-100 units of window height from the bottom.
        set_option(
            set,
            handle,
            "sub-margin-y",
            &style.bottom_offset.clamp(0, 200).to_string(),
        )?;
    }
    Ok(())
}

/// libmpv error codes, so a failure reads as a cause rather than a number.
fn mpv_error_message(code: i32) -> String {
    match code {
        -13 => "This source could not be opened. The link may have expired — pick another source.",
        -14 => "Audio output could not be initialised for this source.",
        -15 => "Video output could not be initialised for this source.",
        -16 => "The source contained nothing playable.",
        -17 => "This source is in a format the player does not recognise.",
        -18 => "This source uses an unsupported codec or container.",
        -12 => "The player rejected the playback command.",
        _ => "This source could not be played.",
    }
    .to_string()
}

/// mpv treats `no` as "disabled" for aid/sid; any other value is a track id.
fn track_value(id: i64) -> String {
    if id <= 0 {
        "no".to_string()
    } else {
        id.to_string()
    }
}

/// MPV_FORMAT_INT64 = 4.
unsafe fn get_int(function: MpvGetProperty, handle: *mut c_void, name: &str) -> Option<i64> {
    let name = CString::new(name).ok()?;
    let mut value = 0i64;
    let code = unsafe {
        function(
            handle,
            name.as_ptr(),
            4,
            &mut value as *mut i64 as *mut c_void,
        )
    };
    (code >= 0).then_some(value)
}

/// MPV_FORMAT_STRING = 1. mpv allocates the buffer, so it must be freed.
unsafe fn get_string(
    function: MpvGetProperty,
    free: MpvFree,
    handle: *mut c_void,
    name: &str,
) -> Option<String> {
    let name = CString::new(name).ok()?;
    let mut raw: *mut c_char = ptr::null_mut();
    let code = unsafe {
        function(
            handle,
            name.as_ptr(),
            1,
            &mut raw as *mut *mut c_char as *mut c_void,
        )
    };
    if code < 0 || raw.is_null() {
        return None;
    }
    let value = unsafe { std::ffi::CStr::from_ptr(raw) }
        .to_string_lossy()
        .into_owned();
    unsafe { free(raw as *mut c_void) };
    Some(value)
}

fn read_tracks(
    get: MpvGetProperty,
    free: MpvFree,
    handle: *mut c_void,
    count: i64,
) -> Vec<PlayerTrack> {
    (0..count.max(0))
        .filter_map(|index| {
            let kind =
                unsafe { get_string(get, free, handle, &format!("track-list/{index}/type")) }?;
            if kind != "audio" && kind != "sub" {
                return None;
            }
            let id = unsafe { get_int(get, handle, &format!("track-list/{index}/id")) }?;
            Some(PlayerTrack {
                id,
                kind,
                title: unsafe {
                    get_string(get, free, handle, &format!("track-list/{index}/title"))
                }
                .unwrap_or_default(),
                lang: unsafe { get_string(get, free, handle, &format!("track-list/{index}/lang")) }
                    .unwrap_or_default(),
                selected: false,
            })
        })
        .collect()
}

unsafe fn set_option(
    function: MpvSetOptionString,
    handle: *mut c_void,
    name: &str,
    value: &str,
) -> Result<()> {
    let option_name = name.to_string();
    let name = CString::new(name)?;
    let value = CString::new(value)?;
    if unsafe { function(handle, name.as_ptr(), value.as_ptr()) } < 0 {
        bail!("libmpv rejected option {option_name}")
    } else {
        Ok(())
    }
}

/// Applies the same RTX VSR pipeline as Nuvio's native Windows bridge. Older
/// mpv/FFmpeg builds can lack `d3d11vpp` or NVIDIA's scaling mode, so a rejected
/// enhancement option is cleared and playback continues through the ordinary
/// automatic GPU pipeline.
unsafe fn configure_video_pipeline(
    function: MpvSetOptionString,
    handle: *mut c_void,
    rtx_super_resolution: bool,
) -> Result<Option<String>> {
    if !rtx_super_resolution {
        for (name, value) in video_pipeline_options(false) {
            unsafe { set_option(function, handle, name, value)? };
        }
        return Ok(None);
    }

    for (name, value) in video_pipeline_options(true) {
        if let Err(error) = unsafe { set_option(function, handle, name, value) } {
            eprintln!(
                "RTX super resolution is unavailable ({error}); continuing with normal GPU playback"
            );
            // Best effort cleanup. These reset values are understood by all
            // libmpv builds supported by this player; failure still should not
            // turn an optional enhancement into a playback failure.
            let _ = unsafe { set_option(function, handle, "vf", "") };
            let _ = unsafe { set_option(function, handle, "gpu-api", "auto") };
            let _ = unsafe { set_option(function, handle, "hwdec", "auto") };
            return Ok(Some(
                "RTX video enhancement is unavailable; using normal GPU playback".to_string(),
            ));
        }
    }
    Ok(None)
}

fn video_pipeline_options(rtx_super_resolution: bool) -> &'static [(&'static str, &'static str)] {
    if rtx_super_resolution {
        &[
            ("vf", "d3d11vpp=scale=2:scaling-mode=nvidia"),
            ("gpu-api", "d3d11"),
            ("hwdec", "d3d11va"),
            ("d3d11-adapter", "NVIDIA"),
        ]
    } else {
        &[("gpu-api", "auto"), ("hwdec", "auto")]
    }
}
unsafe fn command(function: MpvCommand, handle: *mut c_void, values: &[&str]) -> Result<()> {
    let strings = values
        .iter()
        .map(|value| CString::new(*value))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut pointers = strings
        .iter()
        .map(|value| value.as_ptr())
        .collect::<Vec<_>>();
    pointers.push(ptr::null());
    if unsafe { function(handle, pointers.as_ptr()) } < 0 {
        bail!("libmpv rejected command")
    } else {
        Ok(())
    }
}
unsafe fn command_async(
    function: MpvCommandAsync,
    handle: *mut c_void,
    reply_userdata: u64,
    values: &[&str],
) -> Result<()> {
    let strings = values
        .iter()
        .map(|value| CString::new(*value))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut pointers = strings
        .iter()
        .map(|value| value.as_ptr())
        .collect::<Vec<_>>();
    pointers.push(ptr::null());
    if unsafe { function(handle, reply_userdata, pointers.as_ptr()) } < 0 {
        bail!("libmpv rejected asynchronous command")
    } else {
        Ok(())
    }
}
unsafe fn get_double(
    function: MpvGetProperty,
    handle: *mut c_void,
    name: &str,
    output: &mut f64,
) -> i32 {
    let name = CString::new(name).unwrap();
    unsafe { function(handle, name.as_ptr(), 5, (output as *mut f64).cast()) }
}
unsafe fn get_flag(
    function: MpvGetProperty,
    handle: *mut c_void,
    name: &str,
    output: &mut i32,
) -> i32 {
    let name = CString::new(name).unwrap();
    unsafe { function(handle, name.as_ptr(), 3, (output as *mut i32).cast()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mailbox_coalesces_slider_commands_to_the_latest_value() {
        let mailbox = PlayerCommands::default();
        mailbox.send(PlayerCommand::Seek(1_000)).unwrap();
        mailbox.send(PlayerCommand::Seek(8_000)).unwrap();
        mailbox.send(PlayerCommand::Volume(10)).unwrap();
        mailbox.send(PlayerCommand::Volume(70)).unwrap();
        let commands = mailbox.drain();
        assert!(matches!(commands[0], PlayerCommand::Seek(8_000)));
        assert!(matches!(commands[1], PlayerCommand::Volume(70)));
        assert_eq!(commands.len(), 2);
    }

    #[test]
    fn stop_discards_queued_controls() {
        let mailbox = PlayerCommands::default();
        mailbox.send(PlayerCommand::Seek(1_000)).unwrap();
        mailbox.send(PlayerCommand::Stop).unwrap();
        let commands = mailbox.drain();
        assert!(matches!(commands.as_slice(), [PlayerCommand::Stop]));
    }

    #[test]
    fn rtx_pipeline_matches_nuvios_windows_bridge() {
        assert_eq!(
            video_pipeline_options(true),
            &[
                ("vf", "d3d11vpp=scale=2:scaling-mode=nvidia"),
                ("gpu-api", "d3d11"),
                ("hwdec", "d3d11va"),
                ("d3d11-adapter", "NVIDIA"),
            ]
        );
        assert_eq!(
            video_pipeline_options(false),
            &[("gpu-api", "auto"), ("hwdec", "auto")]
        );
    }

    #[test]
    fn progress_checkpoints_are_periodic_and_pause_aware() {
        assert!(!should_emit_progress_checkpoint(
            Duration::from_secs(14),
            false,
            false,
            14_000,
            60_000,
        ));
        assert!(should_emit_progress_checkpoint(
            Duration::from_secs(15),
            false,
            false,
            15_000,
            60_000,
        ));
        assert!(should_emit_progress_checkpoint(
            Duration::ZERO,
            true,
            true,
            15_000,
            60_000,
        ));
        assert!(!should_emit_progress_checkpoint(
            Duration::from_secs(60),
            false,
            true,
            15_000,
            60_000,
        ));
        assert!(!should_emit_progress_checkpoint(
            Duration::from_secs(60),
            true,
            true,
            0,
            60_000,
        ));
    }
}
