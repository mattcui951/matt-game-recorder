<script lang="ts">
  import Icon from './Icon.svelte';
  import { mediaSrc } from '../lib/state.svelte';
  import { clock, displayTitle } from '../lib/format.svelte';
  import { t } from '../lib/i18n.svelte';
  import { filterKey, meta } from '../lib/events';
  import type { Recording } from '../lib/types';

  let { rec, active = false, onclick, compact = false }: { rec: Recording; active?: boolean; onclick: () => void; compact?: boolean } = $props();


  const counts = $derived.by(() => {
    const m = new Map<string, number>();
    for (const e of rec.events) m.set(filterKey(e), (m.get(filterKey(e)) ?? 0) + 1);
    return [...m.entries()].slice(0, 4);
  });
  const src = $derived(rec.kind === 'screenshot' ? mediaSrc(rec, rec.file) : mediaSrc(rec, rec.thumb));
</script>

<button class="tile" class:active class:compact {onclick} title={displayTitle(rec.title)}>
  <div class="thumb">
    {#if src}<img {src} alt="" loading="lazy" draggable="false" />{/if}
    {#if rec.kind !== 'screenshot'}<span class="dur num">{clock(rec.duration)}</span>{/if}
    {#if rec.favorite}<span class="fav"><Icon name="star-on" size={14} /></span>{/if}
  </div>
  <div class="meta">
    <span class="title">{displayTitle(rec.title)}</span>
    <span class="sub">
      <span class="kind">{t.kinds[rec.kind] ?? rec.kind}</span>
      {#each counts as [k, n] (k)}
        <span class="ev" style="color:{meta(k).color}"><Icon name={meta(k).icon} size={12} /><span class="num">{n}</span></span>
      {/each}
    </span>
  </div>
</button>

<style>
  .tile {
    display: grid;
    gap: 7px;
    text-align: left;
    padding: 6px;
    border-radius: var(--r-lg);
    transition: background 120ms;
    min-width: 0;
  }
  .tile:hover {
    background: var(--panel-2);
  }
  .tile.active {
    background: var(--panel-3);
    box-shadow: inset 0 0 0 1px var(--you);
  }
  .thumb {
    position: relative;
    aspect-ratio: 16 / 9;
    border-radius: var(--r-md);
    overflow: hidden;
    background: var(--bg-sunk);
  }
  img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }
  .dur {
    position: absolute;
    right: 5px;
    bottom: 5px;
    padding: 1px 6px;
    border-radius: var(--r-sm);
    background: var(--scrim);
    font-size: 11.5px;
  }
  .fav {
    position: absolute;
    left: 6px;
    top: 5px;
    color: var(--objective);
  }
  .meta {
    display: grid;
    gap: 2px;
    min-width: 0;
    padding: 0 2px;
  }
  .title {
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .compact .title {
    font-size: 12.5px;
  }
  .sub {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 11.5px;
    color: var(--text-3);
    min-width: 0;
  }
  .ev {
    display: inline-flex;
    align-items: center;
    gap: 2px;
  }
  .ev .num {
    font-size: 11.5px;
  }
</style>
