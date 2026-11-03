# Design: Matt Game Recorder

World: an arcade cabinet's high-score memory. Every match is a scored run you can replay;
every game event is a lit mark sized to the seconds it captured. Mode: Operate (a working
tool), so the world lends only type, palette, density and one signature move. Layout,
navigation and controls are the standard ones for a desktop media app.

## Color

Ground is deep indigo, never black. Phosphor colors each carry exactly one meaning; never use
one for decoration or for a different meaning.

| Token | Value | Job |
|---|---|---|
| `--bg` | `#100d26` | app ground |
| `--bg-sunk` | `#0b0920` | title bar, rail, inputs, video wells |
| `--panel` / `--panel-2` / `--panel-3` | `#161238` / `#1d1847` / `#262058` | raised surfaces, hover, active |
| `--seam` / `--seam-strong` | `#2a2461` / `#3b3485` | 1px dividers and outlines |
| `--text` / `--text-2` / `--text-3` | `#eeebff` / `#bdb6e6` / `#908ac2` | primary, secondary, tertiary text (all >= 4.5:1 on `--bg`) |
| `--you` | `#3fe0f0` | you: kills, multikills, your APM, selection, primary actions, focus |
| `--death` | `#ff4f8b` | deaths, destructive actions, recording indicator, defeat |
| `--objective` | `#ffc83d` | turret, inhibitor, dragon, herald, baron, favorites star |
| `--win` | `#52f0a8` | victory, healthy status |
| `--assist` | `#b197ff` | assists, ultimate |
| `--warn` | `#ffa53d` | low disk, connecting, missing sources |
| `--mark` | `#e8e4ff` | manual bookmarks |

KDA is always kills in `--you`, deaths in `--death`, assists in `--assist`.

## Type

- UI text: Segoe UI Variable (system), 13.5px base, 1.45 line height.
- Score voice: Chakra Petch 500/600/700, self-hosted via `@fontsource/chakra-petch`. Used for
  headings, KDA, timers, sizes and every number (`.num`, tabular figures). Section labels in it
  are uppercase with 0.06-0.08em tracking.
- No pixel fonts, no glow, no scanlines.

## Shape and depth

- Radii: 3px (small controls), 4px (buttons, inputs), 6px (panels, cards, popovers).
- Elevation is declared once: panels use a 1px seam; only popovers, toasts and modals use the
  `--shadow-pop` shadow.
- Motion: `--ease-out` (cubic-bezier(0.16, 1, 0.3, 1)), 120-220ms; popovers rise 4px, toasts 8px,
  the REC dot blinks in steps.

## Signature move

The event lane on every timeline: each event is a chip at its moment plus a bar whose width is
the real capture window (seconds before and after). Overlapping chips step to a second row;
multikills carry a count badge. The library reads as a score table: one row per run with the
champion portrait, result, KDA in large score numerals, tags, duration and size.

Recently deleted items keep a struck, grayscale VOID state until purged.

## Components

`Icon` (one 24px stroke family, 1.8 stroke, including one glyph per event kind), `Toggle`,
`Segmented`, `Select`, `Slider`, `Popover`, `HotkeyInput`, `RecordingTile`, `Player`, `Timeline`,
`TitleBar`, `Rail`, `Toasts`. Buttons: `.btn` (default), `.btn.primary` (cyan), `.btn.danger`
(outlined magenta), `.btn.ghost`; `.icon-btn` with `.on` for active tools.

## Layout

Title bar 40px (custom, draggable), rail 56px, content fills the rest. The session view puts the
player and timeline left and a 300px moments/clips column right (360px on very wide screens).
Champion splash art can sit behind the content at 11% opacity, fading out downward
(Change app layout > Show champion art).
