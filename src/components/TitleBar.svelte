<script lang="ts">
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import Icon from './Icon.svelte';
  import Popover from './Popover.svelte';
  import { api } from '../lib/api';
  import { app, go, updateSettings, ui } from '../lib/state.svelte';
  import { bytes, clock, displayTitle, gameName } from '../lib/format.svelte';
  import { t } from '../lib/i18n.svelte';

  const win = getCurrentWindow();
  let diskOpen = $state(false);
  let eventsOpen = $state(false);
  let screenOpen = $state(false);
  let layoutOpen = $state(false);

  const disk = $derived(app.disk);
  const pct = (n: number) => (disk && disk.total ? (n / disk.total) * 100 : 0);
  const usedPct = $derived(disk ? pct(disk.total - disk.free) : 0);
  // The library can only be part of what is used (it may live on another drive's quota).
  const libPct = $derived(disk ? Math.min(pct(disk.library), usedPct) : 0);
  const diskColor = $derived(disk?.level === 'critical' ? 'var(--death)' : disk?.level === 'low' ? 'var(--warn)' : 'var(--win)');
  const diskText = $derived(disk?.level === 'critical' ? t.titleBar.criticalText : disk?.level === 'low' ? t.titleBar.lowText : t.titleBar.goodText);
  const diskState = $derived(disk?.level === 'critical' ? t.titleBar.critical : disk?.level === 'low' ? t.titleBar.low : t.titleBar.good);

  const ev = $derived(app.status.eventsStatus);
  const evColor = $derived(ev === 'down' ? 'var(--death)' : ev === 'partial' ? 'var(--warn)' : 'var(--win)');
  const evText = $derived(ev === 'good' ? t.titleBar.eventsGood : ev === 'down' ? t.titleBar.eventsDown : ev === 'partial' ? t.titleBar.eventsPartial : t.titleBar.eventsIdle);
  const rec = $derived(app.status.state === 'recording');
  const importJob = $derived(Object.values(app.jobs).find((j) => j.kind === 'import' && j.state === 'running'));
</script>

<header class="bar" data-tauri-drag-region>
  <div class="brand" data-tauri-drag-region>
    <svg width="18" height="18" viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="8" fill="none" stroke="var(--you)" stroke-width="2.6" /><circle cx="12" cy="12" r="4" fill="var(--death)" /></svg>
    <span class="name" data-tauri-drag-region>Matt Game Recorder</span>
    {#if rec}
      <button class="rec-chip" onclick={() => app.status.sessionId && go({ name: 'session', sessionId: app.status.sessionId })}>
        <span class="dot"></span>
        <span class="num">{t.titleBar.rec} {clock(app.status.elapsed)}</span>
        <span class="what">{displayTitle(app.status.title ?? '')}</span>
      </button>
    {:else if app.status.state === 'saving'}
      <span class="rec-chip saving"><span class="num">{app.status.message ?? t.common.saving}</span></span>
    {/if}
    {#if importJob && app.route.name !== 'library'}
      <button class="rec-chip imp" onclick={() => go({ name: 'library', tab: 'sessions' })} title={t.titleBar.showImport}>
        <span class="num">{(importJob.paused ? t.titleBar.importPaused : t.titleBar.importing)(Math.round(importJob.progress * 100))}</span>
      </button>
    {/if}
    {#if app.status.state === 'error'}
      <span class="rec-chip err" title={app.status.message ?? ''}><span class="num">{t.titleBar.captureError}</span></span>
    {/if}
  </div>

  <div class="tools">
    <Popover bind:open={diskOpen} width={260}>
      {#snippet anchor(toggle)}
        <button class="pill" onclick={toggle} aria-label={t.titleBar.diskButton(bytes(disk?.free ?? 0), Math.round(usedPct))}>
          <span class="num">{t.titleBar.diskFree(bytes(disk?.free ?? 0))}</span>
          <span class="meter"><span style="width:{Math.max(4, usedPct)}%;background:{diskColor}"></span></span>
        </button>
      {/snippet}
      {#snippet children()}
        <div class="pop-body">
          <div class="big num">{bytes(disk?.free ?? 0)} <span class="of">{t.titleBar.freeOf(bytes(disk?.total ?? 0))}</span></div>
          <div class="bar-lg" role="img" aria-label={t.titleBar.usedAria(Math.round(usedPct))}>
            <span style="width:{libPct}%;background:{diskColor}"></span>
            <span style="width:{usedPct - libPct}%;background:var(--text-3)"></span>
          </div>
          <div class="keys">
            <span><i style="background:{diskColor}"></i>{t.titleBar.recordings} <b class="num">{bytes(disk?.library ?? 0)}</b></span>
            <span><i style="background:var(--text-3)"></i>{t.titleBar.otherFiles} <b class="num">{bytes(Math.max(0, (disk?.total ?? 0) - (disk?.free ?? 0) - (disk?.library ?? 0)))}</b></span>
            <span><i class="free"></i>{t.titleBar.free} <b class="num">{bytes(disk?.free ?? 0)}</b></span>
          </div>
          <p class="note"><strong style="color:{diskColor}">{diskState}</strong> {diskText}</p>
          <button class="btn sm" onclick={() => { diskOpen = false; go({ name: 'settings', page: 'storage' }); }}>{t.titleBar.storageSettings}</button>
        </div>
      {/snippet}
    </Popover>

    <Popover bind:open={eventsOpen} width={290}>
      {#snippet anchor(toggle)}
        <button class="pill" onclick={toggle} aria-label={t.titleBar.gameEventsAria}>
          <span>{t.titleBar.gameEvents}</span>
          <span class="light" style="background:{evColor}"></span>
        </button>
      {/snippet}
      {#snippet children()}
        <div class="pop-body">
          <div class="game-row">
            <span>{gameName('League of Legends')}</span>
            <span class="light" style="background:{evColor}"></span>
          </div>
          <p class="note">{evText}</p>
          <div class="legend">
            <span><i style="background:var(--win)"></i>{t.titleBar.legendGood}</span>
            <span><i style="background:var(--warn)"></i>{t.titleBar.legendConnecting}</span>
            <span><i style="background:var(--death)"></i>{t.titleBar.legendDown}</span>
          </div>
        </div>
      {/snippet}
    </Popover>

    <Popover bind:open={screenOpen} width={230}>
      {#snippet anchor(toggle)}
        <button class="icon-btn" class:on={app.status.screenRecording} onclick={toggle} aria-label={t.titleBar.screenRecorder} title={t.titleBar.screenRecorder}>
          <Icon name="monitor" size={17} />
        </button>
      {/snippet}
      {#snippet children()}
        <div class="pop-body center">
          <div class="pop-title">{t.titleBar.screenRecorder}</div>
          <span class="kbd num">{app.settings?.screenRecorder.hotkeyToggle}</span>
          <button class="rec-btn" class:live={app.status.screenRecording} onclick={() => api.toggleScreenRecorder()} aria-label={app.status.screenRecording ? t.titleBar.stopScreen : t.titleBar.startScreen}>
            <Icon name={app.status.screenRecording ? 'stop' : 'record'} size={26} />
          </button>
          <span class="num timer">{clock(app.status.screenElapsed)}</span>
          <button class="btn ghost sm" onclick={() => { screenOpen = false; go({ name: 'settings', page: 'screen' }); }}>{t.titleBar.openSettings}</button>
        </div>
      {/snippet}
    </Popover>

    <Popover bind:open={layoutOpen} width={240}>
      {#snippet anchor(toggle)}
        <button class="icon-btn" onclick={toggle} aria-label={t.titleBar.layout} title={t.titleBar.layout}>
          <Icon name="layout" size={17} />
        </button>
      {/snippet}
      {#snippet children()}
        <div class="pop-body">
          <div class="pop-title">{t.titleBar.appLayout}</div>
          <button class="opt" class:on={app.settings?.general.layout === 'art'} onclick={() => updateSettings((s) => (s.general.layout = 'art'))}>
            <Icon name="image" size={16} /> {t.titleBar.showArt}
          </button>
          <button class="opt" class:on={app.settings?.general.layout === 'plain'} onclick={() => updateSettings((s) => (s.general.layout = 'plain'))}>
            <Icon name="layout" size={16} /> {t.titleBar.hideArt}
          </button>
          <div class="pop-title sub">{t.titleBar.libraryView}</div>
          <button class="opt" class:on={ui.libraryView === 'table'} onclick={() => (ui.libraryView = 'table')}><Icon name="rows" size={16} /> {t.titleBar.scoreTable}</button>
          <button class="opt" class:on={ui.libraryView === 'grid'} onclick={() => (ui.libraryView = 'grid')}><Icon name="grid" size={16} /> {t.titleBar.thumbGrid}</button>
        </div>
      {/snippet}
    </Popover>

    <div class="sep"></div>
    <button class="win-btn" aria-label={t.titleBar.minimize} onclick={() => win.minimize()}><Icon name="minimize" size={15} /></button>
    <button class="win-btn" aria-label={t.titleBar.maximize} onclick={() => win.toggleMaximize()}><Icon name="maximize" size={13} /></button>
    <button class="win-btn close" aria-label={t.titleBar.closeToTray} title={t.titleBar.closeTitle} onclick={() => win.close()}><Icon name="close" size={15} /></button>
  </div>
</header>

<style>
  .bar {
    height: 40px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding-left: 14px;
    background: var(--bg-sunk);
    border-bottom: 1px solid var(--seam);
    flex: none;
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 9px;
    min-width: 0;
  }
  .name {
    font-family: var(--font-score);
    font-weight: 700;
    font-size: 14px;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    white-space: nowrap;
  }
  .rec-chip {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-left: 12px;
    height: 24px;
    padding: 0 10px;
    border-radius: var(--r-sm);
    background: color-mix(in oklab, var(--death) 14%, transparent);
    color: var(--death);
    font-size: 12.5px;
    white-space: nowrap;
  }
  .rec-chip .what {
    color: var(--text-2);
  }
  .rec-chip.imp {
    background: color-mix(in oklab, var(--you) 14%, transparent);
    color: var(--you);
  }
  .rec-chip.saving {
    background: var(--panel-2);
    color: var(--text-2);
  }
  .rec-chip.err {
    background: color-mix(in oklab, var(--warn) 16%, transparent);
    color: var(--warn);
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
  .tools {
    display: flex;
    align-items: center;
    gap: 4px;
    height: 100%;
  }
  .pill {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 28px;
    padding: 0 10px;
    border-radius: var(--r-md);
    color: var(--text-2);
    font-size: 12.5px;
  }
  .pill:hover {
    background: var(--panel-2);
    color: var(--text);
  }
  .meter {
    width: 28px;
    height: 9px;
    border-radius: 2px;
    background: var(--panel-3);
    box-shadow: inset 0 0 0 1px var(--seam-strong);
    overflow: hidden;
    display: flex;
  }
  .meter span {
    height: 100%;
  }
  .light {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    box-shadow: 0 0 0 3px color-mix(in oklab, currentColor 0%, transparent);
  }
  .sep {
    width: 1px;
    height: 18px;
    background: var(--seam);
    margin: 0 6px;
  }
  .win-btn {
    width: 44px;
    height: 100%;
    display: grid;
    place-items: center;
    color: var(--text-2);
  }
  .win-btn:hover {
    background: var(--panel-2);
    color: var(--text);
  }
  .win-btn.close:hover {
    background: var(--death);
    color: white;
  }
  .pop-body {
    display: grid;
    gap: 10px;
  }
  .pop-body.center {
    justify-items: center;
    text-align: center;
  }
  .pop-title {
    font-family: var(--font-score);
    font-weight: 600;
    font-size: 14px;
  }
  .pop-title.sub {
    margin-top: 6px;
    font-size: 12.5px;
    color: var(--text-3);
  }
  .big {
    font-size: 22px;
  }
  .of {
    font-family: var(--font-ui);
    font-weight: 400;
    font-size: 12.5px;
    color: var(--text-3);
  }
  .bar-lg {
    height: 6px;
    border-radius: 3px;
    background: var(--panel-3);
    overflow: hidden;
  }
  .bar-lg {
    display: flex;
  }
  .bar-lg span {
    height: 100%;
  }
  .keys {
    display: grid;
    gap: 5px;
    font-size: 12.5px;
    color: var(--text-2);
  }
  .keys span {
    display: flex;
    align-items: center;
  }
  .keys i {
    width: 8px;
    height: 8px;
    border-radius: 2px;
    margin-right: 8px;
  }
  .keys i.free {
    background: var(--panel-3);
    box-shadow: inset 0 0 0 1px var(--seam-strong);
  }
  .keys b {
    margin-left: auto;
    font-weight: 500;
    color: var(--text);
  }
  .note {
    font-size: 12.5px;
    color: var(--text-2);
  }
  .game-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 8px 10px;
    background: var(--panel-2);
    border-radius: var(--r-md);
  }
  .legend {
    display: flex;
    gap: 12px;
    font-size: 11.5px;
    color: var(--text-3);
  }
  .legend i {
    display: inline-block;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    margin-right: 5px;
  }
  .kbd {
    padding: 2px 8px;
    border: 1px solid var(--seam-strong);
    border-radius: var(--r-sm);
    font-size: 12px;
    color: var(--text-2);
  }
  .rec-btn {
    width: 54px;
    height: 54px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    color: var(--death);
    background: var(--panel-2);
    box-shadow: inset 0 0 0 2px color-mix(in oklab, var(--death) 50%, transparent);
  }
  .rec-btn:hover {
    background: var(--panel-3);
  }
  .rec-btn.live {
    background: var(--death);
    color: white;
  }
  .timer {
    font-size: 18px;
  }
  .opt {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 34px;
    padding: 0 10px;
    border-radius: var(--r-md);
    color: var(--text-2);
    text-align: left;
  }
  .opt:hover {
    background: var(--panel-2);
    color: var(--text);
  }
  .opt.on {
    background: var(--panel-3);
    color: var(--you);
  }
</style>
