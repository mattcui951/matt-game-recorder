<script lang="ts" generics="T extends string | number">
  import Icon from './Icon.svelte';

  // An option with an icon keeps its label for screen readers when a narrow parent hides the
  // visible label (`.seg-lbl`).
  let {
    value,
    options,
    onchange,
    label,
    disabled = false,
  }: { value: T; options: { value: T; label: string; hint?: string; icon?: string }[]; onchange: (v: T) => void; label: string; disabled?: boolean } = $props();
</script>

<div class="seg" role="radiogroup" aria-label={label} class:disabled>
  {#each options as o (o.value)}
    <button role="radio" aria-checked={value === o.value} aria-label={o.icon ? o.label : undefined} class:on={value === o.value} {disabled} title={o.hint} onclick={() => onchange(o.value)}>
      {#if o.icon}<Icon name={o.icon} size={15} /><span class="seg-lbl">{o.label}</span>{:else}{o.label}{/if}
    </button>
  {/each}
</div>

<style>
  .seg {
    display: flex;
    background: var(--bg-sunk);
    border: 1px solid var(--seam);
    border-radius: var(--r-md);
    padding: 2px;
    gap: 2px;
  }
  .seg.disabled {
    opacity: 0.45;
  }
  button {
    flex: 1;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    height: 26px;
    padding: 0 10px;
    border-radius: var(--r-sm);
    color: var(--text-2);
    font-weight: 600;
    font-size: 12.5px;
    white-space: nowrap;
    transition: background 120ms, color 120ms;
  }
  button:hover:not(:disabled):not(.on) {
    color: var(--text);
    background: var(--panel-2);
  }
  button.on {
    background: var(--panel-3);
    color: var(--you);
    box-shadow: inset 0 -2px 0 var(--you);
  }
</style>
