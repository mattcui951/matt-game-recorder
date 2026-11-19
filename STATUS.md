# Status

Updated: 2026-11-19
Version: 1.0.0, released 2026-11-19

## In progress

- Nothing open.

## Unverified

- Aatrox key fix: unit-tested only; check his icon on the next Aatrox match. 3 older matches (11-05 Match 04, 11-07 Match 04 and 07) keep kill events naming "Name", champion unknown
- 0.4.1 full screen player: controls lie over the video and fade when the pointer rests, also
  while paused. The browser preview cannot enter full screen, so this was not seen working.

## Next

- No open requests. Ideas raised along the way: Highlights mode saves disk but not CPU or GPU,
  because the game is still encoded into a memory buffer to have the seconds before each event.

## Known issues

- Black stretches inside a real match (seen 11-15 Match 01, about 19%) happen while the champion
  stands in the fountain, so most likely the player was tabbed out of fullscreen League. A
  minimized mock game does not reproduce it (Windows keeps the last frame).
- A silent `/S` reinstall drops the Public Desktop shortcut; it has to be recreated.
- A yellow border around the game while recording is not the recorder: its FFmpeg capture asks for
  no border and Windows allows it (desktop apps may turn off the border). It comes from another app
  capturing the screen without that permission, on 2026-11-15 the Codex app's computer use
  (packaged, border permission "Prompt"). Check `ConsentStore\graphicsCaptureWithoutBorder`
  under HKCU CapabilityAccessManager: `LastUsedTimeStop` 0 means a capture is running now.

## Recently done

- (1.0.0) Russian (Русский) as the eighth UI language, in the frontend catalog and every backend `tr!`;
  Russian plurals in the UI, "label: N" wording for counts in backend messages. Checked library,
  match and every Settings page in the dev app; translations not read by a native speaker.
- (0.4.5) Seven UI languages: English, 简体中文, 繁體中文 (Taiwan wording and Taiwan
  League client terms), Español, Français, 日本語, 한국어. "Match Windows" maps the Windows
  display language (zh-TW/HK/MO give Traditional). Backend text (notifications, tray, job progress,
  errors) uses `tr!` with every language required at compile time; champion names come from Data
  Dragon in each language. Checked library, match, editor and every Settings page in the dev app;
  translations are mine and have not been read by native speakers.
- (0.4.4) New look "Internet Café / 网吧" (default theme, plus a light pair; old themes stay):
  graphite and neon green, Rajdhani numerals, 4/6/10px corners; today's play clock in the title bar;
  receipt tail on library rows; the match video box takes the recording's aspect ratio (no
  letterbox on ultrawide); match subtitle shows the date instead of repeating the title. Existing
  installs keep their chosen theme until switched in Appearance.
- (0.4.4) Pointer feedback (taste skill, app-suited rules only): buttons rise 1px and
  press in, grid cards lift 2px, thumbnails zoom 3% in their frame, link underlines draw in; one
  global .link; reduced motion stills it all.
- Less duplicated code (0.4.3). FFmpeg render, supercut render and import conversion
  share one runner (`ffmpeg::run_progress`), covered by new tests on generated videos; one
  confirm dialog component; job results offered through `offerResult` (no polling); shared
  settings-page styles in `app.css`; supercut/clip names translated in notifications and job
  titles too. Settings pages compared pixel for pixel before and after.
- (0.4.3) The back arrow on a match always returns to the match list (it used to step
  back through every match that had opened itself after a game); date bar day chips read
  "Nov 5 Thu 7 matches" / "11月5日 周四 7局"; thumbnails skip black frames
- Champion key from raw names with a suffix (Aatrox showed "A" and no art); fixed the 11-13 match by hand (0.4.2)
- Player controls sit under the video, so they no longer cover the game's HUD (0.4.1)
- APM off by default; with it off only the keyboard is watched (0.4.0)
- Lighter on the PC: Raw Input for APM, idle results check, leaner encoder option
- Supercuts: kill tiers (double to penta), across every match in a date range
- Imports: instant move on the same drive, delete Outplayed originals as you go; settings that stick
- Match timeline: rows by importance, grouped marks, filter chips, champions instead of names
- Installable from GitHub releases, updates itself (0.2.0)
