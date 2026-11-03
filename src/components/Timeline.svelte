<script lang="ts">
  import Icon from './Icon.svelte';
  import { clock } from '../lib/format.svelte';
  import { eventLabel, meta } from '../lib/events';
  import { t } from '../lib/i18n.svelte';
  import type { RecEvent } from '../lib/types';

  type Range = { a: number; b: number };
  let {
    duration,
    time,
    onseek,
    events,
    apm = [],
    showApm = false,
    sprite = null,
    mode = 'view',
    trim = $bindable({ a: 0, b: 30 }),
    selection = $bindable([] as Range[]),
    zoom = $bindable(1),
  }: {
    duration: number;
    time: number;
    onseek: (t: number) => void;
    events: RecEvent[];
    apm?: number[];
    showApm?: boolean;
    sprite?: string | null;
    mode?: 'view' | 'trim' | 'select';
    trim?: Range;
    selection?: Range[];
    zoom?: number;
  } = $props();

  let scroller = $state<HTMLDivElement>();
  let viewW = $state(800);
  const innerW = $derived(viewW * zoom);
  const d = $derived(Math.max(duration, 0.001));
  const x = (t: number) => (t / d) * innerW;
  const tAt = (px: number) => Math.max(0, Math.min(d, (px / innerW) * d));

  // Chips that would overlap move to a second row.
  const placed = $derived.by(() => {
    const last = [-1e9, -1e9];
    return [...events]
      .sort((p, q) => p.t - q.t)
      .map((e) => {
        const px = x(e.t);
        const row = px - last[0] >= 24 ? 0 : px - last[1] >= 24 ? 1 : 0;
        last[row] = px;
        return { e, px, row };
      });
  });

  const ticks = $derived.by(() => {
    const steps = [5, 10, 15, 30, 60, 120, 300, 600, 1200];
    const pxPerSec = innerW / d;
    const step = steps.find((s) => s * pxPerSec >= 80) ?? 1800;
    const out: { t: number; major: boolean }[] = [];
    for (let t = 0; t <= d; t += step / 2) out.push({ t, major: Math.round(t / step) * step === Math.round(t) });
    return out;
  });

  // APM: actions in a 10 s window, scaled to a minute.
  const apmSeries = $derived.by(() => {
    if (!apm.length) return [] as number[];
    const s: number[] = [];
    for (let i = 0; i < apm.length; i++) {
      let sum = 0;
      for (let j = Math.max(0, i - 5); j < Math.min(apm.length, i + 5); j++) sum += apm[j];
      s.push(sum * 6);
    }
    return s;
  });
  const apmMax = $derived(Math.max(60, ...apmSeries));
  const apmPath = $derived.by(() => {
    if (!apmSeries.length) return '';
    const H = 40;
    let p = `M0 ${H}`;
    apmSeries.forEach((v, i) => (p += ` L${(i / d) * 1000} ${H - (v / apmMax) * (H - 3)}`));
    return p + ` L${((apmSeries.length - 1) / d) * 1000} ${H} Z`;
  });

  // Hover preview
  let hover = $state<{ px: number; t: number; label?: string; color?: string } | null>(null);
  let spriteImg = $state<{ w: number; h: number; n: number } | null>(null);
  $effect(() => {
    spriteImg = null;
    if (!sprite) return;
    const img = new Image();
    img.onload = () => {
      const fw = 160;
      spriteImg = { w: img.naturalWidth, h: img.naturalHeight, n: Math.max(1, Math.round(img.naturalWidth / fw)) };
    };
    img.src = sprite;
  });
  const frame = $derived(hover && spriteImg ? Math.min(spriteImg.n - 1, Math.floor((hover.t / d) * spriteImg.n)) : 0);

  // Pointer: scrub, select or drag trim handles.
  let drag: { kind: 'scrub' | 'select' | 'a' | 'b'; start: number } | null = null;
  function localX(e: PointerEvent): number {
    const r = scroller!.getBoundingClientRect();
    return e.clientX - r.left + scroller!.scrollLeft;
  }
  function down(e: PointerEvent) {
    if (e.button !== 0) return;
    const t = tAt(localX(e));
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    if (mode === 'select') {
      drag = { kind: 'select', start: t };
      selection = [...selection, { a: t, b: t }];
    } else {
      drag = { kind: 'scrub', start: t };
      onseek(t);
    }
  }
  function move(e: PointerEvent) {
    const t = tAt(localX(e));
    hover = { px: localX(e) - (scroller?.scrollLeft ?? 0), t };
    if (!drag) return;
    if (drag.kind === 'scrub') onseek(t);
    else if (drag.kind === 'select') {
      const s = [...selection];
      s[s.length - 1] = { a: Math.min(drag.start, t), b: Math.max(drag.start, t) };
      selection = s;
    } else if (drag.kind === 'a') trim = { a: Math.min(t, trim.b - 1), b: trim.b };
    else if (drag.kind === 'b') trim = { a: trim.a, b: Math.max(t, trim.a + 1) };
  }
  function up() {
    if (drag?.kind === 'select') {
      // A click without a drag selects nothing.
      selection = selection.filter((r) => r.b - r.a >= 0.5);
    }
    drag = null;
  }
  function handleDown(e: PointerEvent, which: 'a' | 'b') {
    e.stopPropagation();
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    drag = { kind: which, start: 0 };
  }
  function chipClick(e: MouseEvent, ev: RecEvent) {
    e.stopPropagation();
    if (mode === 'select') {
      const r = { a: Math.max(0, ev.t - ev.before), b: Math.min(d, ev.t + ev.after) };
      const i = selection.findIndex((s) => Math.abs(s.a - r.a) < 0.01 && Math.abs(s.b - r.b) < 0.01);
      selection = i >= 0 ? selection.filter((_, j) => j !== i) : [...selection, r];
    } else {
      onseek(Math.max(0, ev.t - ev.before));
    }
  }

  function onWheel(e: WheelEvent) {
    if (!e.ctrlKey || !scroller) return;
    e.preventDefault();
    const anchorT = tAt(e.clientX - scroller.getBoundingClientRect().left + scroller.scrollLeft);
    zoom = Math.max(1, Math.min(48, zoom * (e.deltaY < 0 ? 1.25 : 0.8)));
    requestAnimationFrame(() => {
      if (scroller) scroller.scrollLeft = x(anchorT) - (e.clientX - scroller.getBoundingClientRect().left);
    });
  }

  // Keep the playhead in view while zoomed.
  $effect(() => {
    if (!scroller || zoom <= 1 || drag) return;
    const px = x(time);
    if (px < scroller.scrollLeft + 40 || px > scroller.scrollLeft + viewW - 40) scroller.scrollLeft = px - viewW * 0.3;
  });
  // Zooming into a trim range centers it.
  export function focusRange(a: number, b: number) {
    zoom = Math.max(1, Math.min(48, (d / Math.max(1, b - a)) * 0.5));
    requestAnimationFrame(() => {
      if (scroller) scroller.scrollLeft = x((a + b) / 2) - viewW / 2;
    });
  }
</script>

<div class="tl" class:apm={showApm && apmSeries.length > 0}>
  <div class="scroller" bind:this={scroller} bind:clientWidth={viewW} onwheel={onWheel}>
    <div
      class="inner"
      style="width:{innerW}px"
      role="slider"
      tabindex="-1"
      aria-label={t.timeline.label}
      aria-valuemin={0}
      aria-valuemax={Math.round(d)}
      aria-valuenow={Math.round(time)}
      onpointerdown={down}
      onpointermove={move}
      onpointerup={up}
      onpointerleave={() => (hover = null)}
    >
      <div class="lane">
        {#each placed as p (p.e.t + p.e.kind)}
          {@const m = meta(p.e.kind)}
          <span class="window" style="left:{x(Math.max(0, p.e.t - p.e.before))}px;width:{Math.max(2, x(p.e.before + p.e.after))}px;--c:{m.color}"></span>
          <button
            class="chip row{p.row}"
            class:multi={p.e.multi}
            style="left:{p.px}px;--c:{m.color}"
            onpointerdown={(e) => e.stopPropagation()}
            onclick={(e) => chipClick(e, p.e)}
            onpointerenter={() => (hover = { px: p.px - (scroller?.scrollLeft ?? 0), t: p.e.t, label: eventLabel(p.e.label), color: m.color })}
            aria-label="{p.e.label} at {clock(p.e.t)}"
          >
            <Icon name={m.icon} size={14} />
            {#if p.e.multi}<span class="badge num">{p.e.multi}</span>{/if}
          </button>
        {/each}
      </div>
      {#if showApm && apmSeries.length}
        <svg class="apm-g" viewBox="0 0 {(apmSeries.length / d) * 1000} 40" preserveAspectRatio="none" style="width:{x(apmSeries.length)}px" aria-hidden="true">
          <path d={apmPath} />
        </svg>
        <span class="apm-max num">{Math.round(apmMax)} {t.timeline.apm}</span>
      {/if}
      <div class="ruler">
        {#each ticks as tk (tk.t)}
          <span class="tick" class:major={tk.major} style="left:{x(tk.t)}px">{#if tk.major}<span class="lbl num">{clock(tk.t)}</span>{/if}</span>
        {/each}
      </div>

      {#each selection as r, i (i)}
        <span class="sel" style="left:{x(r.a)}px;width:{x(r.b - r.a)}px"></span>
      {/each}
      {#if mode === 'trim'}
        <span class="shade" style="left:0;width:{x(trim.a)}px"></span>
        <span class="shade" style="left:{x(trim.b)}px;right:0"></span>
        <span class="trim" style="left:{x(trim.a)}px;width:{x(trim.b - trim.a)}px">
          <span class="len num">{clock(trim.b - trim.a, true)}</span>
        </span>
        <span class="handle" style="left:{x(trim.a)}px" role="slider" tabindex="0" aria-label={t.timeline.clipStart} aria-valuenow={trim.a} onpointerdown={(e) => handleDown(e, 'a')}></span>
        <span class="handle" style="left:{x(trim.b)}px" role="slider" tabindex="0" aria-label={t.timeline.clipEnd} aria-valuenow={trim.b} onpointerdown={(e) => handleDown(e, 'b')}></span>
      {/if}
      <span class="playhead" style="left:{x(time)}px"></span>
    </div>
  </div>

  {#if hover}
    <div class="hover" style="left:{Math.max(90, Math.min(viewW - 90, hover.px))}px">
      {#if spriteImg && sprite}
        <span class="frame" style="background-image:url('{sprite}');background-position:-{frame * 160}px 0;height:{spriteImg.h}px"></span>
      {/if}
      {#if hover.label}<span class="h-label" style="color:{hover.color}">{hover.label}</span>{/if}
      <span class="h-time num">{clock(hover.t)}</span>
    </div>
  {/if}
</div>

<style>
  .tl {
    position: relative;
    user-select: none;
  }
  .scroller {
    overflow-x: auto;
    overflow-y: hidden;
  }
  .inner {
    position: relative;
    height: 96px;
    cursor: pointer;
    min-width: 100%;
  }
  .apm .inner {
    height: 138px;
  }
  .lane {
    position: absolute;
    left: 0;
    right: 0;
    top: 0;
    height: 66px;
  }
  .window {
    position: absolute;
    top: 56px;
    height: 6px;
    border-radius: 1px;
    background: color-mix(in oklab, var(--c) 34%, transparent);
    box-shadow: inset 0 1px 0 var(--c);
  }
  .chip {
    position: absolute;
    width: 22px;
    height: 22px;
    margin-left: -11px;
    display: grid;
    place-items: center;
    border-radius: var(--r-sm);
    background: var(--panel-2);
    color: var(--c);
    box-shadow: inset 0 0 0 1px color-mix(in oklab, var(--c) 45%, transparent);
    transition: transform 100ms var(--ease-out), background 100ms;
    z-index: 2;
  }
  /* Above neighbouring chips, so the kill-count badge is never covered by the next marker. */
  .chip.multi {
    z-index: 3;
  }
  .chip.row0 {
    top: 6px;
  }
  .chip.row1 {
    top: 31px;
  }
  .chip:hover {
    background: color-mix(in oklab, var(--c) 22%, var(--panel-2));
    transform: translateY(-1px);
  }
  .badge {
    position: absolute;
    top: -6px;
    right: -7px;
    min-width: 14px;
    height: 14px;
    padding: 0 3px;
    border-radius: 7px;
    background: var(--c);
    color: var(--you-ink);
    font-size: 10px;
    line-height: 14px;
  }
  .apm-g {
    position: absolute;
    top: 70px;
    left: 0;
    height: 40px;
  }
  .apm-g path {
    fill: color-mix(in oklab, var(--you) 22%, transparent);
    stroke: var(--you);
    stroke-width: 1.2;
    vector-effect: non-scaling-stroke;
  }
  .apm-max {
    position: absolute;
    top: 68px;
    left: 4px;
    font-size: 10.5px;
    color: var(--text-3);
    pointer-events: none;
  }
  .ruler {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    height: 26px;
    border-top: 1px solid var(--seam);
  }
  .tick {
    position: absolute;
    top: 0;
    width: 1px;
    height: 5px;
    background: var(--seam-strong);
  }
  .tick.major {
    height: 9px;
    background: var(--text-3);
  }
  .lbl {
    position: absolute;
    top: 9px;
    left: 4px;
    font-size: 11px;
    color: var(--text-3);
    white-space: nowrap;
  }
  .playhead {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 2px;
    margin-left: -1px;
    background: var(--text);
    pointer-events: none;
    z-index: 3;
  }
  .playhead::before {
    content: '';
    position: absolute;
    bottom: 0;
    left: -4px;
    border: 5px solid transparent;
    border-bottom-color: var(--text);
  }
  .sel {
    position: absolute;
    top: 0;
    bottom: 26px;
    background: color-mix(in oklab, var(--you) 16%, transparent);
    box-shadow: inset 0 0 0 1px var(--you);
    pointer-events: none;
  }
  .shade {
    position: absolute;
    top: 0;
    bottom: 0;
    background: var(--shade);
    pointer-events: none;
    z-index: 1;
  }
  .trim {
    position: absolute;
    top: 0;
    bottom: 0;
    box-shadow: inset 0 0 0 2px var(--you);
    pointer-events: none;
    z-index: 1;
  }
  .len {
    position: absolute;
    bottom: 26px;
    left: 50%;
    transform: translateX(-50%);
    padding: 1px 6px;
    background: var(--you);
    color: var(--you-ink);
    font-size: 11px;
    border-radius: var(--r-sm) var(--r-sm) 0 0;
    white-space: nowrap;
  }
  .handle {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 12px;
    margin-left: -6px;
    cursor: ew-resize;
    z-index: 4;
  }
  .handle::after {
    content: '';
    position: absolute;
    left: 3px;
    top: 30%;
    bottom: 30%;
    width: 6px;
    border-radius: 3px;
    background: var(--you);
  }
  .hover {
    position: absolute;
    bottom: calc(100% + 8px);
    transform: translateX(-50%);
    display: grid;
    justify-items: center;
    gap: 4px;
    padding: 6px;
    background: var(--panel);
    border: 1px solid var(--seam-strong);
    border-radius: var(--r-md);
    box-shadow: var(--shadow-pop);
    pointer-events: none;
    z-index: 20;
  }
  .frame {
    display: block;
    width: 160px;
    border-radius: var(--r-sm);
    background-repeat: no-repeat;
  }
  .h-label {
    font-weight: 600;
    font-size: 12.5px;
    white-space: nowrap;
  }
  .h-time {
    font-size: 12px;
    color: var(--text-2);
  }
</style>
