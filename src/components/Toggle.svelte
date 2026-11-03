<script lang="ts">
  let { checked, onchange, disabled = false, label }: { checked: boolean; onchange: (v: boolean) => void; disabled?: boolean; label: string } = $props();
</script>

<button class="toggle" class:on={checked} role="switch" aria-checked={checked} aria-label={label} {disabled} onclick={() => onchange(!checked)}>
  <span class="knob"></span>
</button>

<style>
  .toggle {
    position: relative;
    width: 34px;
    height: 18px;
    border-radius: 9px;
    background: var(--panel-3);
    box-shadow: inset 0 0 0 1px var(--seam-strong);
    transition: background 160ms var(--ease-out);
    flex: none;
  }
  .toggle.on {
    background: var(--you);
    box-shadow: none;
  }
  .toggle:disabled {
    opacity: 0.4;
  }
  .knob {
    position: absolute;
    top: 3px;
    left: 3px;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: var(--text-2);
    transition: transform 160ms var(--ease-out), background 160ms;
  }
  .on .knob {
    transform: translateX(16px);
    background: var(--you-ink);
  }
</style>
