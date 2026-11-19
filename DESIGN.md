# Design: Matt Game Recorder

World: a gaming cafe (网吧) after dark. Lights low, graphite glass, one neon colour that means
you, and the session clock over every seat. Every match leaves a receipt: what you played and
what it cost. Mode: Operate (a working tool), so the world lends only type, palette, density and
its signature moves. Layout, navigation and controls are the standard ones for a desktop media app.

## Color

The default theme is `cafe` (Internet Café, 网吧) with `cafe-day` as its light pair; every other
built-in theme stays selectable in Appearance and fills the same 18 tokens (`src/lib/themes.ts`).
Each accent carries exactly one meaning; never use one for decoration or a different meaning.

| Token | cafe | Job |
|---|---|---|
| `--bg` | `#0d1014` | app ground |
| `--bg-sunk` | `#090b0e` | title bar, rail, inputs, video wells |
| `--panel` / `--panel-2` / `--panel-3` | `#141920` / `#1b2129` / `#232b35` | raised surfaces, hover, active |
| `--seam` / `--seam-strong` | `#222a34` / `#334050` | 1px dividers, outlines, the receipt tear line |
| `--text` / `--text-2` / `--text-3` | `#e8ecef` / `#b7c0ca` / `#8c96a3` | primary, secondary, tertiary (text-3 is 6.4:1 on bg) |
| `--you` | `#2ee6a6` | you: kills, multikills, your APM, selection, primary actions, focus, the session clock |
| `--death` | `#ff5c70` | deaths, destructive actions, recording, defeat |
| `--objective` | `#ffb547` | turret, inhibitor, dragon, herald, baron, favourites star |
| `--win` | `#4cc9ff` | victory, healthy status |
| `--assist` | `#b9a3ff` | assists, ultimate |
| `--warn` | `#ff8a3d` | low disk, connecting, missing sources |
| `--mark` | `#e8ecef` | manual bookmarks |

KDA is always kills in `--you`, deaths in `--death`, assists in `--assist`.

## Type

- UI text: Segoe UI Variable (system; Microsoft YaHei for Chinese), 13.5px base, 1.45 line height.
- Score voice: Rajdhani 500/600/700, self-hosted via `@fontsource/rajdhani`, for headings, KDA,
  timers, sizes and every number (`.num`, tabular figures). Chakra Petch stays bundled as an
  Appearance choice.
- No glow, no scanlines, no pixel fonts.

## Shape and depth

- Radii (Appearance "default" corners): 4px small controls, 6px buttons and inputs, 10px panels,
  cards and popovers. Sharp and round corners are user choices.
- Elevation is declared once: panels use a 1px seam; only popovers, toasts and modals use
  `--shadow-pop`.
- Motion: `--ease-out`, 120-320ms. Buttons rise 1px on hover and press to 0.96; grid cards lift
  2px; thumbnails (`.zoom`) scale 1.03 inside their frame; text links draw their underline.
  Reduced motion stills all of it.

## Signature moves

- **Session clock:** the title bar shows today's League play time and match count (`今日 1:24:10
  3 局`), from today's sessions, start to end. Green when you have played today, muted otherwise.
- **Receipt tail:** every library row ends with a dashed tear line and the match's bill:
  length and size, label over value.
- **Event lane:** on the match timeline each event is a chip at its moment plus a bar as wide as
  its real capture window; multikills carry a count.

Recently deleted items keep a struck, grayscale state until purged.

## Components

`Icon` (one 24px stroke family, 1.8 stroke, one glyph per event kind), `Toggle`, `Segmented`,
`Select`, `Slider`, `Popover`, `ConfirmDialog`, `HotkeyInput`, `RecordingTile`, `Player`, `Timeline`,
`TitleBar`, `Rail`, `Toasts`. Buttons: `.btn` (default), `.btn.primary` (accent), `.btn.danger`
(outlined), `.btn.ghost`; `.icon-btn` with `.on` for active tools; `.link` for text links.

## Layout

Title bar 40px (custom, draggable), rail 56px, content fills the rest. The match view puts the
player and timeline left and a 300px moments/clips column right. The video box takes the
recording's own aspect ratio (container query on `.stage`), so ultrawide matches are not
letterboxed and the timeline sits right under the picture. Settings pages share `.settings-page`
styles in `app.css`. Champion splash art can sit behind the content at low opacity
(Change app layout > Show champion art).
