<script lang="ts">
  import CpuAssistToggle from './CpuAssistToggle.svelte';
  import Icon from './Icon.svelte';
  import { api } from '../lib/api';
  import { t } from '../lib/i18n.svelte';
  import { app, errorText, toast } from '../lib/state.svelte';
  import type { Job } from '../lib/types';

  let { job }: { job: Job } = $props();

  // Time left from the progress rate while running (pauses and games don't count).
  let samples: { t: number; p: number }[] = [];
  let lastPaused = false;
  const eta = $derived.by(() => {
    const now = Date.now();
    if (job.paused || app.status.state === 'recording') {
      samples = [];
      lastPaused = true;
      return null;
    }
    if (lastPaused) {
      samples = [];
      lastPaused = false;
    }
    samples.push({ t: now, p: job.progress });
    samples = samples.filter((s) => now - s.t < 10 * 60_000);
    const first = samples[0];
    const dp = job.progress - first.p;
    const dt = (now - first.t) / 1000;
    if (dt < 20 || dp <= 0) return null;
    return ((1 - job.progress) / dp) * dt;
  });

  function etaText(sec: number): string {
    const h = Math.floor(sec / 3600);
    const m = Math.round((sec % 3600) / 60);
    return h > 0 ? t.importProgress.etaHours(h, m) : t.importProgress.etaMinutes(Math.max(1, m));
  }

  const waitingForGame = $derived(app.status.state === 'recording');
  const statusLine = $derived(job.paused ? t.importProgress.paused : waitingForGame ? t.importProgress.waiting : (job.message ?? t.importProgress.starting));

  async function setPaused(p: boolean) {
    try {
      await api.pauseJob(job.id, p);
    } catch (e) {
      toast(errorText(e), 'error');
    }
  }
  let confirmCancel = $state(false);
</script>

<section class="ip" class:paused={job.paused} aria-label={t.importProgress.progress(job.title)}>
  <div class="top">
    <span class="title">{job.title}</span>
    <span class="pct num">{(job.progress * 100).toFixed(1)}%</span>
  </div>
  <div class="bar" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(job.progress * 100)}>
    <span style="width:{job.progress * 100}%"></span>
  </div>
  <div class="bottom">
    <span class="state">{statusLine}{#if eta && !job.paused}<span class="dim">{` · ${etaText(eta)}`}</span>{/if}</span>
    <div class="actions">
      {#if confirmCancel}
        <span class="dim">{t.importProgress.stopAsk}</span>
        <button class="btn danger sm" onclick={() => api.cancelJob(job.id)}>{t.importProgress.stopImport}</button>
        <button class="btn ghost sm" onclick={() => (confirmCancel = false)}>{t.importProgress.keepGoing}</button>
      {:else}
        {#if job.pausable}
          {#if job.paused}
            <button class="btn primary sm" onclick={() => setPaused(false)}><Icon name="play" size={14} />{t.importProgress.resume}</button>
          {:else}
            <button class="btn sm" onclick={() => setPaused(true)}><Icon name="pause" size={14} />{t.importProgress.pause}</button>
          {/if}
        {/if}
        <button class="btn ghost sm" onclick={() => (confirmCancel = true)}>{t.importProgress.stop}</button>
      {/if}
    </div>
  </div>
  {#if job.tag === 'outplayed'}
    <div class="assist"><CpuAssistToggle /></div>
  {/if}
</section>

<style>
  .assist {
    margin-top: 2px;
    padding-top: 10px;
    border-top: 1px solid var(--seam);
  }
  .ip {
    display: grid;
    gap: 8px;
    padding: 12px 16px;
    background: var(--panel);
    border: 1px solid var(--seam);
    border-radius: var(--r-lg);
  }
  .top,
  .bottom {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }
  .title {
    font-family: var(--font-score);
    font-weight: 600;
    font-size: 14px;
  }
  .pct {
    font-size: 16px;
    color: var(--you);
  }
  .paused .pct {
    color: var(--warn);
  }
  .bar {
    height: 8px;
    border-radius: 4px;
    background: var(--panel-3);
    overflow: hidden;
  }
  .bar span {
    display: block;
    height: 100%;
    background: var(--you);
    transition: width 400ms var(--ease-out);
  }
  .paused .bar span {
    background: var(--warn);
  }
  .state {
    font-size: 12.5px;
    color: var(--text-2);
  }
  .dim {
    color: var(--text-3);
    font-size: 12.5px;
  }
  .actions {
    display: flex;
    align-items: center;
    gap: 6px;
  }
</style>
