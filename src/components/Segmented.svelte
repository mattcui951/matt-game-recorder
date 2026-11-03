<script lang="ts" generics="T extends string | number">
  let {
    value,
    options,
    onchange,
    label,
    disabled = false,
  }: { value: T; options: { value: T; label: string; hint?: string }[]; onchange: (v: T) => void; label: string; disabled?: boolean } = $props();
</script>

<div class="seg" role="radiogroup" aria-label={label} class:disabled>
  {#each options as o (o.value)}
    <button role="radio" aria-checked={value === o.value} class:on={value === o.value} {disabled} title={o.hint} onclick={() => onchange(o.value)}>{o.label}</button>
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
