use std::{
    ffi::{CString, c_char, c_void},
    path::PathBuf,
    ptr,
    sync::{Arc, Mutex, mpsc},
    thread,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use libloading::Library;
use windows_sys::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage},
};

use super::PlayerState;

#[derive(Clone, Debug)]
pub enum PlayerCommand {
    TogglePause,
    Seek(i64),
    SeekRelative(i64),
    Volume(i64),
    ToggleMute,
    CycleAudio,
    CycleSubtitle,
    Stop,
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
type MpvDestroy = unsafe extern "C" fn(*mut c_void);

pub fn launch(
    parent_hwnd: isize,
    url: String,
    request_headers: Vec<String>,
    start_position_ms: i64,
    state: Arc<Mutex<PlayerState>>,
    on_progress: Box<dyn Fn(i64, i64) + Send + 'static>,
) -> Result<mpsc::Sender<PlayerCommand>> {
    let dll = find_mpv().context("libmpv-2.dll was not found")?;
    let (commands, receiver) = mpsc::channel();
    thread::Builder::new()
        .name("nuvio-embedded-mpv".to_string())
        .spawn(move || {
            if let Err(error) = run_player(
                parent_hwnd as HWND,
                &dll,
                &url,
                &request_headers,
                start_position_ms,
                &state,
                receiver,
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
    Ok(commands)
}

fn find_mpv() -> Option<PathBuf> {
    std::env::var_os("NUVIO_LIBMPV_PATH")
        .map(PathBuf::from)
        .filter(|path| path.is_file())
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

fn run_player(
    parent: HWND,
    dll_path: &PathBuf,
    source: &str,
    request_headers: &[String],
    start_position_ms: i64,
    state: &Arc<Mutex<PlayerState>>,
    receiver: mpsc::Receiver<PlayerCommand>,
    on_progress: Box<dyn Fn(i64, i64) + Send + 'static>,
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
        let mpv_destroy = *library.get::<MpvDestroy>(b"mpv_terminate_destroy\0")?;

        // Match Nuvio's Windows bridge: mpv renders into the same native host
        // whose WebView2 controller supplies transparent controls. A separate
        // sibling video HWND cannot show through a windowed WebView2 surface.
        let hwnd = parent;

        let handle = mpv_create();
        if handle.is_null() {
            bail!("mpv_create failed");
        }
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
        // Keep the source's display aspect ratio inside the exact viewport sent by
        // the WebView. These are explicit so a user mpv.conf or stale runtime
        // property can never stretch/crop the embedded picture.
        set_option(mpv_set_option_string, handle, "keepaspect", "yes")?;
        set_option(mpv_set_option_string, handle, "keepaspect-window", "no")?;
        set_option(mpv_set_option_string, handle, "video-aspect-override", "no")?;
        set_option(mpv_set_option_string, handle, "panscan", "0")?;
        set_option(mpv_set_option_string, handle, "video-unscaled", "no")?;
        set_option(mpv_set_option_string, handle, "video-zoom", "0")?;
        set_option(mpv_set_option_string, handle, "video-align-x", "0")?;
        set_option(mpv_set_option_string, handle, "video-align-y", "0")?;
        set_option(mpv_set_option_string, handle, "vo", "gpu-next")?;
        set_option(mpv_set_option_string, handle, "gpu-api", "auto")?;
        set_option(mpv_set_option_string, handle, "hwdec", "auto")?;
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
            mpv_destroy(handle);
            bail!("libmpv initialization failed");
        }
        if start_position_ms > 0 {
            command(
                mpv_command,
                handle,
                &[
                    "loadfile",
                    source,
                    "replace",
                    "-1",
                    &format!("start={:.3}", start_position_ms as f64 / 1000.0),
                ],
            )?;
        } else {
            command(mpv_command, handle, &["loadfile", source])?;
        }

        let mut position = start_position_ms as f64 / 1000.0;
        let mut duration = 0.0f64;
        let mut sample_tick = 0u32;
        let mut stopped = false;
        while !stopped {
            let mut message: MSG = std::mem::zeroed();
            while PeekMessageW(&mut message, ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            while let Ok(next) = receiver.try_recv() {
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
                    PlayerCommand::Stop => stopped = true,
                }
            }
            let event = mpv_wait_event(handle, 0.0);
            if !event.is_null() {
                match (*event).event_id {
                    1 => stopped = true,
                    7 => {
                        let end = (*event).data.cast::<MpvEventEndFile>();
                        if !end.is_null() && (*end).error < 0 {
                            if let Ok(mut current) = state.lock() {
                                current.loading = false;
                                current.error = Some(format!(
                                    "The selected source could not be played (libmpv error {}). Try refreshing sources or choosing another result.",
                                    (*end).error
                                ));
                            }
                        }
                    }
                    // MPV_EVENT_PLAYBACK_RESTART occurs after playback actually
                    // has a frame ready. MPV_EVENT_FILE_LOADED (8) is too early
                    // and caused the loading cover to reveal a blank video plane.
                    21 => {
                        if let Ok(mut current) = state.lock() {
                            current.loading = false;
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
                if let Ok(mut current) = state.lock() {
                    current.active = true;
                    current.paused = paused != 0;
                    current.position_ms = (position * 1000.0) as i64;
                    current.duration_ms = (duration * 1000.0) as i64;
                    current.volume = volume.round() as i64;
                    current.muted = muted != 0;
                }
            }
            thread::sleep(Duration::from_millis(15));
        }
        if position > 0.0 && duration > 0.0 {
            on_progress((position * 1000.0) as i64, (duration * 1000.0) as i64);
        }
        mpv_destroy(handle);
        if let Ok(mut current) = state.lock() {
            current.active = false;
            current.loading = false;
        }
        Ok(())
    }
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
