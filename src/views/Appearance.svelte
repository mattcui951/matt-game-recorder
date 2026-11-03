<script lang="ts">
  import { open as openDialog, save as saveDialog } from '@tauri-apps/plugin-dialog';
  import FontPicker from '../components/FontPicker.svelte';
  import Icon from '../components/Icon.svelte';
  import Segmented from '../components/Segmented.svelte';
  import Select from '../components/Select.svelte';
  import Slider from '../components/Slider.svelte';
  import Toggle from '../components/Toggle.svelte';
  import { api } from '../lib/api';
  import { app, errorText, setAppearance, toast } from '../lib/state.svelte';
  import {
    activeThemeId,
    allThemes,
    customToTheme,
    DEFAULT_APPEARANCE,
    DEFAULT_FONT_SCORE,
    DEFAULT_FONT_UI,
    parseThemeFile,
    systemDark,
    themeById,
    themeColors,
    themeFile,
  } from '../lib/appearance';
  import { contrast, isHex, THEMES, TOKENS, type ThemeDef, type TokenKey } from '../lib/themes';
  import { t } from '../lib/i18n.svelte';

  /** Names of themes this app named are translated; the rest are their makers' names. */
  const themeName = (th: ThemeDef) => (th.custom ? th.name : (t.appearance.themeNames[th.id] ?? th.name));
  const tokenLabel = (k: TokenKey) => t.appearance.tokens[k][0];
  const tokenHint = (k: TokenKey) => t.appearance.tokens[k][1];

  const a = $derived(app.settings!.appearance);
  const active = $derived(activeThemeId(a));
  const theme = $derived(themeById(a, active));
  const colors = $derived(themeColors(a, active));
  const tweaks = $derived(theme.custom ? {} : (a.overrides[active] ?? {}));
  const tweakCount = $derived(Object.keys(tweaks).length);
  const builtIn = $derived(THEMES.find((x) => x.id === active));

  const groups = $derived([
    { id: 'dark', title: t.appearance.groupDark, list: THEMES.filter((x) => x.dark && !x.contrast) },
    { id: 'light', title: t.appearance.groupLight, list: THEMES.filter((x) => !x.dark && !x.contrast) },
    { id: 'contrast', title: t.appearance.groupContrast, list: THEMES.filter((x) => x.contrast) },
    { id: 'yours', title: t.appearance.groupYours, list: a.customThemes.map(customToTheme) },
  ]);

  function choose(th: ThemeDef) {
    setAppearance((x) => {
      if (!x.followSystem) x.theme = th.id;
      else if (th.dark) x.darkTheme = th.id;
      else x.lightTheme = th.id;
    });
  }
  const isChosen = (th: ThemeDef) => (a.followSystem ? th.id === a.darkTheme || th.id === a.lightTheme : th.id === a.theme);

  // ---------- colors ----------
  function setColor(key: TokenKey, raw: string) {
    const hex = (raw.startsWith('#') ? raw : `#${raw}`).trim().toLowerCase();
    if (!isHex(hex)) return;
    setAppearance((x) => {
      const custom = x.customThemes.find((c) => c.id === active);
      if (custom) {
        custom.colors[key] = hex;
        return;
      }
      const o = (x.overrides[active] ??= {});
      if (builtIn && builtIn.colors[key] === hex) delete o[key];
      else o[key] = hex;
      if (!Object.keys(o).length) delete x.overrides[active];
    });
  }
  function resetColor(key: TokenKey) {
    setAppearance((x) => {
      const o = x.overrides[active];
      if (!o) return;
      delete o[key];
      if (!Object.keys(o).length) delete x.overrides[active];
    });
  }
  function resetAll() {
    setAppearance((x) => {
      delete x.overrides[active];
    });
  }

  // Readability check for the tokens text sits on.
  const warnings = $derived.by(() => {
    const c = colors;
    const w: Partial<Record<TokenKey, string>> = {};
    const check = (k: TokenKey, on: TokenKey, min: number) => {
      const r = contrast(c[k], c[on]);
      if (r < min && !w[k]) w[k] = t.appearance.hardToRead(tokenLabel(on).toLowerCase(), r.toFixed(1), min);
    };
    for (const on of ['bg', 'panel'] as TokenKey[]) {
      check('text', on, 4.5);
      check('text2', on, 4.5);
      check('text3', on, 4.5);
    }
    check('youInk', 'you', 4.5);
    for (const k of ['you', 'death', 'objective', 'win', 'assist', 'warn'] as TokenKey[]) check(k, 'panel', 3);
    return w;
  });

  // ---------- your themes ----------
  let naming = $state<'new' | 'rename' | null>(null);
  let nameDraft = $state('');
  let confirmDelete = $state(false);

  function startNaming(kind: 'new' | 'rename') {
    naming = kind;
    nameDraft = kind === 'rename' ? theme.name : tweakCount || theme.custom ? t.appearance.mine(themeName(theme)) : t.appearance.my(themeName(theme));
  }
  function commitName() {
    const name = nameDraft.trim().slice(0, 40);
    if (!name) return;
    if (naming === 'rename') {
      setAppearance((x) => {
        const c = x.customThemes.find((ct) => ct.id === active);
        if (c) c.name = name;
      });
    } else {
      const id = `custom-${Date.now().toString(36)}`;
      const from = theme;
      setAppearance((x) => {
        x.customThemes.push({ id, name, dark: from.dark, colors: { ...colors } });
        // The tweaks now live in the new theme; the built-in goes back to its own colors.
        if (!from.custom) delete x.overrides[from.id];
        if (!x.followSystem) x.theme = id;
        else if (from.dark) x.darkTheme = id;
        else x.lightTheme = id;
      });
      toast(t.appearance.savedTheme(name), 'ok');
    }
    naming = null;
  }
  function deleteTheme() {
    const name = theme.name;
    setAppearance((x) => {
      x.customThemes = x.customThemes.filter((ct) => ct.id !== active);
      if (x.theme === active) x.theme = DEFAULT_APPEARANCE.theme;
      if (x.darkTheme === active) x.darkTheme = DEFAULT_APPEARANCE.darkTheme;
      if (x.lightTheme === active) x.lightTheme = DEFAULT_APPEARANCE.lightTheme;
    });
    confirmDelete = false;
    toast(t.appearance.deletedTheme(name), 'info');
  }

  async function exportTheme() {
    try {
      const name = theme.custom || !tweakCount ? themeName(theme) : t.appearance.mine(themeName(theme));
      const path = await saveDialog({ defaultPath: `${name.replace(/[\\/:*?"<>|]/g, '')}.json`, filters: [{ name: t.appearance.fileFilter, extensions: ['json'] }] });
      if (!path) return;
      await api.writeThemeFile(path, themeFile(name, theme.dark, colors));
      toast(t.appearance.exported(name), 'ok');
    } catch (e) {
      toast(errorText(e), 'error');
    }
  }
  async function importTheme() {
    try {
      const path = await openDialog({ multiple: false, filters: [{ name: t.appearance.fileFilter, extensions: ['json'] }] });
      if (typeof path !== 'string') return;
      const file = parseThemeFile(await api.readThemeFile(path));
      const id = `custom-${Date.now().toString(36)}`;
      setAppearance((x) => {
        x.customThemes.push({ id, name: file.name, dark: file.dark, colors: { ...file.colors } });
        if (!x.followSystem) x.theme = id;
        else if (file.dark) x.darkTheme = id;
        else x.lightTheme = id;
      });
      toast(t.appearance.imported(file.name), 'ok');
    } catch (e) {
      toast(e instanceof Error ? e.message : errorText(e), 'error');
    }
  }

  function restoreDefaults() {
    setAppearance((x) => {
      const keep = x.customThemes;
      Object.assign(x, structuredClone(DEFAULT_APPEARANCE), { customThemes: keep });
    });
    toast(t.appearance.restored, 'info');
  }

  const lightOptions = $derived(allThemes(a).filter((x) => !x.dark).map((x) => ({ value: x.id, label: themeName(x) })));
  const darkOptions = $derived(allThemes(a).filter((x) => x.dark).map((x) => ({ value: x.id, label: themeName(x) })));
  const SAMPLE = [
    { k: 'you', icon: 'swords' },
    { k: 'death', icon: 'skull' },
    { k: 'assist', icon: 'assist' },
    { k: 'objective', icon: 'baron' },
    { k: 'win', icon: 'trophy' },
    { k: 'warn', icon: 'disk' },
    { k: 'mark', icon: 'flag' },
  ];
</script>

<div class="ap">
  <h1>{t.appearance.title}</h1>
  <p class="lead">{t.appearance.lead}</p>

  <section class="sec">
    <h2>{t.appearance.theme}</h2>
    <div class="row">
      <span>{t.appearance.follow}<small>{t.appearance.followHint(systemDark())}</small></span>
      <Toggle label={t.appearance.follow} checked={a.followSystem} onchange={(v) => setAppearance((x) => (x.followSystem = v))} />
    </div>
    {#if a.followSystem}
      <div class="row sub"><span>{t.appearance.lightTheme}</span><div class="ctl"><Select label={t.appearance.lightTheme} value={a.lightTheme} options={lightOptions} onchange={(v) => setAppearance((x) => (x.lightTheme = v))} /></div></div>
      <div class="row sub"><span>{t.appearance.darkTheme}</span><div class="ctl"><Select label={t.appearance.darkTheme} value={a.darkTheme} options={darkOptions} onchange={(v) => setAppearance((x) => (x.darkTheme = v))} /></div></div>
    {/if}

    {#each groups as g (g.id)}
      {#if g.list.length}
        <h3>{g.title}</h3>
        <div class="themes">
          {#each g.list as th (th.id)}
            {@const c = themeColors(a, th.id)}
            <button
              class="tc"
              class:on={th.id === active}
              class:chosen={isChosen(th)}
              aria-pressed={th.id === active}
              onclick={() => choose(th)}
              style="--p-bg:{c.bg};--p-sunk:{c.bgSunk};--p-panel:{c.panel};--p-panel2:{c.panel2};--p-seam:{c.seam};--p-text:{c.text};--p-text3:{c.text3};--p-you:{c.you};--p-death:{c.death};--p-obj:{c.objective};--p-win:{c.win};--p-assist:{c.assist}"
            >
              <span class="tc-prev" aria-hidden="true">
                <span class="tc-top"><i></i><i></i><i></i></span>
                <span class="tc-card">
                  <span class="tc-l1"></span>
                  <span class="tc-l2"></span>
                  <span class="tc-dots"><i style="background:var(--p-you)"></i><i style="background:var(--p-death)"></i><i style="background:var(--p-obj)"></i><i style="background:var(--p-win)"></i><i style="background:var(--p-assist)"></i></span>
                </span>
                <span class="tc-btn"></span>
              </span>
              <span class="tc-name">
                <span>{themeName(th)}</span>
                {#if a.followSystem && th.id === a.darkTheme}<span class="badge">{t.appearance.badgeDark}</span>{:else if a.followSystem && th.id === a.lightTheme}<span class="badge">{t.appearance.badgeLight}</span>{:else if th.id === active}<Icon name="check" size={14} />{/if}
              </span>
              {#if a.overrides[th.id]}<span class="tweaked">{t.appearance.tweaked}</span>{/if}
            </button>
          {/each}
        </div>
      {/if}
    {/each}
    <div class="actions">
      <button class="btn sm" onclick={importTheme}><Icon name="open" size={14} />{t.appearance.importTheme}</button>
    </div>
  </section>

  <section class="sec">
    <div class="sec-head">
      <h2>{t.appearance.colorsOf(themeName(theme))}</h2>
      {#if tweakCount}<button class="btn ghost sm" onclick={resetAll}><Icon name="undo" size={14} />{t.appearance.resetChanges(tweakCount)}</button>{/if}
    </div>
    <p class="sub-lead">
      {theme.custom ? t.appearance.ownTheme : t.appearance.keptFor(themeName(theme))}
    </p>

    <div class="preview" aria-label={t.appearance.preview}>
      <div class="pv-row">
        {#each SAMPLE as s (s.k)}
          <span class="chip" style="--c:var(--{s.k})"><Icon name={s.icon} size={13} />{t.appearance.sample[s.k]}</span>
        {/each}
      </div>
      <div class="pv-row">
        <button class="btn primary sm" tabindex="-1">{t.appearance.primary}</button>
        <button class="btn sm" tabindex="-1">{t.appearance.button}</button>
        <button class="btn danger sm" tabindex="-1">{t.common.delete}</button>
        <span class="pv-text"><span>{t.appearance.text}</span><span class="t2">{t.appearance.secondary}</span><span class="t3">{t.appearance.mutedHint}</span></span>
      </div>
    </div>

    {#each ['Surfaces', 'Text', 'Accents'] as grp (grp)}
      <h3>{t.appearance.groups[grp]}</h3>
      <div class="tokens">
        {#each TOKENS.filter((tk) => tk.group === grp) as tk (tk.key)}
          {@const val = colors[tk.key]}
          <div class="tok" class:changed={tk.key in tweaks}>
            <label class="swatch" style="background:{val}" title={t.appearance.pickColor(tokenLabel(tk.key))}>
              <input type="color" value={val} aria-label={t.appearance.colorAria(tokenLabel(tk.key))} oninput={(e) => setColor(tk.key, (e.currentTarget as HTMLInputElement).value)} />
            </label>
            <span class="tok-name">
              <span>{tokenLabel(tk.key)}</span>
              <small class:warn={warnings[tk.key]}>{warnings[tk.key] ?? tokenHint(tk.key)}</small>
            </span>
            <input
              class="field hex num"
              value={val}
              spellcheck="false"
              maxlength="7"
              aria-label={t.appearance.hexAria(tokenLabel(tk.key))}
              onchange={(e) => {
                const el = e.currentTarget as HTMLInputElement;
                setColor(tk.key, el.value);
                el.value = colors[tk.key];
              }}
            />
            {#if tk.key in tweaks}
              <button class="icon-btn" title={t.appearance.backTo(themeName(theme))} aria-label={t.appearance.resetToken(tokenLabel(tk.key))} onclick={() => resetColor(tk.key)}><Icon name="undo" size={15} /></button>
            {:else}
              <span class="icon-gap"></span>
            {/if}
          </div>
        {/each}
      </div>
    {/each}

    <div class="actions">
      {#if naming}
        <input
          class="field name"
          bind:value={nameDraft}
          maxlength="40"
          aria-label={t.appearance.themeName}
          onkeydown={(e) => {
            if (e.key === 'Enter') commitName();
            if (e.key === 'Escape') naming = null;
          }}
        />
        <button class="btn primary sm" disabled={!nameDraft.trim()} onclick={commitName}>{naming === 'new' ? t.appearance.saveTheme : t.common.rename}</button>
        <button class="btn ghost sm" onclick={() => (naming = null)}>{t.common.cancel}</button>
      {:else if confirmDelete}
        <span class="dim">{t.appearance.deleteAsk(themeName(theme))}</span>
        <button class="btn danger sm" onclick={deleteTheme}>{t.appearance.deleteTheme}</button>
        <button class="btn ghost sm" onclick={() => (confirmDelete = false)}>{t.appearance.keepIt}</button>
      {:else}
        <button class="btn sm" onclick={() => startNaming('new')}><Icon name="plus" size={14} />{t.appearance.saveAsNew}</button>
        <button class="btn sm" onclick={exportTheme}><Icon name="export" size={14} />{t.appearance.export}</button>
        {#if theme.custom}
          <button class="btn ghost sm" onclick={() => startNaming('rename')}><Icon name="edit" size={14} />{t.common.rename}</button>
          <button class="btn danger sm" onclick={() => (confirmDelete = true)}><Icon name="trash" size={14} />{t.common.delete}</button>
        {/if}
      {/if}
    </div>
  </section>

  <section class="sec">
    <h2>{t.appearance.textLayout}</h2>
    <div class="row">
      <span>{t.appearance.fontUi}<small>{t.appearance.fontUiHint}</small></span>
      <FontPicker label={t.appearance.fontUi} value={a.fontUi} fallback={DEFAULT_FONT_UI} onchange={(v) => setAppearance((x) => (x.fontUi = v))} />
    </div>
    <div class="row">
      <span>{t.appearance.fontScore}<small>{t.appearance.fontScoreHint}</small></span>
      <FontPicker label={t.appearance.fontScore} value={a.fontScore} fallback={DEFAULT_FONT_SCORE} onchange={(v) => setAppearance((x) => (x.fontScore = v))} />
    </div>
    <div class="row">
      <span>{t.appearance.size}<small>{t.appearance.sizeHint}</small></span>
      <div class="ctl"><Slider label={t.appearance.size} value={a.uiScale} min={80} max={150} step={5} suffix="%" onchange={(v) => setAppearance((x) => (x.uiScale = v))} /></div>
    </div>
    <div class="row">
      <span>{t.appearance.corners}</span>
      <Segmented
        label={t.appearance.corners}
        value={a.corners}
        onchange={(v) => setAppearance((x) => (x.corners = v))}
        options={[
          { value: 'sharp', label: t.appearance.sharp },
          { value: 'default', label: t.appearance.default },
          { value: 'round', label: t.appearance.rounded },
        ]}
      />
    </div>
  </section>

  <section class="sec">
    <div class="row">
      <span>{t.appearance.restore}<small>{t.appearance.restoreHint}</small></span>
      <button class="btn sm" onclick={restoreDefaults}><Icon name="restore" size={14} />{t.appearance.restoreButton}</button>
    </div>
  </section>
</div>

<style>
  .ap {
    max-width: 760px;
  }
  h1 {
    font-size: 24px;
  }
  .lead {
    color: var(--text-3);
    margin: 4px 0 20px;
  }
  h2 {
    font-size: 17px;
  }
  h3 {
    margin: 18px 0 8px;
    font-size: 12.5px;
    font-weight: 600;
    color: var(--text-2);
    letter-spacing: 0.02em;
  }
  .sub-lead {
    color: var(--text-3);
    font-size: 12.5px;
    margin: 2px 0 12px;
  }
  .sec {
    padding: 22px 0 18px;
    border-top: 1px solid var(--seam);
  }
  .sec:first-of-type {
    border-top: 0;
    padding-top: 4px;
  }
  .sec-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
  }
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 24px;
    min-height: 44px;
    padding: 4px 0;
  }
  .row > span:first-child {
    display: grid;
    gap: 2px;
  }
  .row small {
    color: var(--text-3);
    font-size: 12px;
  }
  .row.sub {
    padding-left: 16px;
  }
  .ctl {
    width: 260px;
    flex: none;
  }
  .dim {
    color: var(--text-3);
    font-size: 12.5px;
  }

  /* Theme cards: a tiny mock of the app in that theme. */
  .themes {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(172px, 1fr));
    gap: 10px;
  }
  .tc {
    position: relative;
    display: grid;
    gap: 8px;
    padding: 6px 6px 8px;
    border-radius: var(--r-lg);
    background: var(--panel);
    box-shadow: inset 0 0 0 1px var(--seam);
    text-align: left;
    color: var(--text);
    transition: box-shadow 140ms var(--ease-out);
  }
  .tc:hover {
    box-shadow: inset 0 0 0 1px var(--seam-strong);
  }
  .tc.chosen {
    box-shadow: inset 0 0 0 2px var(--you);
  }
  .tc-prev {
    display: grid;
    grid-template-rows: 12px 1fr;
    height: 74px;
    border-radius: calc(var(--r-lg) - 2px);
    background: var(--p-bg);
    box-shadow: inset 0 0 0 1px var(--p-seam);
    overflow: hidden;
    position: relative;
  }
  .tc-top {
    display: flex;
    gap: 3px;
    align-items: center;
    padding: 0 6px;
    background: var(--p-sunk);
  }
  .tc-top i {
    width: 14px;
    height: 3px;
    border-radius: 2px;
    background: var(--p-text3);
    opacity: 0.6;
  }
  .tc-card {
    margin: 7px 8px 0;
    padding: 6px 7px;
    display: grid;
    gap: 4px;
    align-content: start;
    background: var(--p-panel);
    border-radius: 3px;
    box-shadow: inset 0 0 0 1px var(--p-seam);
    width: 70%;
  }
  .tc-l1 {
    height: 5px;
    width: 70%;
    border-radius: 2px;
    background: var(--p-text);
  }
  .tc-l2 {
    height: 4px;
    width: 46%;
    border-radius: 2px;
    background: var(--p-text3);
  }
  .tc-dots {
    display: flex;
    gap: 3px;
    margin-top: 2px;
  }
  .tc-dots i {
    width: 7px;
    height: 7px;
    border-radius: 50%;
  }
  .tc-btn {
    position: absolute;
    right: 8px;
    bottom: 8px;
    width: 26px;
    height: 10px;
    border-radius: 3px;
    background: var(--p-you);
  }
  .tc-name {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 6px;
    padding: 0 4px;
    font-size: 12.5px;
    font-weight: 600;
    min-height: 18px;
  }
  .tc-name > span:first-child {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .tc-name :global(svg) {
    color: var(--you);
    flex: none;
  }
  .badge {
    flex: none;
    font-size: 10.5px;
    font-weight: 600;
    padding: 1px 5px;
    border-radius: var(--r-sm);
    background: var(--you);
    color: var(--you-ink);
  }
  .tweaked {
    position: absolute;
    top: 10px;
    right: 10px;
    font-size: 10.5px;
    padding: 0 5px;
    border-radius: var(--r-sm);
    background: var(--scrim);
    color: var(--text-2);
  }

  /* Live preview of the accent and text colors. */
  .preview {
    display: grid;
    gap: 10px;
    padding: 14px;
    background: var(--panel);
    border: 1px solid var(--seam);
    border-radius: var(--r-lg);
  }
  .pv-row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 24px;
    padding: 0 8px;
    border-radius: var(--r-sm);
    background: color-mix(in oklab, var(--c) 14%, transparent);
    color: var(--c);
    font-size: 12px;
    font-weight: 600;
  }
  .pv-text {
    display: flex;
    gap: 12px;
    margin-left: 8px;
    font-size: 13px;
  }
  .t2 {
    color: var(--text-2);
  }
  .t3 {
    color: var(--text-3);
  }

  .tokens {
    display: grid;
    border: 1px solid var(--seam);
    border-radius: var(--r-lg);
    background: var(--panel);
  }
  .tok {
    display: grid;
    grid-template-columns: 30px 1fr 96px 30px;
    align-items: center;
    gap: 12px;
    padding: 7px 10px;
  }
  .tok + .tok {
    border-top: 1px solid var(--seam);
  }
  .swatch {
    position: relative;
    width: 30px;
    height: 30px;
    border-radius: var(--r-md);
    box-shadow: inset 0 0 0 1px color-mix(in oklab, var(--text) 25%, transparent);
    cursor: pointer;
  }
  .swatch input {
    position: absolute;
    inset: 0;
    opacity: 0;
    width: 100%;
    height: 100%;
    cursor: pointer;
  }
  .swatch:focus-within {
    outline: 2px solid var(--you);
    outline-offset: 2px;
  }
  .tok-name {
    display: grid;
    gap: 1px;
    min-width: 0;
    font-size: 13px;
  }
  .tok-name small {
    color: var(--text-3);
    font-size: 11.5px;
  }
  .tok-name small.warn {
    color: var(--warn);
  }
  .tok.changed .tok-name > span::after {
    content: ' (changed)';
    color: var(--text-3);
    font-size: 11.5px;
  }
  .hex {
    width: 96px;
    text-transform: lowercase;
  }
  .icon-gap {
    width: 30px;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    margin-top: 14px;
  }
  .field.name {
    width: 240px;
  }
</style>
