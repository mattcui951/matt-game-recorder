---
version: 1
slug: "src-app-svelte"
primary_target: "src/App.svelte"
related_targets: []
---

# Surface brief: Matt Game Recorder app shell

Scope: the whole desktop app UI (library, match viewer, editor, settings, top bar, game dashboard). Mode: Operate. Build path: code-led.

Audience and job: Matt, one League player, opens the app right after a match to review his own gameplay (confirmed main job), secondarily to cut clips. Must read instantly on a 3440x1440 ultrawide and stay usable down to ~1280x720.

## Direction contract

THESIS: The recorder is your seat at a gaming cafe after dark: a session clock over the seat, lights low, one neon colour for you, and every match leaves a receipt of what it cost. Refuses the category default of grey cards plus one pink accent.

OWN-WORLD: Graphite glass ground (#0d1014), panels one step lighter with 1px seams. Accents with jobs: neon green = you (kills, multikills, APM, selection, primary, session clock), red = deaths, amber = objectives, blue = victory, lavender = assists, orange = warnings. Rajdhani numerals for every score, time and size; Segoe UI / YaHei for prose. 4/6/10px corners, no glow.

STORY: He sees his latest run on top, KDA and result at a glance, opens it, and the timeline already tells the match story by color: red clusters are where he died. One click jumps to a moment; filters and supercut turn the marks into clips.

FIRST VIEWPORT: Left rail (Library, Editor, Settings). Top bar: app name left, right side disk meter, events status light, screen recorder, layout. Library: a high-score table of sessions grouped by day, each row/tile with champion portrait, result band, KDA in score numerals, duration, size; latest match first. Match viewer: video fills the work area; below it the event lane with colored marks whose width equals the clip window, APM sparkline under it, tools at the lane's right end.

FORM: Internet cafe (网吧) session screens and receipts; the user chose IMPECCABLE'S PICK over the roll after one re-roll. seed:7c367d1b819cc78a
Raise (from Labanotation, declined): length is exact duration; event marks draw their real before/after capture window, not a dot.
Raise (from Jet-age ticket wallet, declined): nothing disappears, it cancels; deleted media keeps a struck VOID state in Recently deleted until purged.
Raise (from Crouwel grid, declined): one visible cell grid; library tiles and settings rows snap to a single column module.
Competitive alternate noted: Graphite console (product clarity), not chosen.

FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance

## Unresolved
- Discord rich presence needs a Discord application ID.
