<script lang="ts">
  import Icon from './Icon.svelte';
  import { clock } from '../lib/format.svelte';
  import { t } from '../lib/i18n.svelte';
  import { app, setUiPref } from '../lib/state.svelte';

  let {
    src,
    poster = '',
    time = $bindable(0),
    duration = $bindable(0),
    paused = $bindable(true),
    seekTo = null,
    image = false,
  }: { src: string; poster?: string; time?: number; duration?: number; paused?: boolean; seekTo?: { t: number; n: number } | null; image?: boolean } = $props();

  let video = $state<HTMLVideoElement>();
  let box = $state<HTMLDivElement>();
  let volume = $state(app.settings?.ui.volume ?? 1);
  let muted = $state(false);
  let rate = $state(1);
  let showControls = $state(true);
  let hideTimer: ReturnType<typeof setTimeout> | undefined;
  // In the window the controls sit under the video, so they never cover the game's HUD.
  // Full screen has no room beside the video: they lie over it and fade out when the pointer rests.
  let fs = $state(false);
  $effect(() => {
    const sync = () => {
      fs = !!box && document.fullscreenElement === box;
      if (fs) nudge();
    };
    document.addEventListener('fullscreenchange', sync);
    return () => document.removeEventListener('fullscreenchange', sync);
  });

  // Remembered in settings (saved shortly after the slider stops).
  $effect(() => {
    setUiPref('volume', Math.round(volume * 100) / 100);
  });

  // External seek requests (timeline clicks, moment list).
  $effect(() => {
    if (seekTo && video) {
      video.currentTime = Math.max(0, Math.min(seekTo.t, (video.duration || seekTo.t) - 0.05));
      time = video.currentTime;
    }
  });

  function toggle() {
    if (!video) return;
    if (video.paused) video.play().catch(() => {});
    else video.pause();
  }
  function step(d: number) {
    if (video) video.currentTime = Math.max(0, Math.min(video.duration || 0, video.currentTime + d));
  }
  function fullscreen() {
    if (document.fullscreenElement) document.exitFullscreen();
    else box?.requestFullscreen();
  }
  function nudge() {
    showControls = true;
    clearTimeout(hideTimer);
    hideTimer = setTimeout(() => (showControls = false), 2200);
  }

  export function onKey(e: KeyboardEvent): boolean {
    if (!video) return false;
    switch (e.key) {
      case ' ':
      case 'k':
        toggle();
        return true;
      case 'ArrowLeft':
        step(e.shiftKey ? -1 : -5);
        return true;
      case 'ArrowRight':
        step(e.shiftKey ? 1 : 5);
        return true;
      case 'j':
        step(-10);
        return true;
      case 'l':
        step(10);
        return true;
      case ',':
        video.pause();
        step(-1 / 60);
        return true;
      case '.':
        video.pause();
        step(1 / 60);
        return true;
      case 'm':
        muted = !muted;
        return true;
      case 'f':
        fullscreen();
        return true;
    }
    return false;
  }

  const rates = [0.25, 0.5, 1, 1.5, 2];
</script>

<div class="player" bind:this={box} onpointermove={nudge} class:fs class:hide={!showControls && (fs || !paused)} role="region" aria-label={t.player.region}>
  {#if image}
    <img class="still" {src} alt="" />
  {:else}
    <!-- svelte-ignore a11y_media_has_caption -->
    <video
      bind:this={video}
      {src}
      {poster}
      preload="metadata"
      bind:currentTime={time}
      bind:duration
      bind:paused
      bind:volume
      bind:muted
      bind:playbackRate={rate}
      onclick={toggle}
      ondblclick={fullscreen}
    ></video>
    <div class="controls">
      <button class="c-btn" onclick={toggle} aria-label={paused ? t.player.play : t.player.pause}><Icon name={paused ? 'play' : 'pause'} size={18} /></button>
      <span class="time num">{clock(time)} <span class="dim">/ {clock(duration)}</span></span>
      <div class="grow"></div>
      <div class="rates" role="group" aria-label={t.player.speed}>
        {#each rates as r (r)}
          <button class:on={rate === r} onclick={() => (rate = r)}>{r}x</button>
        {/each}
      </div>
      <button class="c-btn" onclick={() => (muted = !muted)} aria-label={muted ? t.player.unmute : t.player.mute}><Icon name={muted || volume === 0 ? 'mute' : 'volume'} size={18} /></button>
      <input class="vol" type="range" min="0" max="1" step="0.02" bind:value={volume} aria-label={t.player.volume} style="--p:{(muted ? 0 : volume) * 100}%" />
      <button class="c-btn" onclick={fullscreen} aria-label={t.player.fullscreen}><Icon name="fullscreen" size={17} /></button>
    </div>
  {/if}
</div>

<style>
  .player {
    position: relative;
    width: 100%;
    height: 100%;
    background: var(--media-bg);
    border-radius: var(--r-lg);
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }
  .player.hide {
    cursor: none;
  }
  video,
  .still {
    flex: 1;
    min-height: 0;
    width: 100%;
    object-fit: contain;
    display: block;
  }
  .controls {
    flex: none;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 12px;
    /* On the dark media surface: always light text, whatever the theme. */
    --text: var(--media-ink);
    --text-3: var(--media-ink-2);
    --you: var(--media-accent);
    color: var(--media-ink);
  }
  .fs .controls {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    padding: 28px 12px 10px;
    background: linear-gradient(to top, var(--media-scrim), transparent);
    transition: opacity 200ms;
  }
  .fs.hide .controls {
    opacity: 0;
  }
  .c-btn {
    width: 32px;
    height: 32px;
    display: grid;
    place-items: center;
    border-radius: var(--r-md);
    color: var(--text);
  }
  .c-btn:hover {
    background: rgb(255 255 255 / 0.1);
  }
  .time {
    font-size: 14px;
  }
  .dim {
    color: var(--text-3);
  }
  .grow {
    flex: 1;
  }
  .rates {
    display: flex;
    gap: 2px;
  }
  .rates button {
    padding: 3px 7px;
    border-radius: var(--r-sm);
    font-family: var(--font-score);
    font-weight: 600;
    font-size: 12px;
    color: var(--text-3);
  }
  .rates button:hover {
    color: var(--text);
  }
  .rates button.on {
    color: var(--you);
    background: rgb(255 255 255 / 0.08);
  }
  .vol {
    width: 90px;
    appearance: none;
    background: transparent;
    height: 16px;
    cursor: pointer;
  }
  .vol::-webkit-slider-runnable-track {
    height: 3px;
    border-radius: 2px;
    background: linear-gradient(to right, var(--text) var(--p), rgb(255 255 255 / 0.2) var(--p));
  }
  .vol::-webkit-slider-thumb {
    appearance: none;
    width: 12px;
    height: 12px;
    margin-top: -4.5px;
    border-radius: 50%;
    background: var(--text);
  }
</style>
