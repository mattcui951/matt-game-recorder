<script lang="ts">
  import Icon from './Icon.svelte';
  import Slider from './Slider.svelte';
  import SupercutKinds from './SupercutKinds.svelte';
  import { api } from '../lib/api';
  import { MULTIKILL_TIERS, supercutCounts, supercutKey } from '../lib/events';
  import { clock } from '../lib/format.svelte';
  import { t } from '../lib/i18n.svelte';
  import { app, errorText, go, toast, ui } from '../lib/state.svelte';
  import type { Recording } from '../lib/types';

  // A supercut across every match in a date range: pick the dates and the kinds of moment
  // (kill tiers too), and they are cut into one video, oldest match first.
  let { onstarted }: { onstarted?: () => void } = $props();

  type Preset = 'today' | 'week' | 'month' | 'last30' | 'all' | 'custom';
  const pad = (n: number) => String(n).padStart(2, '0');
  const ymd = (d: Date) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
  const today = new Date();

  function presetRange(p: Preset): [string, string] {
    const end = ymd(today);
    if (p === 'today') return [end, end];
    if (p === 'week') {
      const d = new Date(today);
      d.setDate(d.getDate() - ((d.getDay() + 6) % 7)); // Monday
      return [ymd(d), end];
    }
    if (p === 'month') return [ymd(new Date(today.getFullYear(), today.getMonth(), 1)), end];
    if (p === 'last30') {
      const d = new Date(today);
      d.setDate(d.getDate() - 29);
      return [ymd(d), end];
    }
    const first = app.library.sessions.reduce((a, s) => (s.startedAt < a ? s.startedAt : a), today.toISOString());
    return [ymd(new Date(first)), end];
  }

  // Start from the period picked in the library's date bar, when there is one.
  function initial(): { preset: Preset; from: string; to: string } {
    const { libraryYear: y, libraryMonth: m, libraryDay: d } = ui;
    if (m !== null) {
      const year = y ?? today.getFullYear();
      if (d !== null) return { preset: 'custom', from: ymd(new Date(year, m, d)), to: ymd(new Date(year, m, d)) };
      return { preset: 'custom', from: ymd(new Date(year, m, 1)), to: ymd(new Date(year, m + 1, 0)) };
    }
    if (y !== null) return { preset: 'custom', from: `${y}-01-01`, to: `${y}-12-31` };
    const [from, to] = presetRange('month');
    return { preset: 'month', from, to };
  }
  const start = initial();
  let preset = $state<Preset>(start.preset);
  let from = $state(start.from);
  let to = $state(start.to);
  function pick(p: Preset) {
    preset = p;
    if (p !== 'custom') [from, to] = presetRange(p);
  }

  // The recordings the backend will use (see supercut::sources): per match, the full recording,
  // else its highlight and manual clips.
  const sources = $derived.by(() => {
    const [a, b] = from <= to ? [from, to] : [to, from];
    const out: Recording[] = [];
    const sessions = app.library.sessions.filter((s) => {
      const day = ymd(new Date(s.startedAt));
      return day >= a && day <= b;
    });
    for (const s of [...sessions].sort((p, q) => p.startedAt.localeCompare(q.startedAt))) {
      const recs = app.library.recordings.filter((r) => r.sessionId === s.id && !r.deletedAt);
      const full = recs.filter((r) => r.kind === 'match' || r.kind === 'session');
      out.push(...(full.length ? full : recs.filter((r) => r.kind === 'highlight' || r.kind === 'manual')));
    }
    return out;
  });
  const counts = $derived(supercutCounts(sources));

  // Streaks by default (doubles and up); single kills when a range has no streak at all.
  let kinds = $state<string[]>([]);
  let touched = false;
  $effect(() => {
    if (touched) return;
    const tiers = MULTIKILL_TIERS.filter((k) => counts[k]);
    kinds = tiers.length ? tiers : counts.kill ? ['kill'] : [];
  });
  let before = $state(5);
  let after = $state(2);

  const picked = $derived(sources.flatMap((r) => r.events.filter((e) => kinds.includes(supercutKey(e))).map((e) => ({ r, e }))));
  const matches = $derived(new Set(picked.map((p) => p.r.sessionId)).size);
  const length = $derived(picked.reduce((a, { e }) => a + before + (e.span ?? 0) + after, 0));

  let busy = $state(false);
  async function create() {
    busy = true;
    try {
      const [a, b] = from <= to ? [from, to] : [to, from];
      const job = await api.supercutRange(a, b, kinds, before, after);
      toast(t.session.buildingSupercut, 'info');
      onstarted?.();
      const iv = setInterval(() => {
        const j = app.jobs[job];
        if (j && j.state === 'done' && j.resultId) {
          clearInterval(iv);
          const id = j.resultId;
          toast(t.common.ready(j.title), 'ok', {
            label: t.common.open,
            run: () => {
              const r = app.library.recordings.find((x) => x.id === id);
              if (r) go({ name: 'session', sessionId: r.sessionId, recordingId: r.id });
            },
          });
        } else if (j && j.state !== 'running') clearInterval(iv);
      }, 400);
    } catch (e) {
      toast(errorText(e), 'error');
    } finally {
      busy = false;
    }
  }

  const PRESETS: Preset[] = ['today', 'week', 'month', 'last30', 'all', 'custom'];
</script>

<div class="rs">
  <div class="title">{t.supercut.title}</div>
  <div class="presets" role="group" aria-label={t.supercut.dates}>
    {#each PRESETS as p (p)}
      <button class="preset" class:on={preset === p} aria-pressed={preset === p} onclick={() => pick(p)}>{t.supercut.presets[p]}</button>
    {/each}
  </div>
  <div class="dates">
    <label><span>{t.supercut.from}</span><input class="field num" type="date" bind:value={from} max={to} onchange={() => (preset = 'custom')} /></label>
    <label><span>{t.supercut.to}</span><input class="field num" type="date" bind:value={to} min={from} onchange={() => (preset = 'custom')} /></label>
  </div>

  {#if Object.keys(counts).length}
    <div class="sub">{t.supercut.kinds}</div>
    <SupercutKinds {counts} bind:selected={() => kinds, (v) => ((kinds = v), (touched = true))} />
    <div class="sub">{t.session.clipBuffer}</div>
    <label class="row"><span>{t.session.before}</span><Slider value={before} min={0} max={20} onchange={(v) => (before = v)} label={t.session.secondsBefore} suffix={t.common.seconds} /></label>
    <label class="row"><span>{t.session.after}</span><Slider value={after} min={0} max={20} onchange={(v) => (after = v)} label={t.session.secondsAfter} suffix={t.common.seconds} /></label>
    <p class="summary">
      {#if picked.length}{t.supercut.summary(picked.length, matches, clock(length))}{:else}{t.supercut.pickKinds}{/if}
    </p>
    <button class="btn primary" disabled={!picked.length || busy} onclick={create}><Icon name="bolt" size={15} />{t.session.createSupercut}</button>
  {:else}
    <p class="summary">{t.supercut.nothing}</p>
  {/if}
</div>

<style>
  .rs {
    display: grid;
    gap: 8px;
  }
  .title {
    font-family: var(--font-score);
    font-weight: 600;
    font-size: 14px;
  }
  .presets {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .preset {
    height: 26px;
    padding: 0 10px;
    border-radius: 13px;
    font-size: 12.5px;
    color: var(--text-2);
    box-shadow: inset 0 0 0 1px var(--seam-strong);
  }
  .preset:hover {
    color: var(--text);
    background: var(--panel-2);
  }
  .preset.on {
    color: var(--text);
    background: var(--panel-3);
    box-shadow: inset 0 0 0 1px var(--you);
  }
  .dates {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 8px;
  }
  .dates label {
    display: grid;
    gap: 3px;
    font-size: 12px;
    color: var(--text-3);
  }
  .dates input {
    width: 100%;
    min-width: 0;
    color-scheme: dark light;
  }
  .sub {
    margin-top: 4px;
    font-size: 12.5px;
    color: var(--text-3);
  }
  .row {
    display: grid;
    grid-template-columns: 50px 1fr;
    align-items: center;
    font-size: 12.5px;
    color: var(--text-2);
  }
  .summary {
    margin: 2px 0 0;
    font-size: 12.5px;
    color: var(--text-2);
  }
</style>
