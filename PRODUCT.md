# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

(Desktop app: Tauri 2 shell with a WebView2 UI on Windows 11. Design language is the app's own, not native Windows controls.)

## Stack

Tauri 2 + Rust backend, Svelte 5 + TypeScript + Vite frontend. FFmpeg 8.x (bundled) does capture (Windows Graphics Capture `gfxcapture`) and NVENC encoding. Chosen by the user over Electron for the lowest RAM/CPU footprint.

## Users

One person: Matt, a League of Legends player on Windows (RTX 4060 Ti, 3440x1440 ultrawide plus a second monitor). He plays ranked/normal/ARAM games and opens the app after a match.

## Product Purpose

Matt Game Recorder automatically records League of Legends matches (full match or highlights only) with game audio and microphone, marks game events on the timeline, and lets him review, trim and edit locally. It replaces Outplayed for his use, without ads, cloud, or community features.

Success: he finishes a game, opens the match, and jumps straight to the moments that matter (deaths first for review, then fights, objectives) with zero setup, while the recorder never costs him frames in game.

## Positioning

A personal, local-only recorder tuned for one game and one machine: GPU-only capture pipeline, AV1 files about half the size of Outplayed's, no ads, no account, and an event timeline built from Riot's Live Client Data API.

## Operating Context

- Runs in the tray while he plays; must use as little CPU, RAM and disk as possible.
- Primary post-game job (confirmed): reviewing his own gameplay. Clip making (quick trim, instant supercut, editor) is secondary but must exist.
- Opened on a large ultrawide monitor, usually right after a match ends (auto-open setting), sometimes mid-session on the second monitor (game dashboard).
- Media lives on F:\Matt Game Recorder by default.

## Capabilities and Constraints

- Capture modes per game: Manual only, Highlights, Full match, Full session. FPS selectable (10/20/30/60/90/120; 30 and 60 are the ones he cares about).
- Audio: selectable output device (loopback) and input device, with volumes; option to capture only the game's audio; force mic to mono.
- Events: Victory, Kill, Death, Assist, multikills (double to penta), Ultimate, Baron, Dragon, Rift Herald, Turret destroyed, Inhibitor destroyed, manual bookmarks.
- Library: sessions grouped by game/date, Favorites collection, Recently deleted with retention days.
- Match viewer: event timeline with filters, APM graph, quick trim, instant supercut, open in editor.
- Editor: projects, multi-clip timeline, slice/duplicate/remove, aspect ratio, crop, volume, undo/redo, export.
- Settings mirror Outplayed's general and per-game League of Legends settings, minus "Report an issue".
- Top bar: disk space meter, game events status, screen recorder, app layout switch.
- Out of scope: ads, cloud upload, community feed, account, other games (for now).
- Open: Discord rich presence needs a Discord application ID the user has not provided yet.

## Brand Commitments

Name: "Matt Game Recorder". No logo or palette supplied. Must not reuse Outplayed's branding, artwork or icons.

## Evidence on Hand

None. Only real recordings made by the app may appear; do not fabricate matches, stats or screenshots in shipped UI.

## Product Principles

1. The game comes first: recording must never be the reason a frame drops.
2. Review over flourish: the fastest path is match, then the moment, then playing it.
3. Local and lean: every byte on disk and every MB of RAM has to earn its place.
4. Familiar to an Outplayed user: same concepts and settings, so nothing has to be relearned.
