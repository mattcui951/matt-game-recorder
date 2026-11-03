<script lang="ts">
  import Icon from './Icon.svelte';
  import { clock } from '../lib/format.svelte';
  import { eventLabel, LANES, laneOf, meta, rank } from '../lib/events';
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
  // Room at both ends so the first and last marks are not cut in half.
  const PAD = 14;
  const x = (t: number) => PAD + (t / d) * (innerW - 2 * PAD);
  const tAt = (px: number) => Math.max(0, Math.min(d, ((px - PAD) / (innerW - 2 * PAD)) * d));
  /** Width in px of a stretch of time (no padding). */
  const w = (secs: number) => (secs / d) * (innerW - 2 * PAD);

  // One row per kind of moment (fights, team play, the map), most important on top; rows with
  // nothing to show are left out. Marks that would touch in a row become one group mark with a
  // count; clicking it zooms in until they come apart.
  const CHIP = 22;
  const GAP = 4;
  /** A multikill shows its name ("Double"), so it needs more room than an icon. */
  const STREAK = 74;
  const ROW_H = 28;
  type Mark = { lane: number; px: number; end: number; items: RecEvent[] };
  const marks = $derived.by(() => {
    const out: Mark[] = [];
    const last: (Mark | undefined)[] = [];
    for (const e of [...events].sort((p, q) => p.t - q.t)) {
      const lane = laneOf(e.kind);
      const px = x(e.t);
      const end = e.kind === 'multikill' ? Math.max(px + w(e.span ?? 0), px + STREAK - CHIP) : px;
      const cur = last[lane];
      if (cur && px - CHIP / 2 < cur.end + CHIP / 2 + GAP) {
        cur.items.push(e);
        cur.end = Math.max(cur.end, end);
      } else {
        const m = { lane, px, end, items: [e] };
        out.push(m);
        last[lane] = m;
      }
    }
    return out;
  });
  const rows = $derived(LANES.map((_, i) => i).filter((i) => marks.some((m) => m.lane === i)));
  const rowOf = (lane: number) => Math.max(0, rows.indexOf(lane));
  const lanesH = $derived(Math.max(1, rows.length) * ROW_H + 6);
  const lead = (m: Mark) => m.items.reduce((a, b) => (rank(b) > rank(a) ? b : a));
  /** Where a mark's left edge goes: at its moment, but never past the end of the timeline. */
  function markLeft(m: Mark): number {
    const single = m.items.length === 1;
    const width = single ? m.end - m.px + CHIP : m.end - m.px + CHIP + kindsOf(m).length * 13 + 16;
    return Math.min(m.px, innerW - width + CHIP / 2 - 2);
  }
  /** Distinct kinds in a group, most important first (up to three icons). */
  const kindsOf = (m: Mark) => [...new Set([...m.items].sort((a, b) => rank(b) - rank(a)).map((e) => e.kind))].slice(0, 3);

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
  type Line = { label: string; color: string; t: number };
  let hover = $state<{ px: number; t: number; lines?: Line[] } | null>(null);
  const HOVER_LINES = 6;
  function hoverMark(m: Mark) {
    const lines = m.items.map((e) => ({ label: eventLabel(e), color: meta(e.kind).color, t: e.t }));
    hover = { px: m.px - (scroller?.scrollLeft ?? 0), t: m.items[0].t, lines };
  }
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

  function markClick(e: MouseEvent, m: Mark) {
    if (m.items.length === 1) return chipClick(e, m.items[0]);
    e.stopPropagation();
    // Zoom until the group's marks have room, keeping it in view.
    const a = m.items[0].t;
    const b = Math.max(...m.items.map((ev) => ev.t + (ev.span ?? 0)));
    const need = (m.items.length * (CHIP + GAP) * 2) / Math.max(1, w(b - a) + CHIP);
    zoom = Math.max(1, Math.min(48, zoom * Math.max(1.5, need)));
    requestAnimationFrame(() => {
      if (scroller) scroller.scrollLeft = x((a + b) / 2) - viewW / 2;
    });
    hover = null;
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
      style="width:{innerW}px;--lanes:{lanesH}px"
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
      <div class="lanes">
        {#each rows as lane, i (lane)}
          <span class="row-band" class:odd={i % 2 === 1} style="top:{i * ROW_H + 3}px" aria-hidden="true"></span>
        {/each}
        {#each marks as mk (mk.lane + ':' + mk.items[0].t + mk.items[0].kind)}
          {@const top = lead(mk)}
          {@const m = meta(top.kind)}
          {@const single = mk.items.length === 1}
          <button
            class="mark"
            class:group={!single}
            class:streak={single && top.kind === 'multikill'}
            style="left:{markLeft(mk)}px;top:{rowOf(mk.lane) * ROW_H + 6}px;--w:{Math.max(0, mk.end - mk.px)}px;--c:{m.color}"
            onpointerdown={(e) => e.stopPropagation()}
            onclick={(e) => markClick(e, mk)}
            onpointerenter={() => hoverMark(mk)}
            aria-label={single ? `${eventLabel(top)} ${clock(top.t)}` : t.timeline.group(mk.items.length, clock(mk.items[0].t))}
          >
            {#if single}
              <Icon name={m.icon} size={14} />
              {#if top.kind === 'multikill' && top.multi}<span class="streak-name">{t.timeline.streak[top.multi]}</span>{/if}
            {:else}
              <span class="icons">
                {#each kindsOf(mk) as k (k)}<span style="color:{meta(k).color}"><Icon name={meta(k).icon} size={12} /></span>{/each}
              </span>
              <span class="count num">{mk.items.length}</span>
            {/if}
          </button>
        {/each}
      </div>
      {#if showApm && apmSeries.length}
        <svg class="apm-g" viewBox="0 0 {(apmSeries.length / d) * 1000} 40" preserveAspectRatio="none" style="left:{PAD}px;width:{w(apmSeries.length)}px" aria-hidden="true">
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
        <span class="sel" style="left:{x(r.a)}px;width:{w(r.b - r.a)}px"></span>
      {/each}
      {#if mode === 'trim'}
        <span class="shade" style="left:0;width:{x(trim.a)}px"></span>
        <span class="shade" style="left:{x(trim.b)}px;right:0"></span>
        <span class="trim" style="left:{x(trim.a)}px;width:{w(trim.b - trim.a)}px">
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
      {#if hover.lines?.length === 1}
        <span class="h-label" style="color:{hover.lines[0].color}">{hover.lines[0].label}</span>
        <span class="h-time num">{clock(hover.t)}</span>
      {:else if hover.lines}
        <ul class="h-list">
          {#each hover.lines.slice(0, HOVER_LINES) as l, i (i)}
            <li><span class="h-time num">{clock(l.t)}</span><span class="h-label" style="color:{l.color}">{l.label}</span></li>
          {/each}
        </ul>
        {#if hover.lines.length > HOVER_LINES}<span class="h-time">{t.timeline.more(hover.lines.length - HOVER_LINES)}</span>{/if}
        <span class="h-hint">{t.timeline.zoomHint}</span>
      {:else}
        <span class="h-time num">{clock(hover.t)}</span>
      {/if}
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
    height: calc(var(--lanes) + 30px);
    cursor: pointer;
    min-width: 100%;
  }
  .apm .inner {
    height: calc(var(--lanes) + 72px);
  }
  .lanes {
    position: absolute;
    left: 0;
    right: 0;
    top: 0;
    height: var(--lanes);
  }
  .row-band {
    position: absolute;
    left: 0;
    right: 0;
    height: 28px;
    pointer-events: none;
  }
  .row-band.odd {
    background: color-mix(in oklab, var(--text) 3%, transparent);
  }
  /* A moment: an icon chip. A multikill stretches from its first kill to its last; a group of
     moments too close to tell apart shows their icons and a count. */
  .mark {
    position: absolute;
    min-width: 22px;
    width: calc(var(--w) + 22px);
    height: 22px;
    margin-left: -11px;
    padding: 0 4px;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 3px;
    border-radius: var(--r-sm);
    background: var(--panel-2);
    color: var(--c);
    box-shadow: inset 0 0 0 1px color-mix(in oklab, var(--c) 45%, transparent);
    transition: transform 100ms var(--ease-out), background 100ms;
    z-index: 2;
  }
  .mark.streak {
    justify-content: flex-start;
    gap: 4px;
    padding: 0 7px 0 4px;
    min-width: 74px;
    background: color-mix(in oklab, var(--c) 16%, var(--panel-2));
    box-shadow: inset 0 0 0 1px var(--c);
  }
  /* Several moments in one mark: drawn as a small stack so it never reads as a multikill. */
  .mark.group {
    width: auto;
    min-width: calc(var(--w) + 22px);
    justify-content: flex-start;
    box-shadow:
      inset 0 0 0 1px var(--seam-strong),
      2px 2px 0 -1px var(--panel-3),
      2px 2px 0 0 var(--seam-strong);
    color: var(--text-2);
    z-index: 3;
  }
  .streak-name {
    font-size: 11.5px;
    font-weight: 700;
    white-space: nowrap;
  }
  .mark:hover {
    background: color-mix(in oklab, var(--c) 22%, var(--panel-2));
    transform: translateY(-1px);
    z-index: 4;
  }
  .mark.group:hover {
    background: var(--panel-3);
  }
  .mark :global(svg) {
    flex: none;
  }
  .icons {
    display: flex;
    gap: 1px;
  }
  .icons span {
    display: grid;
  }
  .count {
    font-size: 11px;
    font-weight: 600;
    line-height: 1;
  }
  .apm-g {
    position: absolute;
    top: calc(var(--lanes) + 2px);
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
    top: var(--lanes);
    /* Clear of the playhead when it sits at the start. */
    left: 22px;
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
  .h-list {
    display: grid;
    gap: 3px;
    justify-self: stretch;
    list-style: none;
    margin: 0;
    padding: 0 2px;
  }
  .h-list li {
    display: flex;
    gap: 8px;
    align-items: baseline;
  }
  .h-hint {
    font-size: 11px;
    color: var(--text-3);
  }
</style>
