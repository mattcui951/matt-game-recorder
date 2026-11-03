<script lang="ts">
  import { t } from '../lib/i18n.svelte';

  let { value, onchange, label, single = false }: { value: string; onchange: (v: string) => void; label: string; single?: boolean } = $props();
  let listening = $state(false);

  const KEYMAP: Record<string, string> = { ' ': 'Space', ArrowUp: 'Up', ArrowDown: 'Down', ArrowLeft: 'Left', ArrowRight: 'Right', Escape: 'Esc' };

  function onKey(e: KeyboardEvent) {
    if (!listening) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.key === 'Escape') {
      listening = false;
      return;
    }
    if ((e.key === 'Backspace' || e.key === 'Delete') && !single) {
      onchange('');
      listening = false;
      return;
    }
    if (['Control', 'Shift', 'Alt', 'Meta'].includes(e.key)) return;
    let key = KEYMAP[e.key] ?? (e.key.length === 1 ? e.key.toUpperCase() : e.key);
    if (single) {
      onchange(key);
    } else {
      const mods = [e.ctrlKey && 'Ctrl', e.altKey && 'Alt', e.shiftKey && 'Shift'].filter(Boolean);
      onchange([...mods, key].join('+'));
    }
    listening = false;
  }
</script>

<button class="hk" class:listening aria-label={label} onclick={() => (listening = !listening)} onkeydown={onKey} onblur={() => (listening = false)}>
  {#if listening}
    <span class="hint">{single ? t.hotkey.press : t.hotkey.pressClear}</span>
  {:else if value}
    <span class="num">{value}</span>
  {:else}
    <span class="hint">{t.hotkey.notSet}</span>
  {/if}
</button>

<style>
  .hk {
    height: 32px;
    min-width: 140px;
    padding: 0 12px;
    border-radius: var(--r-md);
    background: var(--bg-sunk);
    border: 1px solid var(--seam);
    text-align: left;
    color: var(--text);
    font-size: 13.5px;
  }
  .hk.listening {
    border-color: var(--you);
  }
  .hint {
    color: var(--text-3);
  }
</style>
