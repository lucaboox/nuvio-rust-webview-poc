//! Seek-bar thumbnails.
//!
//! Decodes a single frame at a timestamp using the libmpv already bundled for
//! playback — no ffmpeg binary needed, since that DLL is a full FFmpeg build.
//!
//! This does not require the video to be buffered to that point. mpv issues an
//! HTTP range request for the bytes around the target, decodes one frame and
//! discards the rest, so scrubbing to the end of a two-hour file costs a few
//! hundred KB rather than the whole stream.

use std::{
    ffi::{CString, c_char, c_void},
    fs,
    path::PathBuf,
    ptr,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use libloading::Library;

/// Scrubbing fast can outrun the decoder; one capture at a time keeps the
/// player's own connection from competing with a queue of stale requests.
static CAPTURE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// libmpv-2.dll pulls in the whole FFmpeg stack, so loading and unloading it
/// per capture cost more than the decode itself. The path never changes within
/// a run, so hold it open for the life of the process.
static LIBRARY: std::sync::OnceLock<Option<Library>> = std::sync::OnceLock::new();

fn library(dll_path: &PathBuf) -> Result<&'static Library> {
    LIBRARY
        .get_or_init(|| unsafe { Library::new(dll_path) }.ok())
        .as_ref()
        .context("could not load libmpv-2.dll")
}

type MpvCreate = unsafe extern "C" fn() -> *mut c_void;
type MpvSetOptionString = unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> i32;
type MpvInitialize = unsafe extern "C" fn(*mut c_void) -> i32;
type MpvCommand = unsafe extern "C" fn(*mut c_void, *const *const c_char) -> i32;
type MpvWaitEvent = unsafe extern "C" fn(*mut c_void, f64) -> *const MpvEvent;
type MpvDestroy = unsafe extern "C" fn(*mut c_void);

#[repr(C)]
struct MpvEvent {
    event_id: i32,
    error: i32,
    reply_userdata: u64,
    data: *mut c_void,
}

/// Frames wider than this add latency without adding useful detail at the size
/// a scrub preview is drawn.
const THUMBNAIL_WIDTH: i32 = 320;
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(6);

pub fn capture(
    dll_path: &PathBuf,
    url: &str,
    request_headers: &[String],
    position_ms: i64,
) -> Result<Vec<u8>> {
    let _guard = CAPTURE_LOCK.lock().map_err(|_| anyhow::anyhow!("thumbnailer was interrupted"))?;
    let out_dir = std::env::temp_dir().join(format!(
        "nuvio-thumb-{}-{}",
        std::process::id(),
        position_ms.max(0)
    ));
    // A stale directory would let us read the previous request's frame.
    let _ = fs::remove_dir_all(&out_dir);
    fs::create_dir_all(&out_dir).context("could not create the thumbnail directory")?;

    let result = capture_into(dll_path, url, request_headers, position_ms, &out_dir);
    let bytes = result.and_then(|()| newest_jpeg(&out_dir));
    let _ = fs::remove_dir_all(&out_dir);
    bytes
}

fn capture_into(
    dll_path: &PathBuf,
    url: &str,
    request_headers: &[String],
    position_ms: i64,
    out_dir: &PathBuf,
) -> Result<()> {
    unsafe {
        let library = library(dll_path)?;
        let mpv_create = *library.get::<MpvCreate>(b"mpv_create\0")?;
        let mpv_set_option_string =
            *library.get::<MpvSetOptionString>(b"mpv_set_option_string\0")?;
        let mpv_initialize = *library.get::<MpvInitialize>(b"mpv_initialize\0")?;
        let mpv_command = *library.get::<MpvCommand>(b"mpv_command\0")?;
        let mpv_wait_event = *library.get::<MpvWaitEvent>(b"mpv_wait_event\0")?;
        let mpv_destroy = *library.get::<MpvDestroy>(b"mpv_terminate_destroy\0")?;

        let handle = mpv_create();
        if handle.is_null() {
            bail!("mpv_create failed for the thumbnailer");
        }

        let try_set = |name: &str, value: &str| -> Result<bool> {
            let name_c = CString::new(name)?;
            let value_c = CString::new(value)?;
            Ok(mpv_set_option_string(handle, name_c.as_ptr(), value_c.as_ptr()) >= 0)
        };
        let set = |name: &str, value: &str| -> Result<()> {
            if !try_set(name, value)? {
                bail!("libmpv rejected thumbnail option {name}");
            }
            Ok(())
        };
        // vo_image's suboptions were renamed to a `vo-image-` prefix; which
        // spelling a given libmpv accepts depends on its vintage, so take
        // whichever one it recognises.
        let set_vo_image = |suffix: &str, value: &str| -> Result<()> {
            if try_set(&format!("vo-image-{suffix}"), value)? || try_set(&format!("image-{suffix}"), value)? {
                return Ok(());
            }
            bail!("libmpv accepted neither vo-image-{suffix} nor image-{suffix}")
        };

        set("config", "no")?;
        set("terminal", "no")?;
        set("osc", "no")?;
        // Nothing but one video frame is wanted, and hardware decode would
        // contend with the player already using the GPU.
        set("audio", "no")?;
        set("sub", "no")?;
        set("sub-auto", "no")?;
        set("hwdec", "no")?;
        set("vo", "image")?;
        set_vo_image("format", "jpg")?;
        set_vo_image("jpeg-quality", "80")?;
        set_vo_image("outdir", &out_dir.to_string_lossy())?;
        set("vf", &format!("scale={THUMBNAIL_WIDTH}:-2"))?;
        set("frames", "1")?;
        set("keep-open", "no")?;
        // Speed hints. Each is optional — an mpv that does not know one should
        // still produce a frame, just more slowly, so rejection is not fatal.
        for (name, value) in [
            // Land on the nearest keyframe instead of decoding forward to an
            // exact timestamp. Preview accuracy of a second or two is fine.
            ("hr-seek", "no"),
            // No point filling a read-ahead cache for a single frame.
            ("cache", "no"),
            ("demuxer-readahead-secs", "0"),
            ("vd-lavc-skiploopfilter", "all"),
            ("vd-lavc-skipidct", "nonref"),
            ("vd-lavc-fast", "yes"),
            // Nothing here should be resolving playlists or running scripts.
            ("ytdl", "no"),
            ("load-scripts", "no"),
            ("load-auto-profiles", "no"),
        ] {
            let _ = try_set(name, value)?;
        }
        // A dead link must fail fast rather than hold the scrub preview open.
        set("network-timeout", "5")?;
        set("start", &format!("{:.3}", position_ms.max(0) as f64 / 1000.0))?;
        if !request_headers.is_empty() {
            set("http-header-fields", &request_headers.join(","))?;
        }

        if mpv_initialize(handle) < 0 {
            mpv_destroy(handle);
            bail!("thumbnailer initialisation failed");
        }

        let load = CString::new("loadfile")?;
        let source = CString::new(url)?;
        let args = [load.as_ptr(), source.as_ptr(), ptr::null()];
        if mpv_command(handle, args.as_ptr()) < 0 {
            mpv_destroy(handle);
            bail!("the thumbnailer could not open this source");
        }

        // MPV_EVENT_END_FILE = 7, MPV_EVENT_SHUTDOWN = 1.
        let deadline = Instant::now() + CAPTURE_TIMEOUT;
        loop {
            if Instant::now() >= deadline {
                mpv_destroy(handle);
                bail!("timed out decoding the thumbnail");
            }
            let event = mpv_wait_event(handle, 0.25);
            if event.is_null() {
                continue;
            }
            match (*event).event_id {
                7 | 1 => break,
                _ => {}
            }
        }
        mpv_destroy(handle);
    }
    Ok(())
}

fn newest_jpeg(out_dir: &PathBuf) -> Result<Vec<u8>> {
    let entry = fs::read_dir(out_dir)
        .context("thumbnail directory could not be read")?
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("jpg"))
        })
        .max_by_key(|entry| {
            entry
                .metadata()
                .and_then(|meta| meta.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
        })
        .context("the thumbnailer produced no frame")?;
    fs::read(entry.path()).context("the thumbnail frame could not be read")
}
