<script lang="ts">
  import ConfirmDialog from './ConfirmDialog.svelte';
  import Toggle from './Toggle.svelte';
  import { api } from '../lib/api';
  import { t } from '../lib/i18n.svelte';
  import { app, errorText, toast } from '../lib/state.svelte';

  // "Use the CPU as a second encoder" for Outplayed imports. Both directions ask first.
  let dialog = $state<ConfirmDialog>();
  let asking = $state<'on' | 'off'>('on');

  function ask(on: boolean) {
    asking = on ? 'on' : 'off';
    dialog?.open();
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

<ConfirmDialog
  bind:this={dialog}
  title={asking === 'on' ? t.cpu.askOn : t.cpu.askOff}
  body={asking === 'off' ? t.cpu.askOffBody : app.cpuAssist.speedup ? t.cpu.askOnBodyX(times(app.cpuAssist.speedup)) : t.cpu.askOnBody}
  note={asking === 'on' ? t.cpu.askOnNote : t.cpu.askOffNote}
  cancel={asking === 'on' ? t.common.cancel : t.cpu.keepOn}
  confirm={asking === 'on' ? t.cpu.yesOn : t.cpu.yesOff}
  onconfirm={confirm}
/>

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
</style>
