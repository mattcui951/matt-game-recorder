<script lang="ts">
  import Icon from '../components/Icon.svelte';
  import Popover from '../components/Popover.svelte';
  import Segmented from '../components/Segmented.svelte';
  import Slider from '../components/Slider.svelte';
  import Toggle from '../components/Toggle.svelte';
  import { api } from '../lib/api';
  import { app, cacheSrc, errorText, go, mediaSrc, toast } from '../lib/state.svelte';
  import { clock, dayLabel, displayTitle, parseClock, timeOfDay } from '../lib/format.svelte';
  import { t } from '../lib/i18n.svelte';
  import type { Project, ProjectClip, Recording } from '../lib/types';

  let { projectId }: { projectId?: string } = $props();

  let projects = $state<Project[]>([]);
  let project = $state<Project | null>(null);
  let loadError = $state('');

  $effect(() => {
    const id = projectId;
    project = null;
    loadError = '';
    if (id) {
      api.getProject(id).then((p) => {
        project = p;
        past = [];
        future = [];
        selected = p.clips[0]?.id ?? null;
      }).catch((e) => (loadError = errorText(e)));
    } else {
      api.listProjects().then((p) => (projects = p)).catch(() => {});
    }
  });

  const recById = $derived(new Map(app.library.recordings.map((r) => [r.id, r])));
  const clipLen = (c: ProjectClip) => Math.max(0, c.end - c.start);
  const total = $derived(project ? project.clips.reduce((a, c) => a + clipLen(c), 0) : 0);

  // ---------- history ----------
  let past = $state<string[]>([]);
  let future = $state<string[]>([]);
  let saveTimer: ReturnType<typeof setTimeout> | undefined;
  function snapshot(): string {
    return JSON.stringify($state.snapshot(project));
  }
  function commit(mut: (p: Project) => void) {
    if (!project) return;
    past = [...past.slice(-60), snapshot()];
    future = [];
    mut(project);
    persist();
  }
  function persist() {
    clearTimeout(saveTimer);
    saveTimer = setTimeout(() => {
      if (project) api.saveProject($state.snapshot(project) as Project).catch((e) => toast(t.editor.notSaved(errorText(e)), 'error'));
    }, 300);
  }
  function undo() {
    if (!past.length || !project) return;
    future = [snapshot(), ...future];
    project = JSON.parse(past[past.length - 1]);
    past = past.slice(0, -1);
    persist();
  }
  function redo() {
    if (!future.length || !project) return;
    past = [...past, snapshot()];
    project = JSON.parse(future[0]);
    future = future.slice(1);
    persist();
  }

  // ---------- selection & playback ----------
  let selected = $state<string | null>(null);
  const selClip = $derived(project?.clips.find((c) => c.id === selected) ?? null);
  const selRec = $derived(selClip ? recById.get(selClip.recordingId) : undefined);
  let panel = $state<'media' | 'clip' | 'project'>('media');

  let video = $state<HTMLVideoElement>();
  let playhead = $state(0);
  let playing = $state(false);
  let clipIndex = $state(0);

  function locate(t: number): { i: number; offset: number } {
    if (!project) return { i: 0, offset: 0 };
    let acc = 0;
    for (let i = 0; i < project.clips.length; i++) {
      const len = clipLen(project.clips[i]);
      if (t < acc + len || i === project.clips.length - 1) return { i, offset: Math.max(0, Math.min(len, t - acc)) };
      acc += len;
    }
    return { i: 0, offset: 0 };
  }
  const clipStart = (i: number) => (project ? project.clips.slice(0, i).reduce((a, c) => a + clipLen(c), 0) : 0);
  const curClip = $derived(project?.clips[clipIndex]);
  const curRec = $derived(curClip ? recById.get(curClip.recordingId) : undefined);
  const curSrc = $derived(curRec ? mediaSrc(curRec, curRec.file) : '');

  function seekProject(t: number) {
    if (!project?.clips.length) return;
    playhead = Math.max(0, Math.min(total, t));
    const { i, offset } = locate(playhead);
    clipIndex = i;
    const c = project.clips[i];
    queueMicrotask(() => {
      if (video) video.currentTime = c.start + offset;
    });
  }
  function onLoaded() {
    if (!video || !project) return;
    const c = project.clips[clipIndex];
    const want = c.start + (playhead - clipStart(clipIndex));
    if (Math.abs(video.currentTime - want) > 0.3) video.currentTime = want;
    if (playing) video.play().catch(() => {});
  }
  function onTime() {
    if (!video || !project) return;
    const c = project.clips[clipIndex];
    if (!c) return;
    playhead = clipStart(clipIndex) + Math.max(0, video.currentTime - c.start);
    if (video.currentTime >= c.end - 0.03) {
      if (clipIndex < project.clips.length - 1) {
        clipIndex += 1;
        const n = project.clips[clipIndex];
        if (curSrc && recById.get(n.recordingId)?.file === curRec?.file) video.currentTime = n.start;
      } else {
        video.pause();
        playing = false;
      }
    }
  }
  function togglePlay() {
    if (!video) return;
    if (playing) {
      video.pause();
      playing = false;
    } else {
      if (playhead >= total - 0.05) seekProject(0);
      playing = true;
      video.play().catch(() => (playing = false));
    }
  }

  // ---------- timeline edits ----------
  function newId() {
    return crypto.randomUUID().replaceAll('-', '');
  }
  function slice() {
    if (!project) return;
    const { i, offset } = locate(playhead);
    const c = project.clips[i];
    if (!c || offset < 0.5 || clipLen(c) - offset < 0.5) {
      toast(t.editor.sliceHint);
      return;
    }
    commit((p) => {
      const cut = c.start + offset;
      const right: ProjectClip = { ...$state.snapshot(c), id: newId(), start: cut };
      p.clips[i].end = cut;
      p.clips.splice(i + 1, 0, right);
    });
  }
  function duplicate() {
    if (!project || !selClip) return;
    const i = project.clips.findIndex((c) => c.id === selClip.id);
    const copy = { ...$state.snapshot(selClip), id: newId() };
    commit((p) => p.clips.splice(i + 1, 0, copy));
    selected = copy.id;
  }
  function removeClip() {
    if (!project || !selClip) return;
    const i = project.clips.findIndex((c) => c.id === selClip.id);
    commit((p) => p.clips.splice(i, 1));
    selected = project.clips[Math.min(i, project.clips.length - 1)]?.id ?? null;
  }
  function addToTimeline(r: Recording) {
    commit((p) => {
      if (!p.media.includes(r.id)) p.media.push(r.id);
      p.clips.push({ id: newId(), recordingId: r.id, start: 0, end: r.duration, crop: 'fit', volume: 100 });
    });
  }

  let zoom = $state(1);
  let tlWidth = $state(800);
  const pxPerSec = $derived(total > 0 ? ((tlWidth - 24) * zoom) / total : 1);

  // Reorder by dragging a block; trim by dragging its edges.
  let drag: { kind: 'move' | 'l' | 'r'; id: string; x0: number; orig: ProjectClip; before: string } | null = null;
  let dropIndex = $state<number | null>(null);
  function blockDown(e: PointerEvent, c: ProjectClip, kind: 'move' | 'l' | 'r') {
    e.stopPropagation();
    selected = c.id;
    panel = 'clip';
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    drag = { kind, id: c.id, x0: e.clientX, orig: $state.snapshot(c) as ProjectClip, before: snapshot() };
  }
  function blockMove(e: PointerEvent) {
    if (!drag || !project) return;
    const dx = (e.clientX - drag.x0) / pxPerSec;
    const c = project.clips.find((x) => x.id === drag!.id);
    const r = c && recById.get(c.recordingId);
    if (!c || !r) return;
    if (drag.kind === 'l') c.start = Math.max(0, Math.min(drag.orig.end - 0.5, drag.orig.start + dx));
    else if (drag.kind === 'r') c.end = Math.min(r.duration, Math.max(drag.orig.start + 0.5, drag.orig.end + dx));
    else if (Math.abs(e.clientX - drag.x0) > 6) {
      const track = (e.currentTarget as HTMLElement).closest('.track') as HTMLElement;
      const x = e.clientX - track.getBoundingClientRect().left + track.scrollLeft - 12;
      let acc = 0;
      let idx = project.clips.length;
      for (let i = 0; i < project.clips.length; i++) {
        const w = clipLen(project.clips[i]) * pxPerSec;
        if (x < acc + w / 2) {
          idx = i;
          break;
        }
        acc += w;
      }
      dropIndex = idx;
    }
  }
  function blockUp() {
    if (!drag || !project) return;
    const changed = drag.kind !== 'move' || dropIndex !== null;
    if (drag.kind === 'move' && dropIndex !== null) {
      const from = project.clips.findIndex((c) => c.id === drag!.id);
      const [c] = project.clips.splice(from, 1);
      project.clips.splice(dropIndex > from ? dropIndex - 1 : dropIndex, 0, c);
    }
    if (changed) {
      past = [...past.slice(-60), drag.before];
      future = [];
      persist();
    }
    drag = null;
    dropIndex = null;
  }
  function trackClick(e: MouseEvent) {
    const track = e.currentTarget as HTMLElement;
    const x = e.clientX - track.getBoundingClientRect().left + track.scrollLeft - 12;
    seekProject(x / pxPerSec);
  }

  const sprites = $state<Record<string, string>>({});
  $effect(() => {
    for (const c of project?.clips ?? []) {
      if (sprites[c.recordingId] === undefined) {
        sprites[c.recordingId] = '';
        api.getSprite(c.recordingId).then((rel) => rel && (sprites[c.recordingId] = cacheSrc(rel))).catch(() => {});
      }
    }
  });

  // ---------- import & export ----------
  let importOpen = $state(false);
  const importable = $derived(app.library.recordings.filter((r) => !r.deletedAt && r.kind !== 'screenshot' && r.duration > 0));
  let exportOpen = $state(false);
  let codec = $state<'h264' | 'av1' | 'hevc'>('h264');
  let exportJob = $state<string | null>(null);
  const job = $derived(exportJob ? app.jobs[exportJob] : undefined);
  async function startExport() {
    if (!project) return;
    exportOpen = false;
    clearTimeout(saveTimer);
    try {
      await api.saveProject($state.snapshot(project) as Project);
      exportJob = await api.exportProject(project.id, codec);
    } catch (e) {
      toast(errorText(e), 'error');
    }
  }

  async function newProject() {
    try {
      const p = await api.createProject([]);
      go({ name: 'editor', projectId: p.id });
    } catch (e) {
      toast(errorText(e), 'error');
    }
  }
  async function deleteProject(id: string) {
    try {
      await api.deleteProject(id);
      if (projectId === id) go({ name: 'editor' });
      else projects = projects.filter((p) => p.id !== id);
    } catch (e) {
      toast(errorText(e), 'error');
    }
  }

  const ASPECTS = $derived([
    { value: '16:9', label: '16:9' },
    { value: '9:16', label: '9:16' },
    { value: '1:1', label: '1:1' },
    { value: '4:5', label: '4:5' },
    { value: '21:9', label: '21:9' },
    { value: 'original', label: t.editor.source },
  ]);
  const aspectCss = $derived.by(() => {
    const a = project?.aspect ?? '16:9';
    if (a === 'original') {
      const r = curRec ?? (project?.clips[0] && recById.get(project.clips[0].recordingId));
      return r && r.width ? `${r.width} / ${r.height}` : '16 / 9';
    }
    return a.replace(':', ' / ');
  });
  const fitCss = $derived(curClip?.crop === 'fill' ? 'cover' : curClip?.crop === 'stretch' ? 'fill' : 'contain');

  function onKey(e: KeyboardEvent) {
    if (!project) return;
    const tag = (e.target as HTMLElement)?.tagName;
    if (tag === 'INPUT' || tag === 'SELECT' || tag === 'TEXTAREA') return;
    if (e.ctrlKey && e.key.toLowerCase() === 'z') return (e.preventDefault(), e.shiftKey ? redo() : undo());
    if (e.ctrlKey && e.key.toLowerCase() === 'y') return (e.preventDefault(), redo());
    if (e.ctrlKey && e.key.toLowerCase() === 'd') return (e.preventDefault(), duplicate());
    if (e.key === 'Delete' || e.key === 'Backspace') return (e.preventDefault(), removeClip());
    if (e.key === 's' && !e.ctrlKey) return slice();
    if (e.key === ' ') return (e.preventDefault(), togglePlay());
  }
  function setClipTime(which: 'start' | 'end', text: string) {
    const v = parseClock(text);
    if (v === null || !selClip || !selRec) return;
    commit((p) => {
      const c = p.clips.find((x) => x.id === selClip.id)!;
      if (which === 'start') c.start = Math.max(0, Math.min(v, c.end - 0.5));
      else c.end = Math.min(selRec.duration, Math.max(v, c.start + 0.5));
    });
  }
  // "edited today 9:35 PM", but "edited Saturday, Oct 31 9:35 PM": only the relative words go lowercase.
  function editedDay(iso: string): string {
    const d = dayLabel(iso);
    return d === t.dates.today || d === t.dates.yesterday ? d.toLowerCase() : d;
  }
</script>

<svelte:window onkeydown={onKey} />

{#if !projectId}
  <section class="list">
    <header class="lhead">
      <h1>{t.editor.title}</h1>
      <button class="btn primary" onclick={newProject}><Icon name="plus" size={16} />{t.editor.newProject}</button>
    </header>
    {#if projects.length}
      <div class="projects">
        {#each projects as p (p.id)}
          {@const first = p.clips[0] && recById.get(p.clips[0].recordingId)}
          <div class="pcard">
            <button class="p-open" onclick={() => go({ name: 'editor', projectId: p.id })}>
              <span class="p-thumb">{#if first?.thumb}<img src={mediaSrc(first, first.thumb)} alt="" />{/if}<span class="p-aspect num">{p.aspect}</span></span>
              <span class="p-name">{displayTitle(p.name)}</span>
              <span class="p-sub"><span class="num">{t.editor.clips(p.clips.length)}</span> · <span class="num">{clock(p.clips.reduce((a, c) => a + c.end - c.start, 0))}</span> · {t.editor.edited(editedDay(p.updatedAt), timeOfDay(p.updatedAt))}</span>
            </button>
            <button class="icon-btn p-del" onclick={() => deleteProject(p.id)} aria-label={t.editor.deleteProjectAria(displayTitle(p.name))} title={t.editor.deleteProject}><Icon name="trash" size={16} /></button>
          </div>
        {/each}
      </div>
    {:else}
      <div class="empty">
        <h3>{t.editor.noProjects}</h3>
        <p>{t.editor.noProjectsText}</p>
      </div>
    {/if}
  </section>
{:else if loadError}
  <div class="empty"><h3>{t.editor.unavailable}</h3><p>{loadError}</p><button class="btn sm" onclick={() => go({ name: 'editor' })}>{t.editor.allProjects}</button></div>
{:else if project}
  <section class="ed">
    <header class="ehead">
      <button class="icon-btn" onclick={() => go({ name: 'editor' })} aria-label={t.editor.allProjects}><Icon name="chevron-left" size={18} /></button>
      <h1 class="crumb"><span class="dim">{t.editor.title}</span> <Icon name="chevron-right" size={14} /> {displayTitle(project.name)}</h1>
      <div class="grow"></div>
      <button class="icon-btn" onclick={undo} disabled={!past.length} aria-label={t.editor.undo} title={t.editor.undo}><Icon name="undo" size={17} /></button>
      <span class="hist num" title={t.editor.history}>{past.length}</span>
      <button class="icon-btn" onclick={redo} disabled={!future.length} aria-label={t.editor.redo} title={t.editor.redo}><Icon name="redo" size={17} /></button>
      <Popover bind:open={exportOpen} width={280}>
        {#snippet anchor(toggleOpen)}
          <button class="btn primary" onclick={toggleOpen} disabled={!project?.clips.length || job?.state === 'running'}><Icon name="export" size={16} />{t.editor.exportVideo}</button>
        {/snippet}
        {#snippet children()}
          <div class="exp">
            <div class="exp-title">{t.editor.exportFormat}</div>
            <Segmented
              label={t.editor.exportCodec}
              value={codec}
              onchange={(v) => (codec = v)}
              options={[
                { value: 'h264', label: 'H.264', hint: t.editor.h264Hint },
                { value: 'hevc', label: 'HEVC', hint: t.editor.hevcHint },
                { value: 'av1', label: 'AV1', hint: t.editor.av1Hint },
              ]}
            />
            <p class="exp-note">{codec === 'h264' ? t.editor.h264Note : codec === 'hevc' ? t.editor.hevcNote : t.editor.av1Note}</p>
            <p class="exp-note"><span class="num">{clock(total)}</span> · {project?.aspect === 'original' ? t.editor.sourceSize : project?.aspect}</p>
            <button class="btn primary" onclick={startExport}><Icon name="export" size={15} />{t.editor.export}</button>
          </div>
        {/snippet}
      </Popover>
    </header>

    <div class="work">
      <aside class="panel">
        <div class="ptabs" role="tablist">
          <button role="tab" aria-selected={panel === 'media'} class:on={panel === 'media'} onclick={() => (panel = 'media')} title={t.editor.projectVideos}><Icon name="duplicate" size={16} /></button>
          <button role="tab" aria-selected={panel === 'clip'} class:on={panel === 'clip'} onclick={() => (panel = 'clip')} title={t.editor.clipProperties}><Icon name="settings" size={16} /></button>
          <button role="tab" aria-selected={panel === 'project'} class:on={panel === 'project'} onclick={() => (panel = 'project')} title={t.editor.projectSettings}><Icon name="editor" size={16} /></button>
        </div>
        <div class="pbody">
          {#if panel === 'media'}
            <div class="ptitle">{t.editor.projectVideos}</div>
            {#each project.media as id (id)}
              {@const r = recById.get(id)}
              {#if r}
                <div class="media">
                  <span class="m-thumb">{#if r.thumb}<img src={mediaSrc(r, r.thumb)} alt="" />{/if}<span class="num">{clock(r.duration)}</span></span>
                  <span class="m-name">{displayTitle(r.title)}</span>
                  <button class="icon-btn" onclick={() => addToTimeline(r)} aria-label={t.editor.addAria(displayTitle(r.title))} title={t.editor.addToTimeline}><Icon name="plus" size={16} /></button>
                </div>
              {:else}
                <div class="media missing">{t.editor.sourceDeleted}</div>
              {/if}
            {/each}
            <Popover bind:open={importOpen} width={320} align="left">
              {#snippet anchor(toggleOpen)}<button class="btn sm wide" onclick={toggleOpen}><Icon name="plus" size={15} />{t.editor.importVideos}</button>{/snippet}
              {#snippet children()}
                <div class="import">
                  {#each importable as r (r.id)}
                    {@const inProject = project?.media.includes(r.id)}
                    <button class="imp" class:on={inProject} onclick={() => commit((p) => (p.media.includes(r.id) ? (p.media = p.media.filter((m) => m !== r.id)) : p.media.push(r.id)))}>
                      <span class="m-thumb sm">{#if r.thumb}<img src={mediaSrc(r, r.thumb)} alt="" />{/if}</span>
                      <span class="m-name">{displayTitle(r.title)}<span class="dim num"> {clock(r.duration)}</span></span>
                      {#if inProject}<Icon name="check" size={15} />{/if}
                    </button>
                  {:else}
                    <span class="dim">{t.editor.noVideos}</span>
                  {/each}
                </div>
              {/snippet}
            </Popover>
          {:else if panel === 'clip'}
            <div class="ptitle">{t.editor.clipProperties}</div>
            {#if selClip && selRec}
              <label class="prop"><span>{t.editor.startTime}</span><input class="field num" value={clock(selClip.start, true)} onchange={(e) => setClipTime('start', (e.currentTarget as HTMLInputElement).value)} /></label>
              <label class="prop"><span>{t.editor.endTime}</span><input class="field num" value={clock(selClip.end, true)} onchange={(e) => setClipTime('end', (e.currentTarget as HTMLInputElement).value)} /></label>
              <div class="prop">
                <span>{t.editor.crop}</span>
                <Segmented label={t.editor.crop} value={selClip.crop} onchange={(v) => commit((p) => (p.clips.find((c) => c.id === selClip.id)!.crop = v))} options={[{ value: 'fit', label: t.editor.fit }, { value: 'fill', label: t.editor.fill }, { value: 'stretch', label: t.editor.stretch }]} />
              </div>
              <div class="prop"><span>{t.editor.volume}</span><Slider label={t.editor.clipVolume} value={selClip.volume} max={200} onchange={(v) => commit((p) => (p.clips.find((c) => c.id === selClip.id)!.volume = v))} /></div>
              <dl class="info">
                <dt>{t.editor.sourceVideo}</dt><dd>{displayTitle(selRec.title)}</dd>
                <dt>{t.editor.duration}</dt><dd class="num">{clock(clipLen(selClip), true)}</dd>
                <dt>{t.editor.resolution}</dt><dd class="num">{selRec.width}x{selRec.height}</dd>
                <dt>{t.editor.frameRate}</dt><dd class="num">{t.editor.fps(Math.round(selRec.fps))}</dd>
                <dt>{t.editor.codec}</dt><dd>{selRec.codec.toUpperCase()}</dd>
              </dl>
            {:else}
              <p class="dim">{t.editor.selectClip}</p>
            {/if}
          {:else}
            <div class="ptitle">{t.editor.projectSettings}</div>
            <label class="prop"><span>{t.editor.projectName}</span><input class="field" value={displayTitle(project.name)} onchange={(e) => commit((p) => (p.name = (e.currentTarget as HTMLInputElement).value.trim() || p.name))} /></label>
            <div class="prop"><span>{t.editor.volume}</span><Slider label={t.editor.projectVolume} value={project.volume} max={200} onchange={(v) => commit((p) => (p.volume = v))} /></div>
            <div class="prop row"><span>{t.editor.protect}<small>{t.editor.protectHint}</small></span><Toggle label={t.editor.protect} checked={project.protect} onchange={(v) => commit((p) => (p.protect = v))} /></div>
            <div class="prop row"><span>{t.editor.watermark}</span><Toggle label={t.editor.watermark} checked={project.watermark} onchange={(v) => commit((p) => (p.watermark = v))} /></div>
            <button class="btn danger sm wide" onclick={() => project && deleteProject(project.id)}><Icon name="trash" size={15} />{t.editor.deleteProject}</button>
          {/if}
        </div>
      </aside>

      <div class="preview">
        <div class="frame-wrap">
          <div class="frame" style="aspect-ratio:{aspectCss}">
            {#if curSrc}
              <!-- svelte-ignore a11y_media_has_caption -->
              <video bind:this={video} src={curSrc} style="object-fit:{fitCss}" preload="auto" onloadedmetadata={onLoaded} ontimeupdate={onTime} onclick={togglePlay} volume={Math.min(1, ((curClip?.volume ?? 100) / 100) * (project.volume / 100))}></video>
            {:else}
              <div class="noclips">{t.editor.noClips}</div>
            {/if}
          </div>
        </div>
        <div class="pctl">
          <button class="icon-btn" onclick={togglePlay} disabled={!project.clips.length} aria-label={playing ? t.player.pause : t.player.play}><Icon name={playing ? 'pause' : 'play'} size={18} /></button>
          <span class="num ptime">{clock(playhead)} <span class="dim">/ {clock(total)}</span></span>
          <div class="grow"></div>
          <span class="dim lbl">{t.editor.aspect}</span>
          <Segmented label={t.editor.aspectAria} value={project.aspect} options={ASPECTS} onchange={(v) => commit((p) => (p.aspect = v))} />
        </div>
      </div>
    </div>

    <div class="tl">
      <div class="tl-tools">
        <button class="icon-btn" onclick={slice} disabled={!project.clips.length} aria-label={t.editor.slice} title={t.editor.slice}><Icon name="slice" size={17} /></button>
        <button class="icon-btn" onclick={duplicate} disabled={!selClip} aria-label={t.editor.duplicate} title={t.editor.duplicate}><Icon name="duplicate" size={17} /></button>
        <button class="icon-btn" onclick={removeClip} disabled={!selClip} aria-label={t.editor.remove} title={t.editor.remove}><Icon name="trash" size={17} /></button>
        <div class="grow"></div>
        <button class="icon-btn" onclick={() => (zoom = Math.max(1, zoom / 1.5))} disabled={zoom <= 1} aria-label={t.session.zoomOut}><Icon name="minus" size={16} /></button>
        <span class="num dim zl">{zoom.toFixed(1)}x</span>
        <button class="icon-btn" onclick={() => (zoom = Math.min(20, zoom * 1.5))} aria-label={t.session.zoomIn}><Icon name="plus" size={16} /></button>
      </div>
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <div class="track" bind:clientWidth={tlWidth} onclick={trackClick} role="presentation">
        <div class="lanes" style="width:{Math.max(tlWidth, total * pxPerSec + 24)}px">
          {#each project.clips as c, i (c.id)}
            {@const r = recById.get(c.recordingId)}
            {@const w = clipLen(c) * pxPerSec}
            {#if dropIndex === i}<span class="drop"></span>{/if}
            <div
              class="block"
              class:sel={selected === c.id}
              class:missing={!r}
              style="width:{w}px;{sprites[c.recordingId] && r ? `background-image:url('${sprites[c.recordingId]}');background-size:${(r.duration * pxPerSec)}px 100%;background-position:-${c.start * pxPerSec}px 0` : ''}"
              role="button"
              tabindex="0"
              aria-label={t.editor.clipAria(i + 1, r ? displayTitle(r.title) : t.editor.missingSourceLower)}
              onpointerdown={(e) => blockDown(e, c, 'move')}
              onpointermove={blockMove}
              onpointerup={blockUp}
              onclick={(e) => e.stopPropagation()}
            >
              <span class="b-label">{r ? displayTitle(r.title) : t.editor.missingSource} <span class="num">{clock(clipLen(c))}</span></span>
              <span class="edge l" role="presentation" onpointerdown={(e) => blockDown(e, c, 'l')} onpointermove={blockMove} onpointerup={blockUp}></span>
              <span class="edge r" role="presentation" onpointerdown={(e) => blockDown(e, c, 'r')} onpointermove={blockMove} onpointerup={blockUp}></span>
            </div>
          {/each}
          {#if dropIndex === project.clips.length}<span class="drop"></span>{/if}
          <span class="ph" style="left:{12 + playhead * pxPerSec}px"></span>
        </div>
      </div>
    </div>

    {#if job}
      <div class="modal-back">
        <div class="modal" role="dialog" aria-label={t.editor.exportVideo}>
          <div class="exp-title">{t.editor.exportVideo}</div>
          <div class="pbar"><span style="width:{Math.round(job.progress * 100)}%"></span></div>
          <div class="pinfo">
            <span>{job.state === 'running' ? t.editor.encoding : job.state === 'done' ? t.common.done : job.state === 'error' ? t.editor.exportFailed : t.common.cancelled}</span>
            <span class="num">{Math.round(job.progress * 100)}%</span>
          </div>
          {#if job.state === 'error'}<p class="err">{job.message}</p>{/if}
          <div class="mbtns">
            {#if job.state === 'running'}
              <button class="btn ghost" onclick={() => api.cancelJob(job.id)}>{t.common.cancel}</button>
            {:else}
              <button class="btn ghost" onclick={() => (exportJob = null)}>{t.common.close}</button>
              {#if job.state === 'done' && job.resultId}
                <button class="btn primary" onclick={() => { const r = recById.get(job.resultId!); exportJob = null; if (r) go({ name: 'session', sessionId: r.sessionId, recordingId: r.id }); }}>{t.editor.viewInFavorites}</button>
              {/if}
            {/if}
          </div>
        </div>
      </div>
    {/if}
  </section>
{/if}

<style>
  .list {
    height: 100%;
    overflow: auto;
    padding: 0 24px 32px;
  }
  .lhead {
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: 64px;
  }
  h1 {
    font-size: 20px;
  }
  .projects {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
    gap: 12px;
  }
  .pcard {
    position: relative;
    background: var(--panel);
    border: 1px solid var(--seam);
    border-radius: var(--r-lg);
  }
  .pcard:hover {
    background: var(--panel-2);
  }
  .p-open {
    display: grid;
    gap: 6px;
    padding: 8px 8px 12px;
    text-align: left;
    width: 100%;
  }
  .p-thumb {
    position: relative;
    aspect-ratio: 16 / 9;
    border-radius: var(--r-md);
    overflow: hidden;
    background: var(--bg-sunk);
  }
  .p-thumb img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  .p-aspect {
    position: absolute;
    left: 6px;
    bottom: 6px;
    padding: 1px 6px;
    font-size: 11px;
    background: var(--scrim);
    border-radius: var(--r-sm);
  }
  .p-name {
    font-family: var(--font-score);
    font-weight: 600;
    font-size: 15px;
    padding: 0 4px;
  }
  .p-sub {
    color: var(--text-3);
    font-size: 12px;
    padding: 0 4px;
  }
  .p-del {
    position: absolute;
    top: 14px;
    right: 14px;
    background: var(--scrim);
  }

  .ed {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }
  .ehead {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 16px 0 12px;
    height: 56px;
    border-bottom: 1px solid var(--seam);
    flex: none;
  }
  .crumb {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 16px;
  }
  .dim {
    color: var(--text-3);
    font-weight: 400;
  }
  .grow {
    flex: 1;
  }
  .hist {
    font-size: 12px;
    color: var(--text-3);
    min-width: 16px;
    text-align: center;
  }
  .exp {
    display: grid;
    gap: 10px;
  }
  .exp-title {
    font-family: var(--font-score);
    font-weight: 600;
    font-size: 15px;
  }
  .exp-note {
    font-size: 12.5px;
    color: var(--text-2);
  }
  .work {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: 280px minmax(0, 1fr);
  }
  .panel {
    display: flex;
    flex-direction: column;
    border-right: 1px solid var(--seam);
    background: var(--panel);
    min-height: 0;
  }
  .ptabs {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 2px;
    padding: 8px;
  }
  .ptabs button {
    height: 30px;
    display: grid;
    place-items: center;
    border-radius: var(--r-sm);
    color: var(--text-3);
  }
  .ptabs button:hover {
    color: var(--text);
  }
  .ptabs button.on {
    background: var(--panel-3);
    color: var(--you);
  }
  .pbody {
    display: grid;
    align-content: start;
    gap: 12px;
    padding: 6px 14px 16px;
    overflow: auto;
  }
  .ptitle {
    font-family: var(--font-score);
    font-weight: 600;
    font-size: 12.5px;
    color: var(--text-3);
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }
  .media,
  .imp {
    display: grid;
    grid-template-columns: 72px 1fr auto;
    align-items: center;
    gap: 10px;
    text-align: left;
  }
  .media.missing {
    display: block;
    color: var(--warn);
    font-size: 12.5px;
  }
  .m-thumb {
    position: relative;
    aspect-ratio: 16 / 9;
    border-radius: var(--r-sm);
    overflow: hidden;
    background: var(--bg-sunk);
  }
  .m-thumb img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  .m-thumb .num {
    position: absolute;
    right: 3px;
    bottom: 2px;
    font-size: 10px;
    padding: 0 3px;
    background: var(--scrim);
    border-radius: 2px;
  }
  .m-thumb.sm {
    width: 56px;
  }
  .m-name {
    font-size: 12.5px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .import {
    display: grid;
    gap: 4px;
    max-height: 360px;
    overflow: auto;
  }
  .imp {
    grid-template-columns: 56px 1fr 18px;
    padding: 4px;
    border-radius: var(--r-md);
  }
  .imp:hover {
    background: var(--panel-2);
  }
  .imp.on {
    color: var(--you);
  }
  .wide {
    width: 100%;
  }
  .prop {
    display: grid;
    gap: 6px;
    font-size: 12.5px;
    color: var(--text-2);
  }
  .prop.row {
    grid-template-columns: 1fr auto;
    align-items: center;
  }
  .prop small {
    display: block;
    color: var(--text-3);
    font-size: 11.5px;
  }
  .info {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 4px 12px;
    margin: 4px 0 0;
    font-size: 12.5px;
  }
  .info dt {
    color: var(--text-3);
  }
  .info dd {
    margin: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .preview {
    display: flex;
    flex-direction: column;
    min-height: 0;
    padding: 14px 16px 8px;
    gap: 8px;
  }
  .frame-wrap {
    flex: 1;
    min-height: 0;
    display: grid;
    place-items: center;
    background: var(--bg-sunk);
    border-radius: var(--r-lg);
    container-type: size;
  }
  .frame {
    max-width: 100cqw;
    max-height: 100cqh;
    height: 100cqh;
    background: var(--media-bg);
    overflow: hidden;
    display: grid;
    place-items: center;
  }
  .frame video {
    width: 100%;
    height: 100%;
  }
  .noclips {
    color: var(--text-3);
    padding: 24px;
    text-align: center;
  }
  .pctl {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .ptime {
    font-size: 14px;
  }
  .lbl {
    font-size: 12px;
  }
  .tl {
    flex: none;
    border-top: 1px solid var(--seam);
    background: var(--panel);
  }
  .tl-tools {
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 6px 12px;
  }
  .zl {
    font-size: 12px;
    min-width: 34px;
    text-align: center;
  }
  .track {
    overflow-x: auto;
    padding: 4px 0 12px;
    cursor: text;
  }
  .lanes {
    position: relative;
    display: flex;
    padding: 0 12px;
    height: 74px;
  }
  .block {
    position: relative;
    flex: none;
    height: 100%;
    border-radius: var(--r-md);
    background-color: var(--panel-3);
    background-repeat: repeat-x;
    box-shadow: inset 0 0 0 1px var(--seam-strong);
    cursor: grab;
    overflow: hidden;
    margin-right: 2px;
  }
  .block.sel {
    box-shadow: inset 0 0 0 2px var(--you);
  }
  .block.missing {
    background: color-mix(in oklab, var(--warn) 14%, var(--panel-2));
    box-shadow: inset 0 0 0 1px color-mix(in oklab, var(--warn) 55%, transparent);
  }
  .b-label {
    position: absolute;
    left: 6px;
    top: 5px;
    right: 6px;
    padding: 1px 6px;
    font-size: 11.5px;
    background: var(--scrim);
    border-radius: var(--r-sm);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    width: fit-content;
    max-width: calc(100% - 12px);
  }
  .edge {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 8px;
    cursor: ew-resize;
  }
  .edge.l {
    left: 0;
  }
  .edge.r {
    right: 0;
  }
  .sel .edge {
    background: color-mix(in oklab, var(--you) 55%, transparent);
  }
  .drop {
    flex: none;
    width: 3px;
    margin: 0 1px;
    background: var(--you);
    border-radius: 2px;
  }
  .ph {
    position: absolute;
    top: -4px;
    bottom: -4px;
    width: 2px;
    margin-left: -1px;
    background: var(--text);
    pointer-events: none;
  }
  .modal-back {
    position: fixed;
    inset: 0;
    background: var(--backdrop);
    display: grid;
    place-items: center;
    z-index: 80;
  }
  .modal {
    width: 380px;
    display: grid;
    gap: 12px;
    padding: 20px;
    background: var(--panel);
    border: 1px solid var(--seam-strong);
    border-radius: var(--r-lg);
    box-shadow: var(--shadow-pop);
  }
  .pbar {
    height: 6px;
    border-radius: 3px;
    background: var(--panel-3);
    overflow: hidden;
  }
  .pbar span {
    display: block;
    height: 100%;
    background: var(--you);
    transition: width 200ms;
  }
  .pinfo {
    display: flex;
    justify-content: space-between;
    color: var(--text-2);
    font-size: 12.5px;
  }
  .err {
    color: var(--death);
    font-size: 12.5px;
    max-height: 120px;
    overflow: auto;
    user-select: text;
  }
  .mbtns {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
</style>
