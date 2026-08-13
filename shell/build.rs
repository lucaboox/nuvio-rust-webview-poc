fn main() {
    // `tauri::generate_context!` bakes ui/dist into the binary at compile time,
    // but cargo does not track files a proc macro reads. Without this,
    // rebuilding after a UI-only change can silently ship the previous bundle.
    println!("cargo:rerun-if-changed=../ui/dist");
    println!("cargo:rerun-if-changed=tauri.conf.json");
    // Embeds the Windows icon and the app manifest, replacing the hand-rolled
    // winresource step.
    tauri_build::build();
}
