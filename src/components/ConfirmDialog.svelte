<script lang="ts">
  // A modal yes/no question: title, explanation, a quieter note, then cancel and confirm.
  let {
    title,
    body,
    note = '',
    cancel,
    confirm,
    danger = false,
    onconfirm,
  }: { title: string; body: string; note?: string; cancel: string; confirm: string; danger?: boolean; onconfirm: () => void } = $props();

  const id = $props.id();
  let dialog = $state<HTMLDialogElement>();

  export function open() {
    dialog?.showModal();
  }
  function yes() {
    dialog?.close();
    onconfirm();
  }
</script>

<dialog bind:this={dialog} aria-labelledby={id}>
  <h2 {id}>{title}</h2>
  <p>{body}</p>
  {#if note}<p class="dim">{note}</p>{/if}
  <div class="btns">
    <button class="btn ghost sm" onclick={() => dialog?.close()}>{cancel}</button>
    <button class="btn sm {danger ? 'danger solid' : 'primary'}" onclick={yes}>{confirm}</button>
  </div>
</dialog>

<style>
  dialog {
    width: min(420px, calc(100vw - 32px));
    padding: 20px 22px 18px;
    background: var(--panel);
    color: var(--text);
    border: 1px solid var(--seam-strong);
    border-radius: var(--r-lg);
    box-shadow: var(--shadow-pop);
    outline: none;
  }
  dialog::backdrop {
    background: var(--backdrop);
  }
  dialog[open] {
    animation: rise 180ms var(--ease-out);
  }
  @keyframes rise {
    from {
      opacity: 0;
      transform: translateY(6px);
    }
  }
  h2 {
    margin: 0 0 10px;
    font-family: var(--font-score);
    font-weight: 600;
    font-size: 16px;
  }
  p {
    margin: 0 0 8px;
    font-size: 13px;
    line-height: 1.5;
    color: var(--text-2);
    word-break: break-word;
  }
  p.dim {
    font-size: 12.5px;
    color: var(--text-3);
  }
  .btns {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 16px;
  }
</style>
