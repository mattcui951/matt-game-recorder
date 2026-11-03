<script lang="ts">
  import type { Snippet } from 'svelte';
  let {
    open = $bindable(false),
    anchor,
    children,
    align = 'right',
    placement = 'below',
    width = 280,
  }: { open?: boolean; anchor: Snippet<[() => void]>; children: Snippet<[() => void]>; align?: 'left' | 'right'; placement?: 'below' | 'above'; width?: number } = $props();

  let root: HTMLDivElement;
  const toggle = () => (open = !open);
  const close = () => (open = false);

  function onWindowDown(e: PointerEvent) {
    if (open && root && !root.contains(e.target as Node)) open = false;
  }
  function onKey(e: KeyboardEvent) {
    if (open && e.key === 'Escape') {
      e.stopPropagation();
      open = false;
    }
  }
</script>

<svelte:window onpointerdown={onWindowDown} onkeydown={onKey} />

<div class="pop-root" bind:this={root}>
  {@render anchor(toggle)}
  {#if open}
    <div class="pop {align} {placement}" style="width:{width}px" role="dialog">
      {@render children(close)}
    </div>
  {/if}
</div>

<style>
  .pop-root {
    position: relative;
    display: inline-flex;
  }
  .pop {
    position: absolute;
    z-index: 50;
    background: var(--panel);
    border: 1px solid var(--seam-strong);
    border-radius: var(--r-lg);
    box-shadow: var(--shadow-pop);
    padding: 12px;
    animation: pop-in 160ms var(--ease-out);
  }
  .below {
    top: calc(100% + 8px);
  }
  .above {
    bottom: calc(100% + 8px);
  }
  .right {
    right: 0;
  }
  .left {
    left: 0;
  }
  @keyframes pop-in {
    from {
      opacity: 0;
      transform: translateY(-4px);
    }
  }
</style>
