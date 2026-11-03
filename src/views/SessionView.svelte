<script lang="ts">
  import Icon from '../components/Icon.svelte';
  import Player from '../components/Player.svelte';
  import Popover from '../components/Popover.svelte';
  import RecordingTile from '../components/RecordingTile.svelte';
  import Slider from '../components/Slider.svelte';
  import Timeline from '../components/Timeline.svelte';
  import { api } from '../lib/api';
  import { app, artSrc, back, cacheSrc, errorText, go, mediaSrc, recordingsOf, sessionById, setUiPref, toast, ui } from '../lib/state.svelte';
  import { championName, clock, displayTitle, modeName, parseClock } from '../lib/format.svelte';
  import { eventLabel, FILTER_KINDS, filterKey, meta, MULTIKILL_TIERS, supercutCounts } from '../lib/events';
  import SupercutKinds from '../components/SupercutKinds.svelte';
  import { t } from '../lib/i18n.svelte';
  import type { Project, Recording } from '../lib/types';

  let { sessionId, recordingId, at }: { sessionId: string; recordingId?: string; at?: number } = $props();

  const session = $derived(sessionById(sessionId));
  const recs = $derived(recordingsOf(sessionId));
  let selectedId = $state<string | undefined>(undefined);
  $effect.pre(() => {
    selectedId = recordingId;
  });
  const rec = $derived<Recording | undefined>(recs.find((r) => r.id === selectedId) ?? recs.find((r) => r.kind === 'match' || r.kind === 'session') ?? recs[0]);

  let time = $state(0);
  let duration = $state(0);
  let paused = $state(true);
  let seekReq = $state<{ t: number; n: number } | null>(null);
  let seekN = 0;
  const seek = (t: number) => (seekReq = { t, n: ++seekN });
  $effect(() => {
    if (at !== undefined && rec) seek(at);
  });

  let sprite = $state<string | null>(null);
  $effect(() => {
    const id = rec?.id;
    sprite = null;
    if (!id) return;
    api.getSprite(id).then((rel) => {
      if (rec?.id === id && rel) sprite = cacheSrc(rel);
    }).catch(() => {});
  });

  // Filter: nothing picked shows everything; pick one or more kinds to see only those, on the
  // timeline and in the moment list alike.
  // Remembered across matches and restarts (settings.ui.timelineFilter). Kinds a match does not
  // have are ignored there; picking none of what it has shows everything.
  let only = $state<string[]>([...(app.settings?.ui.timelineFilter ?? [])]);
  $effect(() => setUiPref('timelineFilter', [...only]));
  const counts = $derived(
    (rec?.events ?? []).reduce<Record<string, number>>((c, e) => ((c[filterKey(e)] = (c[filterKey(e)] ?? 0) + 1), c), {}),
  );
  const kindsHere = $derived(FILTER_KINDS.filter((k) => counts[k]));
  const picked = $derived(only.filter((k) => counts[k]));
  const shown = $derived((rec?.events ?? []).filter((e) => !picked.length || picked.includes(filterKey(e))));
  const pick = (k: string) => (only = only.includes(k) ? only.filter((x) => x !== k) : [...only, k]);
  const current = $derived(rec?.events.findLast((e) => e.t - e.before <= time + 0.25 && e.t + e.after >= time - 0.25));

  // Tools
  let mode = $state<'view' | 'trim' | 'select'>('view');
  let zoom = $state(1);
  let trim = $state({ a: 0, b: 30 });
  let trimName = $state('');
  let selection = $state<{ a: number; b: number }[]>([]);
  let timeline = $state<ReturnType<typeof Timeline>>();
  let player = $state<ReturnType<typeof Player>>();
  let busy = $state(false);

  $effect(() => {
    // Leaving a recording resets transient tools.
    rec?.id;
    mode = 'view';
    selection = [];
    zoom = 1;
  });

  function startTrim() {
    if (mode === 'trim') return (mode = 'view');
    const d = duration || rec?.duration || 30;
    const a = Math.max(0, Math.min(time, d - 1));
    trim = { a, b: Math.min(d, a + 30) };
    trimName = '';
    mode = 'trim';
    timeline?.focusRange(trim.a, trim.b);
  }

  async function createClip() {
    if (!rec) return;
    busy = true;
    try {
      const r = await api.quickTrim(rec.id, trim.a, trim.b, trimName);
      toast(t.common.saved(displayTitle(r.title)), 'ok', { label: t.common.open, run: () => (selectedId = r.id) });
      mode = 'view';
    } catch (e) {
      toast(errorText(e), 'error');
    } finally {
      busy = false;
    }
  }

  let scOpen = $state(false);
  // Kinds for this recording's supercut: its kill streaks to start with, else its kills.
  const scCounts = $derived(rec ? supercutCounts([{ events: rec.events.filter((e) => e.kind !== 'bookmark') }]) : {});
  let scKinds = $state<string[]>([]);
  $effect(() => {
    rec?.id;
    const tiers = MULTIKILL_TIERS.filter((k) => scCounts[k]);
    scKinds = tiers.length ? tiers : scCounts.kill ? ['kill'] : [];
  });
  let scBefore = $state(5);
  let scAfter = $state(2);
  async function createSupercut() {
    if (!rec) return;
    try {
      const job = await api.supercut(rec.id, scKinds, scBefore, scAfter);
      scOpen = false;
      toast(t.session.buildingSupercut, 'info');
      watchJob(job, (id) => (selectedId = id));
    } catch (e) {
      toast(errorText(e), 'error');
    }
  }
  function watchJob(job: string, onDone: (resultId: string) => void) {
    const iv = setInterval(() => {
      const j = app.jobs[job];
      if (j && j.state === 'done' && j.resultId) {
        clearInterval(iv);
        toast(t.common.ready(j.title), 'ok', { label: t.common.open, run: () => onDone(j.resultId!) });
      } else if (j && j.state !== 'running') clearInterval(iv);
    }, 400);
  }

  let projects = $state<Project[]>([]);
  let targetProject = $state('new');
  async function enterSelect() {
    if (mode === 'select') return (mode = 'view');
    mode = 'select';
    selection = [];
    projects = await api.listProjects().catch(() => []);
  }
  async function openInEditor() {
    if (!rec) return;
    const ranges = selection.length ? [...selection].sort((p, q) => p.a - q.a) : [{ a: 0, b: rec.duration }];
    const clips = ranges.map((r) => ({ recordingId: rec.id, start: r.a, end: r.b }));
    try {
      if (targetProject === 'new') {
        const p = await api.createProject(clips);
        go({ name: 'editor', projectId: p.id });
      } else {
        const p = await api.getProject(targetProject);
        if (!p.media.includes(rec.id)) p.media.push(rec.id);
        p.clips.push(...clips.map((c) => ({ id: crypto.randomUUID().replaceAll('-', ''), recordingId: c.recordingId, start: c.start, end: c.end, crop: 'fit' as const, volume: 100 })));
        await api.saveProject(p);
        go({ name: 'editor', projectId: p.id });
      }
    } catch (e) {
      toast(errorText(e), 'error');
    }
  }

  async function toggleFav() {
    if (!rec) return;
    try {
      await api.setFavorite(rec.id, !rec.favorite);
    } catch (e) {
      toast(errorText(e), 'error');
    }
  }
  async function remove() {
    if (!rec) return;
    const { id, title } = rec;
    try {
      await api.deleteRecordings([id]);
      selectedId = undefined;
      toast(t.common.movedToDeleted(displayTitle(title)), 'info', { label: t.common.undo, run: () => api.restoreRecordings([id]) });
      if (recs.length <= 1) back();
    } catch (e) {
      toast(errorText(e), 'error');
    }
  }

  let renaming = $state(false);
  let nameDraft = $state('');
  async function commitRename() {
    renaming = false;
    if (!rec || !nameDraft.trim() || nameDraft === rec.title) return;
    try {
      await api.renameRecording(rec.id, nameDraft);
    } catch (e) {
      toast(errorText(e), 'error');
    }
  }

  function onKey(e: KeyboardEvent) {
    const tag = (e.target as HTMLElement)?.tagName;
    if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return;
    if (e.ctrlKey && e.key.toLowerCase() === 'q') return (e.preventDefault(), startTrim());
    if (e.ctrlKey && e.key.toLowerCase() === 'm') return (e.preventDefault(), (scOpen = !scOpen));
    if (e.ctrlKey && e.key.toLowerCase() === 'e') return (e.preventDefault(), enterSelect());
    if (e.key === 'Escape' && mode !== 'view') return (mode = 'view');
    if (player?.onKey(e)) e.preventDefault();
  }

  const setTrimFrom = (which: 'a' | 'b', text: string) => {
    const v = parseClock(text);
    if (v === null) return;
    trim = which === 'a' ? { a: Math.max(0, Math.min(v, trim.b - 1)), b: trim.b } : { a: trim.a, b: Math.min(duration || v, Math.max(v, trim.a + 1)) };
  };
  const champIcon = $derived(artSrc(session?.championKey));
</script>

<svelte:window onkeydown={onKey} />

{#if !session || !rec}
  <div class="empty">
    <h3>{t.session.gone}</h3>
    <p>{t.session.goneText}</p>
    <button class="btn sm" onclick={() => go({ name: 'library', tab: 'sessions' })}>{t.session.backToLibrary}</button>
  </div>
{:else}
  <section class="sv">
    <header class="head">
      <button class="icon-btn" onclick={back} aria-label={t.common.back}><Icon name="chevron-left" size={18} /></button>
      {#if champIcon}<img class="champ" src={champIcon} alt="" />{/if}
      <div class="titles">
        {#if renaming}
          <!-- svelte-ignore a11y_autofocus -->
          <input class="field rename" bind:value={nameDraft} autofocus onblur={commitRename} onkeydown={(e) => e.key === 'Enter' && commitRename()} aria-label={t.session.recordingName} />
        {:else}
          <button class="name" onclick={() => ((nameDraft = rec.title), (renaming = true))} title={t.common.rename}>{displayTitle(rec.title)}<Icon name="edit" size={13} /></button>
        {/if}
        <span class="sub">
          {displayTitle(session.title)}{session.champion ? ` · ${championName(session.champion)}` : ''}{session.gameMode ? ` · ${modeName(session.gameMode)}` : ''}
        </span>
      </div>
      {#if session.result}<span class="result {session.result}">{session.result === 'win' ? t.common.victory : t.common.defeat}</span>{/if}
      {#if session.kda}
        <span class="kda num"><span class="k">{session.kda.kills}</span>/<span class="d">{session.kda.deaths}</span>/<span class="a">{session.kda.assists}</span></span>
      {/if}
      <div class="grow"></div>
      <button class="icon-btn" class:on={rec.favorite} onclick={toggleFav} aria-label={rec.favorite ? t.session.removeFav : t.session.addFav} title={t.session.favorite}>
        <Icon name={rec.favorite ? 'star-on' : 'star'} size={18} />
      </button>
      <button class="icon-btn" onclick={() => api.revealRecording(rec.id)} aria-label={t.session.showInFolder} title={t.session.showInFolder}><Icon name="folder" size={18} /></button>
      <span class="gap"></span>
      <button class="btn danger solid" onclick={remove}><Icon name="trash" size={16} />{t.common.delete}</button>
    </header>

    <div class="main">
      <div class="stage">
        <div class="video">
          <Player bind:this={player} src={mediaSrc(rec, rec.file)} poster={mediaSrc(rec, rec.thumb)} image={rec.kind === 'screenshot'} bind:time bind:duration bind:paused seekTo={seekReq} />
          {#if current && !paused}
            <span class="now-chip" style="--c:{meta(current.kind).color}"><Icon name={meta(current.kind).icon} size={14} />{eventLabel(current)}</span>
          {/if}
        </div>

        {#if rec.kind !== 'screenshot'}
          <div class="toolbar">
            <div class="mode-panel">
              {#if mode === 'trim'}
                <div class="trim-bar">
                  <label class="tf"><span>{t.session.start}</span><input class="field num" value={clock(trim.a, true)} onchange={(e) => setTrimFrom('a', (e.currentTarget as HTMLInputElement).value)} aria-label={t.timeline.clipStart} /></label>
                  <label class="tf"><span>{t.session.end}</span><input class="field num" value={clock(trim.b, true)} onchange={(e) => setTrimFrom('b', (e.currentTarget as HTMLInputElement).value)} aria-label={t.timeline.clipEnd} /></label>
                  <input class="field clipname" placeholder={t.session.clipName} bind:value={trimName} aria-label={t.session.clipName} />
                  <button class="btn primary sm" disabled={busy} onclick={createClip}><Icon name="scissors" size={15} />{busy ? t.common.saving : t.session.createClip}</button>
                  <button class="btn ghost sm" onclick={() => (mode = 'view')}>{t.common.cancel}</button>
                </div>
              {:else if mode === 'select'}
                <div class="trim-bar">
                  <span class="sel-count"><span class="num">{selection.length}</span> {t.session.rangesSelected(selection.length)} <span class="dim">{t.session.selectHint}</span></span>
                  <select class="field proj" bind:value={targetProject} aria-label={t.session.project}>
                    <option value="new">{t.session.newProject}</option>
                    {#each projects as p (p.id)}<option value={p.id}>{displayTitle(p.name)}</option>{/each}
                  </select>
                  <button class="btn primary sm" onclick={openInEditor}><Icon name="editor" size={15} />{selection.length ? t.session.openInEditor : t.session.openWholeInEditor}</button>
                  {#if selection.length}<button class="btn ghost sm" onclick={() => (selection = [])}>{t.common.clear}</button>{/if}
                </div>
              {:else if kindsHere.length}
                <div class="filters" role="group" aria-label={t.session.filter}>
                  <button class="fchip all" class:on={!picked.length} aria-pressed={!picked.length} onclick={() => (only = [])}>{t.session.tabAll}<span class="num n">{rec.events.length}</span></button>
                  {#each kindsHere as k (k)}
                    <button class="fchip" class:on={only.includes(k)} aria-pressed={only.includes(k)} style="--c:{meta(k).color}" onclick={() => pick(k)}>
                      <Icon name={meta(k).icon} size={13} /><span>{t.filters[k] ?? meta(k).label}</span><span class="num n">{counts[k]}</span>
                    </button>
                  {/each}
                </div>
              {/if}
            </div>
            <div class="tools">
              <Popover bind:open={scOpen} width={290} placement="above">
                {#snippet anchor(toggleOpen)}
                  <button class="icon-btn" class:on={scOpen} onclick={toggleOpen} aria-label={t.session.supercutKey} title={t.session.supercutKey} disabled={!rec.events.length}><Icon name="bolt" size={17} /></button>
                {/snippet}
                {#snippet children()}
                  <div class="sc">
                    <div class="sc-title">{t.session.supercut}</div>
                    <SupercutKinds counts={scCounts} bind:selected={scKinds} />
                    <div class="sc-title sub">{t.session.clipBuffer}</div>
                    <label class="sc-row"><span>{t.session.before}</span><Slider value={scBefore} min={0} max={20} onchange={(v) => (scBefore = v)} label={t.session.secondsBefore} suffix={t.common.seconds} /></label>
                    <label class="sc-row"><span>{t.session.after}</span><Slider value={scAfter} min={0} max={20} onchange={(v) => (scAfter = v)} label={t.session.secondsAfter} suffix={t.common.seconds} /></label>
                    <button class="btn primary" disabled={!scKinds.some((k) => scCounts[k])} onclick={createSupercut}><Icon name="bolt" size={15} />{t.session.createSupercut}</button>
                  </div>
                {/snippet}
              </Popover>
              <button class="icon-btn" class:on={mode === 'select'} onclick={enterSelect} aria-label={t.session.editorKey} title={t.session.editorKey}><Icon name="editor" size={17} /></button>
              <button class="icon-btn" class:on={mode === 'trim'} onclick={startTrim} aria-label={t.session.trimKey} title={t.session.trimKey}><Icon name="scissors" size={17} /></button>
              <span class="divider"></span>
              <button class="icon-btn" onclick={() => (zoom = Math.max(1, zoom / 1.5))} disabled={zoom <= 1} aria-label={t.session.zoomOut} title={t.session.zoomOutWheel}><Icon name="minus" size={16} /></button>
              <span class="zoom num">{zoom < 10 ? zoom.toFixed(1) : Math.round(zoom)}x</span>
              <button class="icon-btn" onclick={() => (zoom = Math.min(48, zoom * 1.5))} aria-label={t.session.zoomIn} title={t.session.zoomInWheel}><Icon name="plus" size={16} /></button>
              <span class="divider"></span>
              <button class="icon-btn" class:on={ui.showApm} onclick={() => (ui.showApm = !ui.showApm)} disabled={!rec.apm?.length} aria-label={t.session.apm} title={rec.apm?.length ? t.session.apm : t.session.noApm}><Icon name="gauge" size={17} /></button>
            </div>
          </div>

          <Timeline bind:this={timeline} duration={duration || rec.duration} {time} onseek={seek} events={shown} apm={rec.apm ?? []} showApm={ui.showApm} {sprite} {mode} bind:trim bind:selection bind:zoom />
        {/if}
      </div>

      <aside class="side">
        <div class="side-title head">{t.session.moments} <span class="num dim">{shown.length}</span></div>
        <ol class="moments">
          {#each shown as e (e.t + e.kind)}
            {@const m = meta(e.kind)}
            <li>
              <button class="moment" class:live={current === e} style="--c:{m.color}" onclick={() => seek(Math.max(0, e.t - e.before))}>
                <span class="m-icon"><Icon name={m.icon} size={15} /></span>
                <span class="m-label">{eventLabel(e)}</span>
                <span class="m-t num">{clock(e.t)}</span>
              </button>
            </li>
          {:else}
            <li class="none">{rec.events.length ? t.session.nothingInFilter : t.session.noGameEvents}</li>
          {/each}
        </ol>
        {#if recs.length > 1}
          <div class="in-session">
            <div class="side-title">{t.session.inSession} <span class="num dim">{recs.length}</span></div>
            <div class="strip">
              {#each recs as r (r.id)}
                <RecordingTile rec={r} compact active={r.id === rec.id} onclick={() => (selectedId = r.id)} />
              {/each}
            </div>
          </div>
        {/if}
      </aside>
    </div>
  </section>
{/if}

<style>
  .sv {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 0 16px 0 12px;
    height: 60px;
    border-bottom: 1px solid var(--seam);
    flex: none;
  }
  .champ {
    width: 38px;
    height: 38px;
    border-radius: var(--r-md);
  }
  .titles {
    display: grid;
    min-width: 0;
  }
  .name {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-family: var(--font-score);
    font-weight: 600;
    font-size: 18px;
    text-align: left;
    white-space: nowrap;
  }
  .name :global(svg) {
    color: var(--text-3);
    opacity: 0;
    transition: opacity 120ms;
  }
  .name:hover :global(svg) {
    opacity: 1;
  }
  .rename {
    width: 320px;
  }
  .sub {
    color: var(--text-3);
    font-size: 12.5px;
    white-space: nowrap;
  }
  .result {
    font-family: var(--font-score);
    font-weight: 700;
    font-size: 12.5px;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    padding: 3px 8px;
    border-radius: var(--r-sm);
  }
  .result.win {
    color: var(--win);
    background: color-mix(in oklab, var(--win) 12%, transparent);
  }
  .result.loss {
    color: var(--death);
    background: color-mix(in oklab, var(--death) 12%, transparent);
  }
  .kda {
    font-size: 24px;
    color: var(--text-3);
  }
  .k {
    color: var(--you);
  }
  .d {
    color: var(--death);
  }
  .a {
    color: var(--assist);
  }
  .grow {
    flex: 1;
  }
  .gap {
    width: 14px;
  }
  .main {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(0, 1fr) 300px;
  }
  .stage {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    padding: 14px 16px 10px;
    gap: 8px;
  }
  .video {
    position: relative;
    flex: 1;
    min-height: 200px;
  }
  .now-chip {
    position: absolute;
    top: 12px;
    left: 12px;
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 10px;
    border-radius: var(--r-sm);
    background: var(--scrim);
    color: var(--c);
    font-weight: 600;
    font-size: 13px;
    pointer-events: none;
  }
  .toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    min-height: 36px;
  }
  .mode-panel {
    min-width: 0;
    flex: 1;
  }
  .tools {
    display: flex;
    align-items: center;
    gap: 2px;
  }
  .divider {
    width: 1px;
    height: 18px;
    background: var(--seam);
    margin: 0 6px;
  }
  .zoom {
    min-width: 36px;
    text-align: center;
    font-size: 12.5px;
    color: var(--text-3);
  }
  .filters {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .fchip {
    --c: var(--text-2);
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 28px;
    padding: 0 10px;
    border-radius: 14px;
    color: var(--text-2);
    font-size: 12.5px;
    font-weight: 600;
    box-shadow: inset 0 0 0 1px var(--seam-strong);
  }
  .fchip :global(svg) {
    color: var(--c);
  }
  .fchip .n {
    font-weight: 500;
    color: var(--text-3);
  }
  .fchip:hover {
    color: var(--text);
    background: var(--panel-2);
  }
  .fchip.on {
    color: var(--text);
    background: color-mix(in oklab, var(--c) 18%, var(--panel-2));
    box-shadow: inset 0 0 0 1px var(--c);
  }
  .fchip.on .n {
    color: var(--text-2);
  }
  .fchip.all.on {
    background: var(--panel-3);
    box-shadow: inset 0 0 0 1px var(--seam-strong);
  }
  .dim {
    color: var(--text-3);
    font-weight: 400;
  }
  .trim-bar {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }
  .tf {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--text-3);
    font-size: 12px;
  }
  .tf input {
    width: 92px;
    height: 28px;
    font-size: 13px;
  }
  .clipname {
    width: 200px;
    height: 28px;
  }
  .proj {
    height: 28px;
    width: 200px;
  }
  .sel-count {
    font-weight: 600;
  }
  .sc {
    display: grid;
    gap: 6px;
  }
  .sc-title {
    font-family: var(--font-score);
    font-weight: 600;
    font-size: 14px;
    margin-bottom: 2px;
  }
  .sc-title.sub {
    margin-top: 6px;
    font-size: 12.5px;
    color: var(--text-3);
  }
  .sc-row {
    display: grid;
    grid-template-columns: 50px 1fr;
    align-items: center;
    font-size: 12.5px;
    color: var(--text-2);
  }
  .side {
    display: flex;
    flex-direction: column;
    min-height: 0;
    border-left: 1px solid var(--seam);
    background: var(--panel);
  }
  .moments {
    list-style: none;
    margin: 0;
    padding: 4px 8px;
    overflow: auto;
    flex: 1;
    min-height: 100px;
  }
  .moment {
    width: 100%;
    display: grid;
    grid-template-columns: 26px 1fr auto;
    align-items: center;
    gap: 8px;
    height: 34px;
    padding: 0 8px 0 4px;
    border-radius: var(--r-md);
    text-align: left;
  }
  .moment:hover {
    background: var(--panel-2);
  }
  .moment.live {
    background: var(--panel-3);
  }
  .m-icon {
    display: grid;
    place-items: center;
    color: var(--c);
  }
  .m-label {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .m-t {
    color: var(--text-3);
    font-size: 12.5px;
  }
  .none {
    color: var(--text-3);
    padding: 12px 8px;
    font-size: 12.5px;
  }
  .in-session {
    border-top: 1px solid var(--seam);
    padding: 10px 8px;
    max-height: 46%;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .side-title {
    font-family: var(--font-score);
    font-weight: 600;
    font-size: 13px;
    padding: 0 6px 6px;
  }
  .side-title.head {
    padding: 14px 14px 4px;
  }
  .strip {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 2px;
    overflow: auto;
  }
  @media (min-width: 2200px) {
    .main {
      grid-template-columns: minmax(0, 1fr) 360px;
    }
  }
</style>
