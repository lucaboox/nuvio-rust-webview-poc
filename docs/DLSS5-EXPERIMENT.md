# DLSS 5 experiment — preflight only

Branch: `codex/dlss5-experiment`. Normal playback is unchanged. There is no DLSS
playback toggle or working DLSS integration in this branch yet.

The stable shared UI was already committed at `efe9101`; it was pushed to web
`main` and GitHub Pages deployed successfully (run `33937733007`). Native baseline
is `2399dd8`. Only this native repository has experimental changes; neither shared
UI checkout has been edited for the experiment.

## Local bundle check

Run `npm run dlss5:check -- "path/to/DLSS test"` and `npm run test:dlss5`.
The checker reads headers/files only. It does not execute, install, copy, download,
or trust binaries. Exit 1 means missing/incompatible basic files; exit 2 means an
inspection error. Exit 0 is **not** proof of neural rendering, shader compatibility,
authenticity, addon support, or a supported GPU.

Inspection of the supplied bundle on 2026-09-04:

- ReShade `dxgi.dll`: PE machine `0x014c` (32-bit), incompatible with our x64 host.
- Feeder and both NVIDIA DLLs: PE machine `0x8664` (x64).
- No `DLSS5_Feed.fx`, motion-vector provider package, or neural consumer present.
- ReShade preset is empty.

The [Feeder upstream instructions](https://github.com/jlrouzies-fr/DLSS5-Feeder)
require an addon-capable ReShade runtime, feeder shader, configured motion provider,
and one compatible neural consumer. The checker targets this route only. Other
routes need different checks; passing basic file inventory is insufficient.

## How this differs from existing RTX enhancement

Our `shell/src/player/native.rs` uses libmpv `vo=gpu-next` in a native child window.
Ordinary playback chooses its graphics API automatically. RTX VSR explicitly uses
D3D11 and `d3d11vpp=scale=2:scaling-mode=nvidia`. That is not DLSS neural rendering.
Windows DXGI is part of D3D presentation; the provided `dxgi.dll` is a ReShade
replacement/proxy, not the Windows library we should overwrite.

Do not place the supplied proxy beside the working app. Hooks could affect more
than the video swapchain. Never install into Windows directories, alter system
graphics settings, disable security software, or commit proprietary runtime files.

## Integration decision and remaining work

First obtain a trusted complete **x64** user-supplied runtime package. Then test
in a separate disposable build directory with local test media, not the stable
executable. Establish actual successful neural evaluations, not just loaded DLLs
or an "enabled" flag. Check pause, seek, fullscreen, repeated open/close, audio
sync, subtitle rendering and video-only targeting before adding a setting.

A direct integration is not just `LoadLibrary(nvngx_dlssnr.dll)`. The application
would need a GPU frame-processing path, temporal inputs, resource synchronization,
and compatible NGX invocation. Our current libmpv child-window presentation does
not expose that pipeline to the Rust host. A standalone rendering helper or custom
renderer is a separate implementation effort, not a safe small patch to playback.

The [community DLSS5 video player](https://github.com/2600th/dlss5-video-player)
is a useful reference but its current neural mode preprocesses entire videos into
a cache, rather than performing live neural playback. It is not a drop-in solution
for Nuvio's streaming player.

Keep any eventual experimental setting local to this client, default off, distinct
from the official synced RTX VSR setting. Do not publish this experiment to Pages
or advertise supported DLSS until hardware playback is verified.
