<script lang="ts">
  import Icon from './Icon.svelte';
  import { api } from '../lib/api';
  import { t } from '../lib/i18n.svelte';
  import { app } from '../lib/state.svelte';

  // Exports and imports have their own progress panels.
  const running = $derived(Object.values(app.jobs).filter((j) => j.kind !== 'export' && j.kind !== 'import'));
</script>

<div class="stack" aria-live="polite">
  {#each running as j (j.id)}
    <div class="toast job">
      <div class="j-top">
        <span class="j-title">{j.title}</span>
        {#if j.state === 'running'}
          <span class="num pct">{Math.round(j.progress * 100)}%</span>
          <button class="icon-btn sm" onclick={() => api.cancelJob(j.id)} aria-label={t.common.cancel}><Icon name="close" size={14} /></button>
        {:else}
          <span class="state {j.state}">{j.state === 'done' ? t.common.done : j.state === 'error' ? t.common.failed : t.common.cancelled}</span>
        {/if}
      </div>
      <div class="bar"><span style="width:{Math.round(j.progress * 100)}%"></span></div>
      {#if j.message}<span class="j-msg">{j.message}</span>{/if}
    </div>
  {/each}
  {#each app.toasts as toast (toast.id)}
    <div class="toast {toast.tone}" role="status">
      <span class="txt">{toast.text}</span>
      {#if toast.action}
        <button class="act" onclick={() => { toast.action?.run(); app.toasts = app.toasts.filter((x) => x.id !== toast.id); }}>{toast.action.label}</button>
      {/if}
    </div>
  {/each}
</div>

<style>
  .stack {
    position: fixed;
    right: 20px;
    bottom: 20px;
    display: grid;
    gap: 8px;
    z-index: 100;
    width: 340px;
    pointer-events: none;
  }
  .toast {
    pointer-events: auto;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 14px;
    background: var(--panel);
    border: 1px solid var(--seam-strong);
    border-radius: var(--r-lg);
    box-shadow: var(--shadow-pop);
    animation: rise 220ms var(--ease-out);
  }
  .toast.error {
    border-color: color-mix(in oklab, var(--death) 60%, transparent);
  }
  .toast.ok {
    border-color: color-mix(in oklab, var(--win) 50%, transparent);
  }
  .txt {
    flex: 1;
    font-size: 13px;
    user-select: text;
  }
  .act {
    color: var(--you);
    font-weight: 600;
  }
  .job {
    display: grid;
    gap: 8px;
  }
  .j-top {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .j-title {
    flex: 1;
    font-weight: 600;
    font-size: 13px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .pct {
    font-size: 13px;
    color: var(--text-2);
  }
  .icon-btn.sm {
    width: 24px;
    height: 24px;
  }
  .j-msg {
    font-size: 12px;
    color: var(--text-3);
  }
  .state.done {
    color: var(--win);
  }
  .state.error {
    color: var(--death);
  }
  .state.cancelled {
    color: var(--text-3);
  }
  .bar {
    height: 4px;
    border-radius: 2px;
    background: var(--panel-3);
    overflow: hidden;
  }
  .bar span {
    display: block;
    height: 100%;
    background: var(--you);
    transition: width 200ms;
  }
  @keyframes rise {
    from {
      opacity: 0;
      transform: translateY(8px);
    }
  }
</style>
