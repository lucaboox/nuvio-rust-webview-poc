# Releasing Nuvio

Nuvio uses semantic versions internally so the Tauri updater can compare builds safely. Human-facing versions are formatted in the app; for example, `0.1.0-alpha.1` is displayed as **0.1 Alpha 1**.

## Windows installer

The Windows release is one NSIS setup executable. Its first page lets the user choose:

- **Current user** — installs under `%LOCALAPPDATA%`.
- **All users** — installs under `Program Files`.

Because this installer offers both choices, Windows asks for administrator approval when setup starts. Updates run in Tauri's passive mode: after the user confirms inside Nuvio, the signed update downloads, setup runs without further choices, and the app restarts. An all-users installation may show a UAC prompt.

The updater is cross-platform. Windows is the only release job enabled today; later Linux and macOS jobs can publish their signed artifacts into the same `latest.json` release manifest.

## One-time GitHub setup

The update keypair and local password backup live in `.tauri/` and are ignored by Git. Back up `.tauri/nuvio-updater.key` and `.tauri/nuvio-updater-password.txt` somewhere secure. If either is lost, existing installs cannot trust future releases.

Create these repository settings:

1. Add the private key as the GitHub Actions secret `TAURI_SIGNING_PRIVATE_KEY`:

   ```powershell
   Get-Content -Raw .tauri\nuvio-updater.key | gh secret set TAURI_SIGNING_PRIVATE_KEY
   ```

2. Add the password from `.tauri/nuvio-updater-password.txt` as the GitHub Actions secret `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.
3. Add the public client configuration as GitHub Actions repository variables: `NUVIO_SUPABASE_URL`, `NUVIO_SUPABASE_FALLBACK_URL`, and `NUVIO_SUPABASE_ANON_KEY`.

Tauri updater signatures protect downloaded updates. They are separate from Microsoft Authenticode signing; until a Windows code-signing certificate is configured, SmartScreen can still show an unknown-publisher warning for a first-time install.

## Publish a version

From a clean release branch:

```powershell
npm run version:set -- 0.1.0-alpha.2
# Move the shipped notes into a dated [0.1.0-alpha.2] section in CHANGELOG.md.
npm run release:notes
npm run prepare:runtime
npm run check:shell-ui
cargo test --manifest-path shell/Cargo.toml
git add CHANGELOG.md package.json package-lock.json shell/Cargo.toml shell/Cargo.lock shell/tauri.conf.json
git commit -m "Release 0.1.0-alpha.2"
git tag v0.1.0-alpha.2
git push origin HEAD --tags
```

The `Release Nuvio` workflow validates that the tag matches `tauri.conf.json` and that `CHANGELOG.md` contains a matching non-empty version section. It uses that section as the GitHub Release description, builds the x64 NSIS installer, signs its updater artifact, and uploads `latest.json`. The app checks that manifest for updates and reads the public GitHub Releases feed for its in-app changelog.

The workflow downloads the pinned libmpv Windows runtime from the repository's `runtime-libmpv-v0.40.0-465-gf6c116491` dependency release and verifies its recorded SHA-256 before compiling. That dependency release remains a GitHub prerelease so it cannot replace the application updater's latest release.

Alpha tags are deliberately published as normal GitHub releases for now. GitHub's `/releases/latest/` endpoint excludes releases marked as prereleases, so marking the release as a GitHub prerelease would make this update channel invisible.

## Local installer build

Use the ignored local key when creating a test installer:

```powershell
$env:TAURI_SIGNING_PRIVATE_KEY = Get-Content -Raw .tauri\nuvio-updater.key
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ""
npm run bundle
```

If your terminal still prompts while using an unencrypted local key, the explicit equivalent is:

```powershell
npm run tauri -- signer sign --private-key-path .tauri\nuvio-updater.key --password= shell\target\release\bundle\nsis\Nuvio_0.1.0-alpha.1_x64-setup.exe
```

The setup executable and signature are written below `shell/target/release/bundle/nsis/`.
