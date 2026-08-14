fn main() {
    // `tauri::generate_context!` bakes ui/dist into the binary at compile time,
    // but cargo does not track files a proc macro reads. Without this,
    // rebuilding after a UI-only change can silently ship the previous bundle.
    println!("cargo:rerun-if-changed=../ui/dist");
    println!("cargo:rerun-if-changed=tauri.conf.json");
    println!("cargo:rerun-if-changed=../.env.local");
    // Supabase's anon key is a public client credential. Embed the same
    // configuration used by development so an installed build does not depend
    // on a source-tree .env.local file being present at runtime.
    if let Ok(values) = dotenvy::from_path_iter("../.env.local") {
        for item in values.flatten() {
            let (name, value) = item;
            if matches!(
                name.as_str(),
                "NUVIO_SUPABASE_URL" | "NUVIO_SUPABASE_FALLBACK_URL" | "NUVIO_SUPABASE_ANON_KEY"
            ) && std::env::var_os(&name).is_none()
            {
                println!("cargo:rustc-env={name}={value}");
            }
        }
    }
    // Embeds the Windows icon and the app manifest, replacing the hand-rolled
    // winresource step.
    tauri_build::build();
}
