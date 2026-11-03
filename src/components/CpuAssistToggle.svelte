<script lang="ts">
  import Toggle from './Toggle.svelte';
  import { api } from '../lib/api';
  import { t } from '../lib/i18n.svelte';
  import { app, errorText, toast } from '../lib/state.svelte';

  // "Use the CPU as a second encoder" for Outplayed imports. Both directions ask first.
  let dialog = $state<HTMLDialogElement>();
  let asking = $state<'on' | 'off' | null>(null);

  function ask(on: boolean) {
    asking = on ? 'on' : 'off';
    dialog?.showModal();
  }
  // Measured on this PC (a short test encode, then the real import), never a fixed figure.
  const times = (x: number) => (Math.round(x * 10) / 10).toFixed(1);
  const gain = $derived.by(() => {
    const x = app.cpuAssist.speedup;
    if (x) return x < 1.15 ? t.cpu.slightly(times(x)) : t.cpu.asFast(times(x));
    return app.cpuAssist.measuring ? t.cpu.measuring : t.cpu.faster;
  });

  async function confirm() {
    const on = asking === 'on';
    dialog?.close();
    try {
      app.cpuAssist = await api.setImportCpuAssist(on);
    } catch (e) {
      toast(errorText(e), 'error');
    }
  }
</script>

{#if app.cpuAssist.available}
  <div class="cpu" class:on={app.cpuAssist.on}>
    <Toggle checked={app.cpuAssist.on} label={t.cpu.label} onchange={ask} />
    <span class="txt">
      <span class="name">{t.cpu.label}</span>
      <span class="hint">{app.cpuAssist.on ? t.cpu.onHint : t.cpu.offHint(gain)}</span>
    </span>
  </div>
{/if}

<dialog bind:this={dialog} onclose={() => (asking = null)} aria-labelledby="cpu-q">
  {#if asking === 'on'}
    <h2 id="cpu-q">{t.cpu.askOn}</h2>
    <p>{app.cpuAssist.speedup ? t.cpu.askOnBodyX(times(app.cpuAssist.speedup)) : t.cpu.askOnBody}</p>
    <p class="dim">{t.cpu.askOnNote}</p>
    <div class="btns">
      <button class="btn ghost sm" onclick={() => dialog?.close()}>{t.common.cancel}</button>
      <button class="btn primary sm" onclick={confirm}>{t.cpu.yesOn}</button>
    </div>
  {:else if asking === 'off'}
    <h2 id="cpu-q">{t.cpu.askOff}</h2>
    <p>{t.cpu.askOffBody}</p>
    <p class="dim">{t.cpu.askOffNote}</p>
    <div class="btns">
      <button class="btn ghost sm" onclick={() => dialog?.close()}>{t.cpu.keepOn}</button>
      <button class="btn primary sm" onclick={confirm}>{t.cpu.yesOff}</button>
    </div>
  {/if}
</dialog>

<style>
  .cpu {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .txt {
    display: grid;
    gap: 1px;
    min-width: 0;
  }
  .name {
    font-size: 12.5px;
    color: var(--text);
  }
  .hint {
    font-size: 12px;
    color: var(--text-3);
  }
  .cpu.on .hint {
    color: var(--warn);
  }
  dialog {
    width: min(420px, calc(100vw - 32px));
    padding: 20px 22px 18px;
    background: var(--panel);
    color: var(--text);
    border: 1px solid var(--seam-strong);
    border-radius: var(--r-lg);
    box-shadow: var(--shadow-pop);
    outline: none;
  }
  dialog::backdrop {
    background: var(--backdrop);
  }
  dialog[open] {
    animation: rise 180ms var(--ease-out);
  }
  @keyframes rise {
    from {
      opacity: 0;
      transform: translateY(6px);
    }
  }
  h2 {
    margin: 0 0 10px;
    font-family: var(--font-score);
    font-weight: 600;
    font-size: 16px;
  }
  p {
    margin: 0 0 8px;
    font-size: 13px;
    line-height: 1.5;
    color: var(--text-2);
  }
  p.dim {
    font-size: 12.5px;
    color: var(--text-3);
  }
  .btns {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 16px;
  }
</style>
