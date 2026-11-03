<script lang="ts">
  let {
    value,
    min = 0,
    max = 100,
    step = 1,
    onchange,
    label,
    suffix = '',
    disabled = false,
  }: { value: number; min?: number; max?: number; step?: number; onchange: (v: number) => void; label: string; suffix?: string; disabled?: boolean } = $props();
  const pct = $derived(((value - min) / (max - min)) * 100);
</script>

<div class="slider" class:disabled>
  <input type="range" aria-label={label} {min} {max} {step} {value} {disabled} style="--p:{pct}%" oninput={(e) => onchange(Number((e.currentTarget as HTMLInputElement).value))} />
  <span class="num val">{value}{suffix}</span>
</div>

<style>
  .slider {
    display: flex;
    align-items: center;
    gap: 12px;
    min-width: 0;
  }
  .slider.disabled {
    opacity: 0.45;
  }
  input {
    flex: 1;
    appearance: none;
    height: 18px;
    background: transparent;
    cursor: pointer;
    min-width: 0;
  }
  input::-webkit-slider-runnable-track {
    height: 4px;
    border-radius: 2px;
    background: linear-gradient(to right, var(--you) var(--p), var(--panel-3) var(--p));
  }
  input::-webkit-slider-thumb {
    appearance: none;
    width: 14px;
    height: 14px;
    margin-top: -5px;
    border-radius: 50%;
    background: var(--text);
    box-shadow: 0 1px 3px rgb(0 0 0 / 0.5);
  }
  .val {
    min-width: 44px;
    text-align: right;
    color: var(--text-2);
    font-size: 13px;
  }
</style>
