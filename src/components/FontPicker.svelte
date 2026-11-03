<script lang="ts">
  import Icon from './Icon.svelte';
  import Popover from './Popover.svelte';
  import { api } from '../lib/api';
  import { BUNDLED_FONTS } from '../lib/appearance';
  import { t } from '../lib/i18n.svelte';

  // Pick any installed font; each name is shown in its own face. Empty value = the theme's font.
  let { value, fallback, label, onchange }: { value: string; fallback: string; label: string; onchange: (v: string) => void } = $props();

  let open = $state(false);
  let query = $state('');
  let fonts = $state<string[]>([]);
  let loading = $state(false);

  async function load() {
    if (fonts.length || loading) return;
    loading = true;
    try {
      fonts = [...new Set([...BUNDLED_FONTS, ...(await api.systemFonts())])].sort((a, b) => a.localeCompare(b));
    } catch {
      fonts = [...BUNDLED_FONTS];
    } finally {
      loading = false;
    }
  }
  $effect(() => {
    if (open) {
      load();
      query = '';
    }
  });

  const shown = $derived.by(() => {
    const q = query.trim().toLowerCase();
    return q ? fonts.filter((f) => f.toLowerCase().includes(q)) : fonts;
  });
  const current = $derived(value || fallback);
  const face = (f: string) => `font-family: "${f.replace(/"/g, '')}", system-ui`;

  function pick(v: string, close: () => void) {
    onchange(v);
    close();
  }
  function focusOnMount(el: HTMLInputElement) {
    el.focus();
  }
</script>

<Popover bind:open width={300} align="right">
  {#snippet anchor(toggle)}
    <button class="trigger" onclick={toggle} aria-label="{label}: {current}" aria-expanded={open}>
      <span class="name" style={face(current)}>{current}</span>
      {#if !value}<span class="tag">{t.fonts.theme}</span>{/if}
      <Icon name="chevron-down" size={14} />
    </button>
  {/snippet}
  {#snippet children(close)}
    <div class="fp">
      <div class="search">
        <Icon name="search" size={14} />
        <input class="field" placeholder={t.fonts.search(fonts.length || '')} bind:value={query} aria-label={t.fonts.searchAria} use:focusOnMount />
      </div>
      <div class="list" role="listbox" aria-label={label}>
        {#if !query}
          <button class="opt" class:on={!value} role="option" aria-selected={!value} onclick={() => pick('', close)}>
            <span style={face(fallback)}>{fallback}</span><span class="tag">{t.fonts.themeDefault}</span>
          </button>
        {/if}
        {#each shown as f (f)}
          <button class="opt" class:on={value === f} role="option" aria-selected={value === f} onclick={() => pick(f, close)}>
            <span style={face(f)}>{f}</span>
            {#if BUNDLED_FONTS.includes(f)}<span class="tag">{t.fonts.builtIn}</span>{/if}
          </button>
        {:else}
          <p class="empty">{loading ? t.fonts.loading : t.fonts.none}</p>
        {/each}
      </div>
    </div>
  {/snippet}
</Popover>

<style>
  .trigger {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 260px;
    height: 34px;
    padding: 0 10px;
    background: var(--panel-2);
    border-radius: var(--r-md);
    box-shadow: inset 0 0 0 1px var(--seam);
    color: var(--text);
    text-align: left;
  }
  .trigger:hover {
    box-shadow: inset 0 0 0 1px var(--seam-strong);
  }
  .name {
    flex: 1;
    min-width: 0;
    font-size: 14px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .tag {
    flex: none;
    font-size: 11px;
    color: var(--text-3);
  }
  .fp {
    display: grid;
    gap: 8px;
  }
  .search {
    position: relative;
    color: var(--text-3);
  }
  .search :global(svg) {
    position: absolute;
    left: 10px;
    top: 9px;
  }
  .search input {
    width: 100%;
    padding-left: 30px;
  }
  .list {
    max-height: 300px;
    overflow: auto;
    display: grid;
    gap: 1px;
    margin: 0 -4px;
  }
  .opt {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    min-height: 32px;
    padding: 4px 8px;
    border-radius: var(--r-sm);
    color: var(--text);
    font-size: 14.5px;
    text-align: left;
  }
  .opt span:first-child {
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .opt:hover {
    background: var(--panel-2);
  }
  .opt.on {
    background: var(--panel-3);
    color: var(--you);
  }
  .empty {
    margin: 8px;
    color: var(--text-3);
    font-size: 12.5px;
  }
</style>
