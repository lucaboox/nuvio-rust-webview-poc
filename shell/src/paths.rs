use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::Once;

/// Files this client owns, for moving out of the directory it used to share.
const OWNED_FILES: [&str; 3] = ["downloads.json", "download-settings.json", "backend.json"];

static MIGRATE: Once = Once::new();

fn local_data_root() -> PathBuf {
    env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| env::var_os("XDG_DATA_HOME").map(PathBuf::from))
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .unwrap_or_else(env::temp_dir)
}

/// This client's data directory.
///
/// Named apart from the official desktop app on purpose. Both are "Nuvio", and
/// sharing a directory put this client's files in among that app's cache.
pub fn app_data_dir() -> PathBuf {
    let dir = local_data_root().join("Nuvio Rust");
    MIGRATE.call_once(|| migrate_from_shared_dir(&dir));
    dir
}

/// Moves what this client wrote in the shared directory into its own.
///
/// Only the files listed above, and only when the destination has none: the old
/// directory still belongs to the official app, so everything else there is left
/// alone. Failures are ignored — a lost settings file is not worth refusing to
/// start over.
fn migrate_from_shared_dir(destination: &PathBuf) {
    let previous = local_data_root().join("Nuvio");
    if !previous.is_dir() || previous == *destination {
        return;
    }
    for name in OWNED_FILES {
        let from = previous.join(name);
        let to = destination.join(name);
        if !from.is_file() || to.exists() {
            continue;
        }
        if fs::create_dir_all(destination).is_err() {
            return;
        }
        if fs::rename(&from, &to).is_err() {
            let _ = fs::copy(&from, &to).map(|_| fs::remove_file(&from));
        }
    }
}
