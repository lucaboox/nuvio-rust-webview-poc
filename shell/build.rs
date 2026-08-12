fn main() {
    println!("cargo:rerun-if-changed=assets/nuvio.ico");
    #[cfg(windows)]
    {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("assets/nuvio.ico");
        resource
            .compile()
            .expect("failed to embed the Nuvio Windows icon");
    }
}
