// Applies the Appearance settings: theme colors, fonts, corners and interface size.
// Everything is CSS custom properties on <html>, so every view follows without its own code.

import { emit } from '@tauri-apps/api/event';
import { t } from './i18n.svelte';
import { bestInk, contrast, DEFAULT_THEME, isHex, THEMES, TOKENS, type Colors, type ThemeDef, type TokenKey } from './themes';
import type { Appearance, CustomTheme } from './types';

export const DEFAULT_APPEARANCE: Appearance = {
  theme: DEFAULT_THEME,
  followSystem: false,
  lightTheme: 'rose-pine-dawn',
  darkTheme: 'rose-pine-moon',
  overrides: {},
  fontUi: '',
  fontScore: '',
  uiScale: 100,
  corners: 'default',
  customThemes: [],
  windowBg: '#100d26',
};

export const DEFAULT_FONT_UI = 'Segoe UI Variable Text';
export const DEFAULT_FONT_SCORE = 'Chakra Petch';
/** Fonts that ship inside the app (always available, even if not installed in Windows). */
export const BUNDLED_FONTS = ['Chakra Petch'];

const CORNERS: Record<string, [number, number, number]> = { sharp: [1, 2, 2], default: [3, 4, 6], round: [6, 8, 12] };
const HAS_TAURI = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
const CACHE_KEY = 'mgr.theme';

/** A custom theme as a full theme: missing colors come from a built-in of the same lightness. */
export function customToTheme(c: CustomTheme): ThemeDef {
  const base = THEMES.find((x) => x.id === (c.dark ? 'arcade' : 'github-light'))!.colors;
  const colors = { ...base };
  for (const tk of TOKENS) if (isHex(c.colors[tk.key] ?? '')) colors[tk.key] = c.colors[tk.key].toLowerCase();
  return { id: c.id, name: c.name, dark: c.dark, colors, custom: true };
}

export function allThemes(a: Appearance): ThemeDef[] {
  return [...THEMES, ...a.customThemes.map(customToTheme)];
}

export function themeById(a: Appearance, id: string): ThemeDef {
  return allThemes(a).find((x) => x.id === id) ?? THEMES[0];
}

export const systemDark = () => typeof matchMedia === 'undefined' || matchMedia('(prefers-color-scheme: dark)').matches;

/** The theme in use right now (with "match Windows" on, it depends on Windows' mode). */
export function activeThemeId(a: Appearance): string {
  return a.followSystem ? (systemDark() ? a.darkTheme : a.lightTheme) : a.theme;
}

/** Final colors of a theme: the theme, plus the user's tweaks to it. */
export function themeColors(a: Appearance, id: string): Colors {
  const th = themeById(a, id);
  const colors = { ...th.colors };
  const tweaks = a.overrides[th.id] ?? {};
  for (const [k, v] of Object.entries(tweaks)) if (k in colors && isHex(v)) colors[k as TokenKey] = v.toLowerCase();
  // A changed accent gets readable label text unless that was set too.
  // The theme's own colors come first; pure black or white only when none of them reads well.
  if (tweaks.you && !tweaks.youInk) {
    const own = [th.colors.youInk, colors.bg, colors.bgSunk, colors.text];
    colors.youInk = own.find((c) => contrast(c, colors.you) >= 4.5) ?? bestInk(colors.you, [...own, '#000000', '#ffffff']);
  }
  return colors;
}

function mix(a: string, b: string, t: number): string {
  const p = (h: string) => [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16));
  const [x, y] = [p(a), p(b)];
  return x.map((v, i) => Math.round(v + (y[i] - v) * t)).join(' ');
}

const font = (name: string, fallback: string) => `"${name.replace(/"/g, '')}", ${fallback}`;

/** Every CSS variable the appearance controls. */
export function cssVars(a: Appearance, th: ThemeDef, c: Colors): Record<string, string> {
  const v: Record<string, string> = {};
  for (const tk of TOKENS) v[tk.css] = c[tk.key];
  // Text over video stays light whatever the theme, since video is dark behind it.
  v['--media-bg'] = th.dark ? `rgb(${mix(c.bgSunk, '#000000', 0.65)})` : '#121116';
  v['--media-ink'] = th.dark ? c.text : '#f4f4f7';
  // Light themes have dark accents; over video they need a lighter version to show.
  v['--media-accent'] = th.dark ? c.you : `rgb(${mix(c.you, '#ffffff', 0.45)})`;
  v['--media-ink-2'] = th.dark ? c.text3 : '#bdbdc8';
  v['--danger-ink'] = bestInk(c.death, ['#ffffff', c.bgSunk, '#000000']);
  const shade = mix(th.dark ? c.bgSunk : c.text, '#000000', th.dark ? 0.6 : 0.3);
  v['--shadow-pop'] = th.dark
    ? `0 10px 28px -6px rgb(${shade} / 0.7), 0 2px 6px rgb(${shade} / 0.5)`
    : `0 10px 28px -6px rgb(${shade} / 0.22), 0 2px 6px rgb(${shade} / 0.14)`;
  const [sm, md, lg] = CORNERS[a.corners] ?? CORNERS.default;
  v['--r-sm'] = `${sm}px`;
  v['--r-md'] = `${md}px`;
  v['--r-lg'] = `${lg}px`;
  v['--font-ui'] = font(a.fontUi || DEFAULT_FONT_UI, "'Segoe UI Variable Text', 'Segoe UI', system-ui, sans-serif");
  v['--font-score'] = font(a.fontScore || DEFAULT_FONT_SCORE, "'Segoe UI', sans-serif");
  return v;
}

function setVars(vars: Record<string, string>, dark: boolean) {
  const root = document.documentElement;
  for (const [k, val] of Object.entries(vars)) root.style.setProperty(k, val);
  root.style.colorScheme = dark ? 'dark' : 'light';
  root.dataset.scheme = dark ? 'dark' : 'light';
}

/** Apply the cached look before the app has loaded its settings, so a light theme never flashes dark. */
export function applyCachedAppearance() {
  try {
    const cached = JSON.parse(localStorage.getItem(CACHE_KEY) ?? 'null') as { vars: Record<string, string>; dark: boolean } | null;
    if (cached?.vars) setVars(cached.vars, cached.dark);
  } catch {
    /* no cache yet */
  }
}

let lastZoom = 0;
let lastBg = '';

/** Apply `a` to this window. */
export function applyAppearance(a: Appearance) {
  const th = themeById(a, activeThemeId(a));
  const colors = themeColors(a, th.id);
  const vars = cssVars(a, th, colors);
  setVars(vars, th.dark);
  try {
    localStorage.setItem(CACHE_KEY, JSON.stringify({ vars, dark: th.dark }));
  } catch {
    /* storage unavailable */
  }
  if (!HAS_TAURI) return;
  const zoom = Math.min(150, Math.max(80, a.uiScale || 100)) / 100;
  if (zoom !== lastZoom) {
    lastZoom = zoom;
    import('@tauri-apps/api/webview').then((m) => m.getCurrentWebview().setZoom(zoom)).catch(() => {});
  }
  if (colors.bg !== lastBg) {
    lastBg = colors.bg;
    import('@tauri-apps/api/window').then((m) => m.getCurrentWindow().setBackgroundColor(colors.bg)).catch(() => {});
  }
}

/** Tell the other windows (the live dashboard) to follow. */
export function broadcastAppearance(a: Appearance) {
  if (HAS_TAURI) emit('appearance', a).catch(() => {});
}

// ---------- theme files ----------

export const THEME_FILE_TYPE = 'matt-game-recorder-theme';

export function themeFile(name: string, dark: boolean, colors: Colors): string {
  return JSON.stringify({ type: THEME_FILE_TYPE, version: 1, name, dark, colors }, null, 2) + '\n';
}

/** Read a theme file. Accepts partial color sets; anything missing comes from a built-in theme. */
export function parseThemeFile(text: string): { name: string; dark: boolean; colors: Partial<Colors> } {
  let raw: unknown;
  try {
    raw = JSON.parse(text);
  } catch {
    throw new Error(t.appearance.notJson);
  }
  const o = raw as { name?: unknown; dark?: unknown; colors?: Record<string, unknown> };
  if (!o || typeof o !== 'object' || !o.colors || typeof o.colors !== 'object') throw new Error(t.appearance.noColors);
  const colors: Partial<Colors> = {};
  for (const tk of TOKENS) {
    const val = o.colors[tk.key];
    if (typeof val === 'string' && isHex(val)) colors[tk.key] = val.toLowerCase();
  }
  if (!Object.keys(colors).length) throw new Error(t.appearance.unreadable);
  const bg = colors.bg;
  const dark = typeof o.dark === 'boolean' ? o.dark : bg ? parseInt(bg.slice(1, 3), 16) + parseInt(bg.slice(3, 5), 16) + parseInt(bg.slice(5, 7), 16) < 384 : true;
  const name = typeof o.name === 'string' && o.name.trim() ? o.name.trim().slice(0, 40) : t.appearance.importedName;
  return { name, dark, colors };
}
