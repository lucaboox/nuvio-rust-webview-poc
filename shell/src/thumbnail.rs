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
    collections::{VecDeque, hash_map::DefaultHasher},
    ffi::{CString, c_char, c_void},
    fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    ptr,
    sync::TryLockError,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use libloading::Library;

/// Scrubbing fast can outrun the decoder; one capture at a time keeps the
/// player's own connection from competing with a queue of stale requests.
static CAPTURE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Reuses frames across player remounts and repeated visits to the same part
/// of a stream. Only a hash of the (potentially signed) URL and headers is kept
/// in memory, never the credentials themselves.
static FRAME_CACHE: std::sync::OnceLock<std::sync::Mutex<ThumbnailCache>> =
    std::sync::OnceLock::new();
const FRAME_CACHE_LIMIT: usize = 64;

#[derive(Default)]
struct ThumbnailCache {
    frames: VecDeque<(u64, Vec<u8>)>,
}

impl ThumbnailCache {
    fn get(&mut self, key: u64) -> Option<Vec<u8>> {
        let index = self
            .frames
            .iter()
            .position(|(candidate, _)| *candidate == key)?;
        let frame = self.frames.remove(index)?;
        let bytes = frame.1.clone();
        self.frames.push_back(frame);
        Some(bytes)
    }

    fn insert(&mut self, key: u64, bytes: Vec<u8>) {
        if let Some(index) = self
            .frames
            .iter()
            .position(|(candidate, _)| *candidate == key)
        {
            self.frames.remove(index);
        }
        self.frames.push_back((key, bytes));
        while self.frames.len() > FRAME_CACHE_LIMIT {
            self.frames.pop_front();
        }
    }
}

fn cache() -> &'static std::sync::Mutex<ThumbnailCache> {
    FRAME_CACHE.get_or_init(|| std::sync::Mutex::new(ThumbnailCache::default()))
}

fn frame_key(url: &str, request_headers: &[String], position_ms: i64) -> u64 {
    let mut hasher = DefaultHasher::new();
    url.hash(&mut hasher);
    request_headers.hash(&mut hasher);
    // The UI asks in ten-second buckets. Normalising here also coalesces any
    // direct callers that supply a nearby timestamp.
    (position_ms.max(0) / 10_000).hash(&mut hasher);
    hasher.finish()
}

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
    let key = frame_key(url, request_headers, position_ms);
    if let Ok(mut cache) = cache().lock()
        && let Some(bytes) = cache.get(key)
    {
        return Ok(bytes);
    }

    // Never let obsolete pointer positions queue for several seconds. The UI
    // keeps the newest pending bucket and retries it after the active request.
    let _guard = match CAPTURE_LOCK.try_lock() {
        Ok(guard) => guard,
        Err(TryLockError::WouldBlock) => bail!("thumbnailer is busy"),
        Err(TryLockError::Poisoned(_)) => bail!("thumbnailer was interrupted"),
    };
    if let Ok(mut cache) = cache().lock()
        && let Some(bytes) = cache.get(key)
    {
        return Ok(bytes);
    }
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
    let bytes = bytes?;
    if let Ok(mut cache) = cache().lock() {
        cache.insert(key, bytes.clone());
    }
    Ok(bytes)
}

fn capture_into(
    dll_path: &PathBuf,
    url: &str,
    request_headers: &[String],
    position_ms: i64,
    out_dir: &Path,
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

        let raw_handle = mpv_create();
        if raw_handle.is_null() {
            bail!("mpv_create failed for the thumbnailer");
        }
        let handle_guard = MpvHandle {
            raw: raw_handle,
            destroy: mpv_destroy,
        };
        let handle = handle_guard.raw;

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
            if try_set(&format!("vo-image-{suffix}"), value)?
                || try_set(&format!("image-{suffix}"), value)?
            {
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
        set(
            "start",
            &format!("{:.3}", position_ms.max(0) as f64 / 1000.0),
        )?;
        if !request_headers.is_empty() {
            set("http-header-fields", &request_headers.join(","))?;
        }

        if mpv_initialize(handle) < 0 {
            bail!("thumbnailer initialisation failed");
        }

        let load = CString::new("loadfile")?;
        let source = CString::new(url)?;
        let args = [load.as_ptr(), source.as_ptr(), ptr::null()];
        if mpv_command(handle, args.as_ptr()) < 0 {
            bail!("the thumbnailer could not open this source");
        }

        // MPV_EVENT_END_FILE = 7, MPV_EVENT_SHUTDOWN = 1.
        let deadline = Instant::now() + CAPTURE_TIMEOUT;
        loop {
            if Instant::now() >= deadline {
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
        drop(handle_guard);
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

#[cfg(test)]
mod tests {
    use super::frame_key;

    #[test]
    fn frame_keys_coalesce_the_same_ten_second_bucket() {
        let headers = vec!["Authorization: token".to_string()];
        assert_eq!(
            frame_key("https://example/video", &headers, 20_001),
            frame_key("https://example/video", &headers, 29_999)
        );
        assert_ne!(
            frame_key("https://example/video", &headers, 20_001),
            frame_key("https://example/video", &headers, 30_000)
        );
    }

    #[test]
    fn frame_keys_include_source_and_headers() {
        assert_ne!(
            frame_key("https://example/one", &[], 0),
            frame_key("https://example/two", &[], 0)
        );
        assert_ne!(
            frame_key("https://example/one", &[], 0),
            frame_key("https://example/one", &["Range: one".to_string()], 0)
        );
    }
}
