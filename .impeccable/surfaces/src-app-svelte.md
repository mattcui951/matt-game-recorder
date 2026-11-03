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

THESIS: The recorder is a cabinet's high-score memory: every match is a scored run you can replay, and every event is a lit mark sized to the seconds it captured. Refuses the category default of grey cards plus one pink accent and dot-sized timeline icons.

OWN-WORLD: Deep indigo cabinet ground (never pure black), raised panels one step lighter with 1px seams. Full phosphor palette with jobs: cyan = you (kills, multikills, your APM), magenta-red = deaths, gold = objectives (turret, inhib, dragon, baron, herald), mint = victory/healthy states, violet = assists/ultimate, amber = warnings. Chunky condensed numerals (tabular) for every score, time and size readout; a quiet UI sans for labels and prose. Square-ish corners (2-4px), no glows, no scanlines, no pixel fonts.

STORY: He sees his latest run on top, KDA and result at a glance, opens it, and the timeline already tells the match story by color: red clusters are where he died. One click jumps to a moment; filters and supercut turn the marks into clips.

FIRST VIEWPORT: Left rail (Library, Editor, Settings). Top bar: app name left, right side disk meter, events status light, screen recorder, layout. Library: a high-score table of sessions grouped by day, each row/tile with champion portrait, result band, KDA in score numerals, duration, size; latest match first. Match viewer: video fills the work area; below it the event lane with colored marks whose width equals the clip window, APM sparkline under it, tools at the lane's right end.

FORM: Arcade high-score / attract-screen world, candidate 7 of 7 on the grounded list (assigned by the roll). seed:ab1d21bb69ce9627
Raise (from Labanotation, declined): length is exact duration; event marks draw their real before/after capture window, not a dot.
Raise (from Jet-age ticket wallet, declined): nothing disappears, it cancels; deleted media keeps a struck VOID state in Recently deleted until purged.
Raise (from Crouwel grid, declined): one visible cell grid; library tiles and settings rows snap to a single column module.
Competitive alternate noted: Graphite console (product clarity), not chosen.

FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance

## Unresolved
- Discord rich presence needs a Discord application ID.
