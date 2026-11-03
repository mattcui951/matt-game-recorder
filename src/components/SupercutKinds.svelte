<script lang="ts">
  import Icon from './Icon.svelte';
  import { SUPERCUT_KINDS, supercutMeta } from '../lib/events';

  // Which kinds of moment go into a supercut: one row per kind that has moments, with its count.
  // Kill tiers (Penta ... Double, Single kills) are separate rows, so "triples and up" is easy.
  let { counts, selected = $bindable([] as string[]) }: { counts: Record<string, number>; selected?: string[] } = $props();

  const kinds = $derived(SUPERCUT_KINDS.filter((k) => counts[k]));
  const toggle = (k: string) => (selected = selected.includes(k) ? selected.filter((x) => x !== k) : [...selected, k]);
</script>

<div class="kinds" role="group">
  {#each kinds as k (k)}
    {@const m = supercutMeta(k)}
    <button class="kind" class:on={selected.includes(k)} style="--c:{m.color}" aria-pressed={selected.includes(k)} onclick={() => toggle(k)}>
      <Icon name={m.icon} size={15} /><span>{m.label}</span>
      <span class="num cnt">{counts[k]}</span>
      {#if selected.includes(k)}<Icon name="check" size={14} />{:else}<span></span>{/if}
    </button>
  {/each}
</div>

<style>
  .kinds {
    display: grid;
    gap: 4px;
  }
  .kind {
    display: grid;
    grid-template-columns: 18px 1fr auto 16px;
    align-items: center;
    gap: 8px;
    height: 32px;
    padding: 0 10px;
    border-radius: var(--r-md);
    background: var(--panel-2);
    color: var(--text-2);
    text-align: left;
  }
  .kind > :global(svg:first-child) {
    color: var(--c);
  }
  .kind:hover {
    color: var(--text);
  }
  .kind.on {
    color: var(--text);
    box-shadow: inset 0 0 0 1px color-mix(in oklab, var(--c) 60%, transparent);
  }
  .cnt {
    font-size: 12px;
    color: var(--text-3);
  }
</style>
