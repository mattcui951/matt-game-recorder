# Status

Updated: 2026-11-15
Version: 0.4.2, released 2026-11-14; next changes below are not released yet

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

- Not released yet: the back arrow on a match always returns to the match list (it used to step
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
