<script lang="ts" generics="T extends string | number">
  import Icon from './Icon.svelte';
  let {
    value,
    options,
    onchange,
    label,
    disabled = false,
  }: { value: T; options: { value: T; label: string; disabled?: boolean }[]; onchange: (v: T) => void; label: string; disabled?: boolean } = $props();
</script>

<div class="select" class:disabled>
  <select
    aria-label={label}
    {disabled}
    onchange={(e) => {
      const raw = (e.currentTarget as HTMLSelectElement).value;
      const opt = options.find((o) => String(o.value) === raw);
      if (opt) onchange(opt.value);
    }}
  >
    {#each options as o (o.value)}
      <option value={String(o.value)} selected={o.value === value} disabled={o.disabled}>{o.label}</option>
    {/each}
  </select>
  <span class="chev"><Icon name="chevron-down" size={14} /></span>
</div>

<style>
  .select {
    position: relative;
    min-width: 0;
  }
  .select.disabled {
    opacity: 0.45;
  }
  select {
    appearance: none;
    width: 100%;
    height: 32px;
    padding: 0 32px 0 10px;
    border-radius: var(--r-md);
    background: var(--bg-sunk);
    border: 1px solid var(--seam);
    outline: none;
    cursor: pointer;
    text-overflow: ellipsis;
  }
  select:focus-visible {
    border-color: var(--you);
  }
  option {
    background: var(--panel);
  }
  .chev {
    position: absolute;
    right: 10px;
    top: 9px;
    color: var(--text-3);
    pointer-events: none;
  }
</style>
