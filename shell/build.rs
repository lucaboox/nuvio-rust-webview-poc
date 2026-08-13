fn main() {
    println!("cargo:rerun-if-changed=assets/nuvio.ico");
    // `include_dir!` bakes ui/dist into the binary at compile time, but cargo
    // does not track files a proc macro reads. Without this, rebuilding after a
    // UI-only change can silently ship the previously embedded bundle.
    println!("cargo:rerun-if-changed=../ui/dist");
    #[cfg(windows)]
    {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("assets/nuvio.ico");
        resource
            .compile()
            .expect("failed to embed the Nuvio Windows icon");
    }
}
