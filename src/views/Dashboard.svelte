<script lang="ts">
  import { listen } from '@tauri-apps/api/event';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import Icon from '../components/Icon.svelte';
  import { app, artSrc } from '../lib/state.svelte';
  import { championName, clock, displayTitle, gameName } from '../lib/format.svelte';
  import { eventLabel, meta } from '../lib/events';
  import { t } from '../lib/i18n.svelte';

  let layout = $state<'full' | 'compact'>('full');
  $effect(() => {
    const un = listen('dashboard-layout', () => (layout = layout === 'full' ? 'compact' : 'full'));
    return () => {
      un.then((f) => f());
    };
  });

  const st = $derived(app.status);
  const feed = $derived([...st.events].reverse().slice(0, 30));
  const icon = $derived(artSrc(st.championKey));
</script>

<div class="dash {layout}">
  <header data-tauri-drag-region>
    <span class="live" data-tauri-drag-region>
      {#if st.state === 'recording'}<span class="dot"></span><span class="num">{t.titleBar.rec} {clock(st.elapsed)}</span>{:else}<span class="num">{st.state === 'saving' ? t.common.saving : t.dashboard.waiting}</span>{/if}
    </span>
    <button class="x" onclick={() => getCurrentWindow().close()} aria-label={t.dashboard.close}><Icon name="close" size={14} /></button>
  </header>

  <div class="hero">
    {#if icon}<img src={icon} alt="" />{/if}
    <div class="who">
      <span class="champ">{championName(st.champion) || gameName('League of Legends')}</span>
      <span class="mode">{displayTitle(st.title ?? '')} · {st.mode === 'highlights' ? t.dashboard.highlights : st.mode === 'manual' ? t.dashboard.manual : t.dashboard.fullMatch}</span>
    </div>
  </div>

  <div class="score">
    <div class="cell"><span class="num big k">{st.kda?.kills ?? 0}</span><span>{t.dashboard.kills}</span></div>
    <div class="cell"><span class="num big d">{st.kda?.deaths ?? 0}</span><span>{t.dashboard.deaths}</span></div>
    <div class="cell"><span class="num big a">{st.kda?.assists ?? 0}</span><span>{t.dashboard.assists}</span></div>
    <div class="cell"><span class="num big">{st.apm}</span><span>APM</span></div>
  </div>

  {#if layout === 'full'}
    <div class="feed-head">
      <span>{t.dashboard.moments}</span>
      {#if st.mode === 'highlights'}<span class="num dim">{t.dashboard.clipsSaved(st.clipsSaved)}</span>{/if}
    </div>
    <ol class="feed">
      {#each feed as e (e.t + e.kind)}
        {@const m = meta(e.kind)}
        <li style="--c:{m.color}"><Icon name={m.icon} size={15} /><span class="lbl">{eventLabel(e)}</span><span class="num t">{clock(e.t)}</span></li>
      {:else}
        <li class="none">{t.dashboard.empty}</li>
      {/each}
    </ol>
  {/if}
  {#if st.bookmarking}<div class="bm"><span class="dot"></span>{t.dashboard.bookmarking}</div>{/if}
</div>

<style>
  .dash {
    height: 100vh;
    display: flex;
    flex-direction: column;
    background: var(--bg);
    border: 1px solid var(--seam-strong);
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: 34px;
    padding: 0 6px 0 12px;
    background: var(--bg-sunk);
    border-bottom: 1px solid var(--seam);
  }
  .live {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--death);
    font-size: 13px;
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--death);
    animation: blink 1.2s steps(2, jump-none) infinite;
  }
  @keyframes blink {
    50% {
      opacity: 0.25;
    }
  }
  .x {
    width: 28px;
    height: 26px;
    display: grid;
    place-items: center;
    color: var(--text-3);
    border-radius: var(--r-sm);
  }
  .x:hover {
    background: var(--panel-2);
    color: var(--text);
  }
  .hero {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 14px 14px 10px;
  }
  .hero img {
    width: 52px;
    height: 52px;
    border-radius: var(--r-md);
  }
  .who {
    display: grid;
  }
  .champ {
    font-family: var(--font-score);
    font-weight: 700;
    font-size: 20px;
  }
  .mode {
    color: var(--text-3);
    font-size: 12.5px;
  }
  .score {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    margin: 0 14px;
    border: 1px solid var(--seam);
    border-radius: var(--r-lg);
    background: var(--panel);
  }
  .cell {
    display: grid;
    justify-items: center;
    padding: 10px 0 8px;
    color: var(--text-3);
    font-size: 11.5px;
    border-left: 1px solid var(--seam);
  }
  .cell:first-child {
    border-left: 0;
  }
  .big {
    font-size: 30px;
    line-height: 1.1;
    color: var(--text);
  }
  .k {
    color: var(--you);
  }
  .d {
    color: var(--death);
  }
  .a {
    color: var(--assist);
  }
  .feed-head {
    display: flex;
    justify-content: space-between;
    padding: 16px 14px 6px;
    font-family: var(--font-score);
    font-weight: 600;
    font-size: 13px;
  }
  .dim {
    color: var(--text-3);
    font-family: var(--font-ui);
    font-weight: 400;
  }
  .feed {
    list-style: none;
    margin: 0;
    padding: 0 8px 12px;
    overflow: auto;
    flex: 1;
  }
  .feed li {
    display: grid;
    grid-template-columns: 22px 1fr auto;
    align-items: center;
    gap: 8px;
    height: 32px;
    padding: 0 8px;
    border-radius: var(--r-md);
    color: var(--c);
    animation: slide 300ms var(--ease-out);
  }
  .feed li:first-child {
    background: var(--panel);
  }
  .lbl {
    color: var(--text);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .t {
    color: var(--text-3);
    font-size: 12.5px;
  }
  .none {
    color: var(--text-3) !important;
    font-size: 12.5px;
    display: block !important;
    height: auto !important;
    padding: 8px !important;
  }
  .bm {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 14px;
    border-top: 1px solid var(--seam);
    color: var(--death);
    font-weight: 600;
  }
  @keyframes slide {
    from {
      opacity: 0;
      transform: translateY(-6px);
    }
  }
</style>
