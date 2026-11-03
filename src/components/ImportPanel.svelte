<script lang="ts">
  import { open as openDialog } from '@tauri-apps/plugin-dialog';
  import CpuAssistToggle from './CpuAssistToggle.svelte';
  import Icon from './Icon.svelte';
  import { api } from '../lib/api';
  import { app, errorText, toast } from '../lib/state.svelte';
  import { bytes } from '../lib/format.svelte';
  import { t } from '../lib/i18n.svelte';
  import type { OutplayedScan } from '../lib/types';

  let { onstarted }: { onstarted?: () => void } = $props();

  let scan = $state<OutplayedScan | null>(null);
  let scanning = $state(true);
  let folder = $state('');
  let includeDeleted = $state(false);
  // Delete each Outplayed original once imported: offered only when the drive is too full, and
  // only after the user confirms in the dialog.
  let deleteSources = $state(false);
  let confirmDialog = $state<HTMLDialogElement>();
  const running = $derived(Object.values(app.jobs).some((j) => j.kind === 'import' && j.state === 'running'));

  async function rescan(f?: string) {
    scanning = true;
    try {
      scan = await api.scanOutplayed(f);
      folder = scan.folder ?? '';
    } catch (e) {
      toast(errorText(e), 'error');
    } finally {
      scanning = false;
    }
  }
  rescan();

  async function pick(): Promise<string | null> {
    const r = await openDialog({ directory: true, multiple: false });
    return typeof r === 'string' ? r : null;
  }
  async function chooseOutplayed() {
    const f = await pick();
    if (f) rescan(f);
  }
  async function startOutplayed() {
    try {
      await api.importOutplayed(folder, includeDeleted, deleteSources && short);
      toast(t.importPanel.started, 'info');
      onstarted?.();
    } catch (e) {
      toast(errorText(e), 'error');
    }
  }
  async function startLibrary() {
    const f = await pick();
    if (!f) return;
    try {
      await api.importLibrary(f);
      toast(t.importPanel.copying, 'info');
      onstarted?.();
    } catch (e) {
      toast(errorText(e), 'error');
    }
  }
  const remaining = $derived(scan ? scan.files - scan.alreadyImported - (includeDeleted ? 0 : scan.deletedAfterImport) : 0);
  // AV1 at this app's settings came out ~43% smaller than Outplayed's H.264 in testing.
  const estimate = $derived(scan ? scan.bytes * 0.57 * (remaining / Math.max(1, scan.files)) : 0);
  // Room for the converted videos, plus the margin the import keeps free.
  const MARGIN = 2 * 1024 ** 3;
  const short = $derived(Boolean(scan && remaining > 0 && estimate + MARGIN > scan.freeBytes));
  function toggleDelete(e: Event) {
    const box = e.currentTarget as HTMLInputElement;
    if (box.checked) {
      // Not on until confirmed.
      box.checked = false;
      confirmDialog?.showModal();
    } else {
      deleteSources = false;
    }
  }
</script>

<div class="imp">
  <section>
    <div class="head"><span class="title">{t.importPanel.fromOutplayed}</span><button class="link" onclick={chooseOutplayed}>{t.importPanel.chooseFolder}</button></div>
    {#if scanning}
      <p class="dim">{t.importPanel.looking}</p>
    {:else if !folder}
      <p class="dim">{t.importPanel.notFound}</p>
    {:else}
      <p class="path" title={folder}>{folder}</p>
      <dl>
        <dt>{t.importPanel.videos}</dt><dd class="num">{scan?.files ?? 0} <span class="dim">({bytes(scan?.bytes ?? 0)})</span></dd>
        <dt>{t.importPanel.withEvents}</dt><dd class="num">{scan?.withEvents ?? 0}</dd>
        <dt>{t.importPanel.alreadyImported}</dt><dd class="num">{scan?.alreadyImported ?? 0}</dd>
        {#if scan?.deletedAfterImport}<dt>{t.importPanel.deletedAfter}</dt><dd class="num">{scan.deletedAfterImport}</dd>{/if}
        {#if remaining > 0}<dt>{t.importPanel.newSize}</dt><dd class="num">{t.importPanel.about(bytes(estimate))}</dd>{/if}
      </dl>
      {#if scan?.deletedAfterImport}
        <label class="opt">
          <input type="checkbox" bind:checked={includeDeleted} />
          <span>{t.importPanel.bringBack(scan.deletedAfterImport)}</span>
        </label>
      {/if}
      {#if short && scan}
        <div class="warn" role="alert">
          <strong>{t.importPanel.lowSpace}</strong>
          <span>{t.importPanel.lowSpaceText(bytes(estimate), bytes(scan.freeBytes))}</span>
          {#if scan.sameDrive}
            <label class="opt danger">
              <input type="checkbox" checked={deleteSources} onchange={toggleDelete} />
              <span>{t.importPanel.deleteAsYouGo}</span>
            </label>
            {#if deleteSources}<span class="on-note">{t.importPanel.deleteOn}</span>{/if}
          {:else}
            <span>{t.importPanel.otherDrive}</span>
          {/if}
        </div>
      {/if}
      <p class="note">{t.importPanel.dupNote}</p>
      <p class="note">{deleteSources && short ? t.importPanel.convertNoteDelete : t.importPanel.convertNote}</p>
      {#if remaining > 0}<CpuAssistToggle />{/if}
      <button class="btn primary" disabled={running || remaining === 0} onclick={startOutplayed}>
        <Icon name="export" size={15} />{remaining === 0 ? t.importPanel.everythingImported : t.importPanel.importN(remaining)}
      </button>
    {/if}
  </section>
  <section>
    <div class="head"><span class="title">{t.importPanel.fromLibrary}</span></div>
    <p class="note">{t.importPanel.libraryNote}</p>
    <p class="note">{t.importPanel.moveNote}</p>
    <button class="btn" disabled={running} onclick={startLibrary}><Icon name="folder" size={15} />{t.importPanel.chooseAndImport}</button>
  </section>
  {#if running}<p class="dim">{t.importPanel.running}</p>{/if}
</div>

<dialog bind:this={confirmDialog} aria-labelledby="del-q">
  <h2 id="del-q">{t.importPanel.deleteAsk}</h2>
  <p>{t.importPanel.deleteAskBody(folder)}</p>
  <p class="dim">{t.importPanel.deleteAskNote}</p>
  <div class="btns">
    <button class="btn ghost sm" onclick={() => confirmDialog?.close()}>{t.common.cancel}</button>
    <button class="btn danger solid sm" onclick={() => ((deleteSources = true), confirmDialog?.close())}>{t.importPanel.deleteYes}</button>
  </div>
</dialog>

<style>
  .imp {
    display: grid;
    gap: 14px;
  }
  section {
    display: grid;
    gap: 8px;
  }
  section + section {
    padding-top: 14px;
    border-top: 1px solid var(--seam);
  }
  .head {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 12px;
  }
  .title {
    font-family: var(--font-score);
    font-weight: 600;
    font-size: 14px;
  }
  .link {
    color: var(--you);
    font-size: 12.5px;
  }
  .link:hover {
    text-decoration: underline;
    text-underline-offset: 3px;
  }
  .path {
    font-size: 12.5px;
    color: var(--text-2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 3px 14px;
    margin: 0;
    font-size: 12.5px;
  }
  dt {
    color: var(--text-3);
  }
  dd {
    margin: 0;
    text-align: right;
  }
  .note {
    font-size: 12px;
    color: var(--text-3);
  }
  .dim {
    color: var(--text-3);
    font-size: 12.5px;
  }
  .opt {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12.5px;
    color: var(--text-2);
    cursor: pointer;
  }
  .opt input {
    accent-color: var(--you);
    width: 15px;
    height: 15px;
  }
  .warn {
    display: grid;
    gap: 6px;
    padding: 10px 12px;
    border-radius: var(--r-md);
    background: color-mix(in oklab, var(--warn) 12%, var(--panel-2));
    box-shadow: inset 0 0 0 1px color-mix(in oklab, var(--warn) 45%, transparent);
    font-size: 12.5px;
    color: var(--text-2);
  }
  .warn strong {
    color: var(--warn);
    font-weight: 600;
  }
  .opt.danger input {
    accent-color: var(--death);
  }
  .on-note {
    color: var(--death);
    font-weight: 600;
  }
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
  h2 {
    margin: 0 0 10px;
    font-family: var(--font-score);
    font-weight: 600;
    font-size: 16px;
  }
  dialog p {
    margin: 0 0 8px;
    font-size: 13px;
    line-height: 1.5;
    color: var(--text-2);
    word-break: break-word;
  }
  dialog p.dim {
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
