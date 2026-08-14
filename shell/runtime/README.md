# Windows libmpv runtime

Windows builds require `libmpv-2.dll` in this directory. The DLL is intentionally not committed because it is larger than GitHub's normal file limit.

Run `npm run prepare:runtime` to download and verify the pinned runtime before invoking Cargo directly. Tauri builds run this preparation step automatically.

- Runtime: mpv `v0.40.0-465-gf6c116491`
- Upstream commit: <https://github.com/mpv-player/mpv/commit/f6c116491>
- Pinned asset: <https://github.com/lucaboox/nuvio-rust-webview-poc/releases/tag/runtime-libmpv-v0.40.0-465-gf6c116491>
- SHA-256: `07c68bb211f23a218ded0a36eb12207dc3aeb44e5318ffca6ce9dcc7c3173906`

The dependency release is marked as a prerelease so the application updater's `/releases/latest/` endpoint continues to resolve only application releases.
