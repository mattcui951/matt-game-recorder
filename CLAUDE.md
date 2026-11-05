# Matt Game Recorder: notes for Claude

A Windows game recorder for League of Legends (Tauri 2 + Rust + Svelte 5, FFmpeg for capture),
an Outplayed replacement. Public repo, released through GitHub with an in-app updater.
`DESIGN.md` is the visual design system and `PRODUCT.md` the product brief.

## Start here

- `STATUS.md`: where the app stands, what is next, known issues. Read it before starting.
- `README.md`: develop, test without playing (`tools/mock_lol.py`), preview the UI, build, release.

## Rules

- Before you finish a piece of work, update `STATUS.md`: move finished items to "Recently done",
  add what is next, note anything left unverified. Keep it short.
- Never stop, restart or reinstall the installed app while a match is being recorded: check that
  `League of Legends.exe` is not running first. A restart cuts the recording.
- Only one instance runs (single-instance plugin), so a `tauri dev` copy exits at once while the
  installed app runs.
- Follow `DESIGN.md` for every UI change.

## Releasing

Bump the version in `package.json`, `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json`,
commit, then push an annotated tag (`git tag -a vX.Y.Z -m "notes"`). Never publish a release that
could become "latest" without `latest.json`. Do not wait on the build in the foreground; watch it
with a background command (`gh run watch <id> --exit-status`) and carry on.
