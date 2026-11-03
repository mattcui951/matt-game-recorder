// Themes: every color in the UI comes from these 18 tokens (see app.css). A theme fills all of
// them; the user can tweak any token per theme, or save their own themes.
// Palettes follow each theme's official source (rosepinetheme.com, catppuccin/palette, draculatheme.com,
// nordtheme.com, gruvbox, tokyonight.nvim, ethanschoonover.com/solarized, atom one-dark, GitHub Primer,
// everforest, kanagawa.nvim, synthwave-84, night-owl, ayu). Where an official "muted" text color is
// too faint to read at small sizes, the closest readable step is used instead.

export type TokenKey =
  | 'bg'
  | 'bgSunk'
  | 'panel'
  | 'panel2'
  | 'panel3'
  | 'seam'
  | 'seamStrong'
  | 'text'
  | 'text2'
  | 'text3'
  | 'you'
  | 'youInk'
  | 'death'
  | 'objective'
  | 'win'
  | 'assist'
  | 'warn'
  | 'mark';

export type Colors = Record<TokenKey, string>;

export type ThemeDef = { id: string; name: string; dark: boolean; contrast?: boolean; colors: Colors; custom?: boolean };

export const TOKENS: { key: TokenKey; css: string; label: string; hint: string; group: 'Surfaces' | 'Text' | 'Accents' }[] = [
  { key: 'bg', css: '--bg', label: 'Background', hint: 'Behind everything', group: 'Surfaces' },
  { key: 'bgSunk', css: '--bg-sunk', label: 'Sunken', hint: 'Title bar, side rail, wells', group: 'Surfaces' },
  { key: 'panel', css: '--panel', label: 'Panels', hint: 'Cards, rows, menus', group: 'Surfaces' },
  { key: 'panel2', css: '--panel-2', label: 'Raised', hint: 'Buttons, fields, hover', group: 'Surfaces' },
  { key: 'panel3', css: '--panel-3', label: 'Selected', hint: 'Pressed and chosen items', group: 'Surfaces' },
  { key: 'seam', css: '--seam', label: 'Borders', hint: 'Lines between things', group: 'Surfaces' },
  { key: 'seamStrong', css: '--seam-strong', label: 'Strong borders', hint: 'Outlines that must show', group: 'Surfaces' },
  { key: 'text', css: '--text', label: 'Text', hint: 'Titles and main text', group: 'Text' },
  { key: 'text2', css: '--text-2', label: 'Secondary text', hint: 'Labels and details', group: 'Text' },
  { key: 'text3', css: '--text-3', label: 'Muted text', hint: 'Hints and small print', group: 'Text' },
  { key: 'you', css: '--you', label: 'Accent', hint: 'Primary buttons, your kills, selection', group: 'Accents' },
  { key: 'youInk', css: '--you-ink', label: 'Text on accent', hint: 'Labels on primary buttons', group: 'Accents' },
  { key: 'death', css: '--death', label: 'Deaths and danger', hint: 'Deaths, delete, recording', group: 'Accents' },
  { key: 'objective', css: '--objective', label: 'Objectives', hint: 'Turrets, dragons, barons', group: 'Accents' },
  { key: 'win', css: '--win', label: 'Victory', hint: 'Wins and healthy states', group: 'Accents' },
  { key: 'assist', css: '--assist', label: 'Assists', hint: 'Assists and ultimates', group: 'Accents' },
  { key: 'warn', css: '--warn', label: 'Warnings', hint: 'Low disk, partial data', group: 'Accents' },
  { key: 'mark', css: '--mark', label: 'Bookmarks', hint: 'Manual marks', group: 'Accents' },
];

const t = (id: string, name: string, dark: boolean, c: string, extra: Partial<ThemeDef> = {}): ThemeDef => {
  // Compact palette string, in TOKENS order.
  const v = c.trim().split(/\s+/);
  const colors = Object.fromEntries(TOKENS.map((tk, i) => [tk.key, v[i]])) as Colors;
  return { id, name, dark, colors, ...extra };
};

// Order: bg bgSunk panel panel2 panel3 seam seamStrong | text text2 text3 | you youInk death objective win assist warn mark
export const THEMES: ThemeDef[] = [
  t('arcade', 'Arcade', true, '#100d26 #0b0920 #161238 #1d1847 #262058 #2a2461 #3b3485  #eeebff #bdb6e6 #908ac2  #3fe0f0 #062a30 #ff4f8b #ffc83d #52f0a8 #b197ff #ffa53d #e8e4ff'),
  t('rose-pine', 'Rosé Pine', true, '#191724 #16141f #1f1d2e #26233a #312e48 #403d52 #524f67  #e0def4 #b9b6d3 #908caa  #c4a7e7 #191724 #eb6f92 #f6c177 #9ccfd8 #ebbcba #f2a46d #e0def4'),
  t('rose-pine-moon', 'Rosé Pine Moon', true, '#232136 #1f1d30 #2a273f #393552 #44415a #44415a #56526e  #e0def4 #bebbd8 #9692b0  #c4a7e7 #232136 #eb6f92 #f6c177 #9ccfd8 #ea9a97 #f2a46d #e0def4'),
  t('catppuccin-mocha', 'Catppuccin Mocha', true, '#181825 #11111b #1e1e2e #313244 #45475a #313244 #585b70  #cdd6f4 #bac2de #a6adc8  #cba6f7 #11111b #f38ba8 #f9e2af #a6e3a1 #89b4fa #fab387 #cdd6f4'),
  t('catppuccin-macchiato', 'Catppuccin Macchiato', true, '#1e2030 #181926 #24273a #363a4f #494d64 #363a4f #5b6078  #cad3f5 #b8c0e0 #a5adcb  #c6a0f6 #181926 #ed8796 #eed49f #a6da95 #8aadf4 #f5a97f #cad3f5'),
  t('catppuccin-frappe', 'Catppuccin Frappé', true, '#292c3c #232634 #303446 #414559 #51576d #414559 #626880  #c6d0f5 #b5bfe2 #a5adce  #ca9ee6 #232634 #e78284 #e5c890 #a6d189 #8caaee #ef9f76 #c6d0f5'),
  t('dracula', 'Dracula', true, '#21222c #191a21 #282a36 #343746 #44475a #44475a #6272a4  #f8f8f2 #d4d6e0 #a3a9c9  #bd93f9 #191a21 #ff5555 #f1fa8c #50fa7b #ff79c6 #ffb86c #f8f8f2'),
  t('nord', 'Nord', true, '#2e3440 #272c36 #3b4252 #434c5e #4c566a #434c5e #616e88  #eceff4 #d8dee9 #aeb7c8  #88c0d0 #2e3440 #e07c86 #ebcb8b #a3be8c #b48ead #d08770 #e5e9f0'),
  t('gruvbox-dark', 'Gruvbox Dark', true, '#282828 #1d2021 #32302f #3c3836 #504945 #3c3836 #665c54  #ebdbb2 #d5c4a1 #a89984  #8ec07c #1d2021 #fb4934 #fabd2f #b8bb26 #d3869b #fe8019 #ebdbb2'),
  t('tokyo-night', 'Tokyo Night', true, '#16161e #121218 #1a1b26 #24283b #292e42 #292e42 #414868  #c0caf5 #a9b1d6 #7f88b2  #7aa2f7 #16161e #f7768e #e0af68 #9ece6a #bb9af7 #ff9e64 #c0caf5'),
  t('one-dark', 'One Dark', true, '#21252b #1b1f23 #282c34 #2c313a #3a3f4b #333842 #4b5263  #d7dae0 #abb2bf #9097a5  #61afef #1b1f23 #e06c75 #e5c07b #98c379 #c678dd #d19a66 #d7dae0'),
  t('monokai', 'Monokai', true, '#1e1f1c #191a17 #272822 #33342c #3e3d32 #3e3d32 #5c5b4e  #f8f8f2 #cfcfc2 #a59f85  #66d9ef #1e1f1c #f92672 #e6db74 #a6e22e #ae81ff #fd971f #f8f8f2'),
  t('github-dark', 'GitHub Dark', true, '#010409 #010409 #0d1117 #161b22 #21262d #30363d #484f58  #e6edf3 #c9d1d9 #9198a1  #4493f8 #010409 #f85149 #d29922 #3fb950 #ab7df8 #db6d28 #e6edf3'),
  t('solarized-dark', 'Solarized Dark', true, '#002b36 #00212b #073642 #0b3f4c #14505e #0e4452 #586e75  #eee8d5 #b3bdbd #93a1a1  #3d9ae0 #002b36 #ef4a46 #b58900 #859900 #8589dc #e2622a #eee8d5'),
  t('everforest', 'Everforest', true, '#232a2e #1e2326 #2d353b #343f44 #3d484d #3d484d #56635f  #d3c6aa #c5b9a0 #9da9a0  #a7c080 #232a2e #e67e80 #dbbc7f #83c092 #d699b6 #e69875 #d3c6aa'),
  t('kanagawa', 'Kanagawa', true, '#1f1f28 #16161d #2a2a37 #363646 #43435a #363646 #54546d  #dcd7ba #c8c093 #9a9a8f  #7e9cd8 #16161d #e46876 #e6c384 #98bb6c #957fb8 #ffa066 #dcd7ba'),
  t('synthwave-84', "Synthwave '84", true, '#241b2f #1d1527 #262335 #34294f #463465 #34294f #495495  #ffffff #e0d6f2 #b6b1c8  #36f9f6 #241b2f #fe4450 #fede5d #72f1b8 #ff7edb #ff8b39 #ffffff'),
  t('night-owl', 'Night Owl', true, '#011627 #010f1c #0b2942 #0f3352 #1d3b53 #122d42 #2a4d6e  #d6deeb #b2c2d6 #8fa3b8  #82aaff #011627 #ef5350 #ecc48d #addb67 #c792ea #f78c6c #d6deeb'),
  t('ayu-mirage', 'Ayu Mirage', true, '#1f2430 #191e2a #242936 #2d3340 #353c4a #343b4a #4a5367  #cccac2 #b8b4a8 #8f97a0  #73d0ff #1f2430 #f28779 #ffcc66 #d5ff80 #dfbfff #ffad66 #cccac2'),

  t('rose-pine-dawn', 'Rosé Pine Dawn', false, '#faf4ed #f2e9e1 #fffaf3 #f4ede8 #dfdad9 #dfdad9 #bdb8bc  #464261 #575279 #6e6a86  #286983 #faf4ed #b4637a #b8761f #3e8399 #907aa9 #c0603b #464261'),
  t('catppuccin-latte', 'Catppuccin Latte', false, '#eff1f5 #dce0e8 #f6f7fa #e6e9ef #ccd0da #ccd0da #acb0be  #3d4057 #4c4f69 #5c5f77  #8839ef #eff1f5 #d20f39 #b06b0b #3a9126 #1e66f5 #d9520a #4c4f69'),
  t('gruvbox-light', 'Gruvbox Light', false, '#f2e5bc #ebdbb2 #fbf1c7 #ebdbb2 #d5c4a1 #d5c4a1 #bdae93  #3c3836 #504945 #665c54  #3b7050 #fbf1c7 #9d0006 #b57614 #79740e #8f3f71 #af3a03 #3c3836'),
  t('solarized-light', 'Solarized Light', false, '#eee8d5 #e4ddc8 #fdf6e3 #eee8d5 #e4ddc8 #ddd6c1 #93a1a1  #073642 #475b62 #52666d  #1f74b3 #fdf6e3 #dc322f #a07800 #738500 #6c71c4 #cb4b16 #073642'),
  t('github-light', 'GitHub Light', false, '#f6f8fa #eff2f5 #ffffff #f6f8fa #eaeef2 #d1d9e0 #afb8c1  #1f2328 #424a53 #59636e  #0969da #ffffff #cf222e #9a6700 #1a7f37 #8250df #bc4c00 #1f2328'),

  t('high-contrast-dark', 'High Contrast Dark', true, '#000000 #000000 #0a0a0a #161616 #2b2b2b #8a8a8a #ffffff  #ffffff #ffffff #e0e0e0  #1aebff #000000 #ff6b6b #ffeb3b #3ff23f #d6b4fd #ffa940 #ffffff', { contrast: true }),
  t('high-contrast-light', 'High Contrast Light', false, '#ffffff #ffffff #ffffff #f0f0f0 #dcdcdc #6e6e6e #000000  #000000 #000000 #262626  #0040c0 #ffffff #b00020 #7a4f00 #006b21 #6b1fb3 #9a4a00 #000000', { contrast: true }),
];

export const DEFAULT_THEME = 'arcade';

// ---------- color math ----------

export const isHex = (v: string) => /^#[0-9a-f]{6}$/i.test(v);

function rgb(hex: string): [number, number, number] {
  const n = parseInt(hex.slice(1), 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

export function luminance(hex: string): number {
  const [r, g, b] = rgb(hex).map((c) => {
    const s = c / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

/** WCAG contrast ratio, 1 to 21. */
export function contrast(a: string, b: string): number {
  const [x, y] = [luminance(a), luminance(b)].sort((p, q) => q - p);
  return (x + 0.05) / (y + 0.05);
}

/** Whichever candidate reads best on `on`. */
export function bestInk(on: string, candidates: string[]): string {
  return candidates.reduce((best, c) => (contrast(c, on) > contrast(best, on) ? c : best));
}
