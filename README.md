# Matt Game Recorder

A local, ad-free League of Legends recorder for Windows: full-match or highlights capture,
game-event timeline, APM, quick trim, instant supercut and a small video editor. English and
Simplified Chinese.

## Install

Download `Matt.Game.Recorder_*_x64-setup.exe` from the
[latest release](https://github.com/mattcui951/matt-game-recorder/releases/latest) and run it.
The installer is not code-signed, so Windows may say "Windows protected your PC": click
**More info**, then **Run anyway**. Recordings go to `Videos\Matt Game Recorder` until you pick
another folder in Settings > Storage.

## Updates

The app checks the latest GitHub release shortly after it starts and every 6 hours (Settings >
Updates turns this off). A new version shows a banner; nothing installs until you click
**Update now**, and never while League is running or a match is being recorded or saved,
because the installer closes the app. Windows asks for permission (the app installs for all
users), then the new version starts by itself. Recordings, settings and an import in progress
are kept; the import resumes after the restart.

Updates are verified: `latest.json` and the installer's signature are checked against the
public key in `src-tauri/tauri.conf.json` before anything runs.

## How it works

- **Capture**: one `ffmpeg` process grabs the game window with Windows Graphics Capture
  (`gfxcapture`) and encodes it on the GPU (NVENC AV1 by default). Frames never leave the GPU,
  so recording costs well under 1% CPU. The recorder's own windows are never in the video.
- **Audio**: WASAPI loopback of the chosen output device (or only the game process) plus the
  chosen microphone, mixed in Rust and fed to ffmpeg through a named pipe.
- **Files**: ffmpeg writes fragmented MP4 to a pipe. Full match: fragments go to a file that is
  finalized at the end. Highlights/manual: fragments sit in a RAM (or disk) ring buffer and clips
  are cut by copying fragments, no re-encode. Keyframes every 2 s.
- **Events**: Riot's Live Client Data API (`https://127.0.0.1:2999`) is polled once a second.
  Kills, deaths, assists, multikills, enemy turrets/inhibitors, ally dragon/herald/baron and
  victory are mapped to the active player. Ultimate is detected from the ultimate key while
  the game has focus (the API does not report casts). APM comes from low-level input hooks,
  only while a match is recorded and only when the game window is focused.
- **Library**: plain files in the media folder (`Videos\Matt Game Recorder` by default); every
  recording has a `.mgr.json` sidecar with events and APM. Copying the folder is a full backup.

## Importing

Library > Import offers two imports. Both skip anything already imported, keep running in
the background, pause while a game is being recorded, and resume by themselves after a restart.

- **From Outplayed**: finds Outplayed's media folder from its database, re-encodes every video to
  this app's format on the GPU (AV1 + Opus, about 40% smaller) and keeps champion, KDA, result and
  timeline events, read from Outplayed's IndexedDB (`%LOCALAPPDATA%\Overwolf\CefBrowserCache\Default\IndexedDB\overwolf-extension_cghph..._0.indexeddb.leveldb`).
  Outplayed's own files are never changed; its "Recently Deleted" folder is skipped.
  "Use the CPU as a second encoder" (asks first, off at every launch) converts a second video on
  the CPU with SVT-AV1 alongside the GPU. The speed-up shown is measured on each PC: a 5-second
  test encode per encoder, then updated from real conversions (`encoder-speed.json`, pixels/second).
- **From another Matt Game Recorder folder**: copies sessions and recordings (with their events)
  from another media folder or backup, without converting.

Command line (forwarded to the running app): `matt-game-recorder.exe --import-outplayed <folder>`
or `--import-library <folder>`.

## Develop

```bash
npm install
npm run tauri dev
```

Requires Rust (MSVC), Node 20+, and `ffmpeg.exe` / `ffprobe.exe` (FFmpeg 8.x with `gfxcapture`)
in `src-tauri/binaries/` (`ffplay.exe` too for the mock below).

### Test without playing

```bash
py -I tools/mock_lol.py --length 90
```

Starts a stand-in "League of Legends.exe" window with moving video and a tone, and a fake
Live Client API that plays a scripted match. The running app records it like a real game.

### Preview the UI in a browser

With `npm run tauri dev` running, open
`http://localhost:1430/?media=<contents of %APPDATA%\Matt Game Recorder\dev-media-url.txt>`.
Reads come from the running app (debug builds only); edits are no-ops.

## Build

```bash
npm run tauri build
```

Produces an NSIS installer in `src-tauri/target/release/bundle/nsis/`. Signing the updater
artifacts needs `TAURI_SIGNING_PRIVATE_KEY` (the key file's contents) and
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` set; without them the installer still builds.

## Release

```bash
git tag -a v0.3.0 -m "What changed"
git push origin v0.3.0
```

First bump `version` in `package.json`, `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json`.
The tag starts `.github/workflows/release.yml`, which type-checks, runs the Rust tests, builds the
installer, signs it, and publishes the release with `latest.json` (what installed apps read).
The tag message becomes the release notes. The signing key lives outside the repository
(`%USERPROFILE%\.tauri\matt-game-recorder.key`, with its password next to it) and as the
`TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` repository secrets. Losing it
means installed copies can no longer update themselves.

FFmpeg is not in git (165 MB per file). The workflow downloads the pinned build from the
`deps-ffmpeg` release of this repository and checks its SHA-256 (see `FFMPEG_SHA256` in the
workflow). To change FFmpeg, upload a new zip of `ffmpeg.exe` and `ffprobe.exe` there and update
the asset name and hash.

## Third-party software

The installer includes FFmpeg (`ffmpeg.exe`, `ffprobe.exe`) version n8.1.3-27-g8dca1586f4, a
GPL v3 build by [BtbN/FFmpeg-Builds](https://github.com/BtbN/FFmpeg-Builds) (2026-10-31). FFmpeg
is free software licensed under the GPL; its source code is at
[git.ffmpeg.org](https://git.ffmpeg.org/ffmpeg.git) (commit `8dca1586f4`) and the build scripts
at BtbN/FFmpeg-Builds. Matt Game Recorder runs it as a separate program.
