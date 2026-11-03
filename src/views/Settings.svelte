<script lang="ts">
  import { open as openDialog } from '@tauri-apps/plugin-dialog';
  import HotkeyInput from '../components/HotkeyInput.svelte';
  import Appearance from './Appearance.svelte';
  import Icon from '../components/Icon.svelte';
  import Segmented from '../components/Segmented.svelte';
  import Select from '../components/Select.svelte';
  import Slider from '../components/Slider.svelte';
  import Toggle from '../components/Toggle.svelte';
  import { api } from '../lib/api';
  import { app, checkUpdate, errorText, go, installUpdate, releaseUrl, setLanguage, toast, updateBlocked, updateSettings } from '../lib/state.svelte';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { meta, SETTING_KINDS } from '../lib/events';
  import { bytes, gameName } from '../lib/format.svelte';
  import { i18n, intlLocale, LANGUAGES, t } from '../lib/i18n.svelte';
  import type { Devices, Settings } from '../lib/types';

  let { page }: { page: string } = $props();
  const s = $derived(app.settings!);
  const deletedRecs = $derived(app.library.recordings.filter((r) => r.deletedAt));
  const deletedCount = $derived(deletedRecs.length);
  const deletedBytes = $derived(deletedRecs.reduce((a, r) => a + r.size, 0));
  const set = updateSettings;

  let devices = $state<Devices | null>(null);
  let devicesError = $state('');
  $effect(() => {
    if (!devices && (page === 'capture' || page === 'screen')) {
      api.listDevices().then((d) => (devices = d)).catch((e) => (devicesError = errorText(e)));
    }
  });

  const NAV = [
    { id: 'games', icon: 'grid' },
    { id: 'capture', icon: 'record' },
    { id: 'screen', icon: 'monitor' },
    { id: 'storage', icon: 'disk' },
    { id: 'notifications', icon: 'flag' },
    { id: 'appearance', icon: 'palette' },
    { id: 'language', icon: 'globe' },
    { id: 'updates', icon: 'download' },
  ];

  const u = $derived(app.update);
  /** "5 minutes ago" in the UI language. */
  function ago(ms: number): string {
    const min = Math.round((Date.now() - ms) / 60000);
    const rtf = new Intl.RelativeTimeFormat(intlLocale(), { numeric: 'auto' });
    if (min < 60) return rtf.format(-min, 'minute');
    const h = Math.round(min / 60);
    return h < 48 ? rtf.format(-h, 'hour') : rtf.format(-Math.round(h / 24), 'day');
  }
  const updateLine = $derived.by(() => {
    const U = t.update;
    if (u.phase === 'checking') return { color: 'var(--objective)', text: U.checking };
    if (u.phase === 'downloading' && u.version) return { color: 'var(--you)', text: u.progress == null ? U.downloadingNoPct(u.version) : U.downloading(u.version, Math.round(u.progress * 100)) };
    if (u.phase === 'installing') return { color: 'var(--you)', text: U.installing(u.version ?? '') };
    if (u.version) return { color: 'var(--you)', text: U.available(u.version) };
    if (u.phase === 'error') return { color: 'var(--warn)', text: U.failed };
    if (u.checkedAt) return { color: 'var(--win)', text: U.upToDate(ago(u.checkedAt)) };
    return { color: 'var(--text-3)', text: U.notChecked };
  });

  const PRESETS = {
    low: { resolution: 480, fps: 20, bitrateKbps: 1500 },
    medium: { resolution: 720, fps: 30, bitrateKbps: 3500 },
    high: { resolution: 1080, fps: 60, bitrateKbps: 8000 },
  } as const;
  const presetText = $derived({
    low: [t.settings.video.low, t.settings.video.lowHint],
    medium: [t.settings.video.medium, t.settings.video.mediumHint],
    high: [t.settings.video.high, t.settings.video.highHint],
  });
  function applyPreset(p: keyof typeof PRESETS | 'custom') {
    set((x) => {
      x.video.preset = p;
      if (p !== 'custom') Object.assign(x.video, { resolution: PRESETS[p].resolution, fps: PRESETS[p].fps, bitrateKbps: PRESETS[p].bitrateKbps });
    });
  }
  function setVideo<K extends keyof Settings['video']>(k: K, v: Settings['video'][K]) {
    set((x) => {
      x.video[k] = v;
      if (k === 'resolution' || k === 'fps' || k === 'bitrateKbps') x.video.preset = 'custom';
    });
  }
  // Rough size per hour at the bitrate ceiling (average is usually well below with quality-based encoding).
  const gbPerHour = $derived(((s.video.bitrateKbps * 1000) / 8) * 3600 / 1024 ** 3);

  const MODES = $derived([
    { id: 'manual', label: t.settings.lol.modeManual, hint: t.settings.lol.modeManualHint },
    { id: 'highlights', label: t.settings.lol.modeHighlights, hint: t.settings.lol.modeHighlightsHint },
    { id: 'session', label: t.settings.lol.modeSession, hint: t.settings.lol.modeSessionHint },
    { id: 'match', label: t.settings.lol.modeMatch, hint: t.settings.lol.modeMatchHint },
  ] as const);

  async function pickFolder(): Promise<string | null> {
    const r = await openDialog({ directory: true, multiple: false, defaultPath: s.storage.mediaFolder });
    return typeof r === 'string' ? r : null;
  }
  let moveAsk = $state<string | null>(null);
  async function changeFolder() {
    const p = await pickFolder();
    if (p && p !== s.storage.mediaFolder) moveAsk = p;
  }
  async function confirmMove(move: boolean) {
    const p = moveAsk!;
    moveAsk = null;
    try {
      await api.changeMediaFolder(p, move);
      toast(move ? t.settings.storage.moving : t.settings.storage.changed, 'info');
    } catch (e) {
      toast(errorText(e), 'error');
    }
  }
  async function backup() {
    const p = await pickFolder();
    if (!p) return;
    try {
      await api.createBackup(p);
      toast(t.settings.storage.backupStarted, 'info');
    } catch (e) {
      toast(errorText(e), 'error');
    }
  }
  const SECONDS = $derived([0, 5, 10, 15, 20, 30, 45, 60].map((v) => ({ value: v, label: t.settings.seconds(v) })));
  const NOTIFY = ['sessionStart', 'matchStart', 'matchEnd', 'manualCapture', 'gameEventCapture'] as const;
  const languageName = (l: string) => LANGUAGES.find((x) => x.value === l)?.label ?? l;
</script>

<div class="settings">
  <nav class="snav" aria-label={t.settings.sections}>
    <div class="sgroup">{t.settings.general}</div>
    {#each NAV as n (n.id)}
      <button class:on={page === n.id} onclick={() => go({ name: 'settings', page: n.id })}><Icon name={n.icon} size={16} />{t.settings.nav[n.id]}</button>
    {/each}
    <div class="sgroup">{t.settings.myGames}</div>
    <button class:on={page === 'lol'} onclick={() => go({ name: 'settings', page: 'lol' })}><span class="lol-mark">L</span>{gameName('League of Legends')}</button>
    <div class="grow"></div>
    <div class="ver"><span class="num">v{app.version}</span><button class="link" onclick={() => api.quitApp()}>{t.settings.quit}</button></div>
  </nav>

  <div class="content">
    {#if page === 'games'}
      <h1>{t.settings.games.title}</h1>
      <p class="lead">{t.settings.games.lead}</p>
      <div class="game-card">
        <span class="lol-mark big">L</span>
        <div class="gc-text"><strong>{gameName('League of Legends')}</strong><span>{t.settings.games.lolHint}</span></div>
        <button class="btn ghost sm" onclick={() => go({ name: 'settings', page: 'lol' })}>{t.settings.games.gameSettings}</button>
        <Toggle label={t.settings.games.record} checked={s.games.lol.enabled} onchange={(v) => set((x) => (x.games.lol.enabled = v))} />
      </div>
      <p class="note">{t.settings.games.more}</p>
    {:else if page === 'capture'}
      {@const C = t.settings.capture}
      {@const V = t.settings.video}
      {@const A = t.settings.audio}
      {@const W = t.settings.webcam}
      <h1>{C.title}</h1>
      <p class="lead">{C.lead}</p>
      <section class="sec">
        <div class="row"><span>{C.ownWindows}</span><Toggle label={C.ownWindows} checked={s.general.captureOwnWindows} onchange={(v) => set((x) => (x.general.captureOwnWindows = v))} /></div>
        <div class="row"><span>{C.notifications}</span><Toggle label={C.notifications} checked={s.general.captureNotifications} onchange={(v) => set((x) => (x.general.captureNotifications = v))} /></div>
        <div class="row"><span>{C.discord}</span><Toggle label={C.discord} checked={s.general.discordPresence} onchange={(v) => set((x) => (x.general.discordPresence = v))} /></div>
        {#if s.general.discordPresence}
          <div class="row sub"><span>{C.discordId}<small>{C.discordIdHint}</small></span><input class="field id" value={s.general.discordAppId} placeholder={C.discordIdPlaceholder} aria-label={C.discordId} onchange={(e) => set((x) => (x.general.discordAppId = (e.currentTarget as HTMLInputElement).value.trim()))} /></div>
        {/if}
        <div class="row"><span>{C.watermark}</span><Toggle label={C.watermark} checked={s.general.watermark} onchange={(v) => set((x) => (x.general.watermark = v))} /></div>
        <div class="row"><span>{C.startWithWindows}<small>{C.startWithWindowsHint}</small></span><Toggle label={C.startWithWindows} checked={s.general.startWithWindows} onchange={(v) => set((x) => (x.general.startWithWindows = v))} /></div>
      </section>

      <section class="sec">
        <h2>{V.title}</h2>
        <p class="sub-lead">{V.lead}</p>
        <div class="cards4">
          {#each Object.keys(PRESETS) as id (id)}
            {@const [label, hint] = presetText[id as keyof typeof PRESETS]}
            <button class="mcard" class:on={s.video.preset === id} onclick={() => applyPreset(id as keyof typeof PRESETS)}><span class="radio"></span><strong>{label}</strong><small>{hint}</small></button>
          {/each}
          <button class="mcard" class:on={s.video.preset === 'custom'} onclick={() => applyPreset('custom')}><span class="radio"></span><strong>{V.custom}</strong><small>{V.customHint}</small></button>
        </div>
        <div class="row"><span>{V.resolution}</span>
          <div class="ctl"><Select label={V.resolution} value={s.video.resolution} onchange={(v) => setVideo('resolution', v)} options={[{ value: 480, label: '480p' }, { value: 720, label: '720p (HD)' }, { value: 1080, label: '1080p (Full HD)' }, { value: 1440, label: '1440p (QHD)' }, { value: 2160, label: '2160p (4K)' }, { value: 0, label: V.native }]} /></div>
        </div>
        <div class="row"><span>{V.bitrate}<small>{V.bitrateHint(gbPerHour.toFixed(1))}</small></span>
          <div class="ctl"><Select label={V.bitrateAria} value={s.video.bitrateKbps} onchange={(v) => setVideo('bitrateKbps', v)} options={[1500, 2500, 3500, 5000, 6000, 8000, 10000, 12000, 16000, 20000, 30000].map((b) => ({ value: b, label: String(b) }))} /></div>
        </div>
        <div class="row"><span>{V.fps}</span>
          <div class="ctl wide"><Segmented label={V.fpsAria} value={s.video.fps} onchange={(v) => setVideo('fps', v)} options={[10, 20, 30, 60, 90, 120].map((f) => ({ value: f, label: String(f) }))} /></div>
        </div>
        <details class="adv">
          <summary><Icon name="chevron-right" size={14} />{V.advanced}</summary>
          <div class="row"><span>{V.force169}</span><Toggle label={V.force169} checked={s.video.force169} onchange={(v) => setVideo('force169', v)} /></div>
          <div class="row" class:off={!s.video.force169}><span>{V.crop}</span>
            <div class="ctl"><Segmented label={V.crop} disabled={!s.video.force169} value={s.video.cropMode} onchange={(v) => setVideo('cropMode', v)} options={[{ value: 'fit', label: V.fit }, { value: 'stretch', label: V.stretch }]} /></div>
          </div>
          <div class="row"><span>{V.encoder}<small>{devices ? '' : V.checkingGpu}</small></span>
            <div class="ctl"><Select label={V.encoder} value={s.video.encoder} onchange={(v) => setVideo('encoder', v)} options={(devices?.encoders ?? [{ id: s.video.encoder, label: s.video.encoder, available: true }]).map((e) => ({ value: e.id, label: e.available ? e.label : V.notAvailable(e.label), disabled: !e.available }))} /></div>
          </div>
          <div class="row"><span>{V.preset}</span>
            <div class="ctl"><Select label={V.presetAria} value={s.video.encoderPreset} onchange={(v) => setVideo('encoderPreset', v)} options={[{ value: 'default', label: V.presetDefault }, { value: 'quality', label: V.presetQuality }, { value: 'performance', label: V.presetPerformance }, { value: 'lowlatency', label: V.presetLowLatency }, { value: 'perf_lowlatency', label: V.presetPerfLowLatency }, { value: 'quality_lowlatency', label: V.presetQualityLowLatency }]} /></div>
          </div>
          <div class="row"><span>{V.buffer}<small>{V.bufferHint}</small></span>
            <div class="ctl"><Segmented label={V.buffer} value={s.video.buffer} onchange={(v) => setVideo('buffer', v)} options={[{ value: 'ram', label: V.ram }, { value: 'disk', label: V.disk }]} /></div>
          </div>
        </details>
      </section>

      <section class="sec">
        <h2>{A.title}</h2>
        {#if devicesError}<p class="note err">{A.devicesError(devicesError)}</p>{/if}
        <div class="row"><span>{A.system}</span><Toggle label={A.system} checked={s.audio.systemEnabled} onchange={(v) => set((x) => (x.audio.systemEnabled = v))} /></div>
        <div class="row" class:off={!s.audio.systemEnabled}><span>{A.output}</span>
          <div class="ctl"><Select label={A.output} disabled={!s.audio.systemEnabled} value={s.audio.systemDevice} onchange={(v) => set((x) => (x.audio.systemDevice = v))} options={[{ value: 'default', label: A.systemDefault }, ...(devices?.audio.outputs ?? []).map((d) => ({ value: d.id, label: d.name }))]} /></div>
        </div>
        <div class="row" class:off={!s.audio.systemEnabled}><span>{A.volume}</span><div class="ctl"><Slider label={A.systemVolume} disabled={!s.audio.systemEnabled} value={s.audio.systemVolume} onchange={(v) => set((x) => (x.audio.systemVolume = v))} /></div></div>
        <div class="row gap"><span>{A.mic}</span><Toggle label={A.mic} checked={s.audio.micEnabled} onchange={(v) => set((x) => (x.audio.micEnabled = v))} /></div>
        <div class="row" class:off={!s.audio.micEnabled}><span>{A.input}</span>
          <div class="ctl"><Select label={A.input} disabled={!s.audio.micEnabled} value={s.audio.micDevice} onchange={(v) => set((x) => (x.audio.micDevice = v))} options={[{ value: 'default', label: A.systemDefault }, ...(devices?.audio.inputs ?? []).map((d) => ({ value: d.id, label: d.name }))]} /></div>
        </div>
        <div class="row" class:off={!s.audio.micEnabled}><span>{A.volume}</span><div class="ctl"><Slider label={A.micVolume} disabled={!s.audio.micEnabled} value={s.audio.micVolume} onchange={(v) => set((x) => (x.audio.micVolume = v))} /></div></div>
        <details class="adv">
          <summary><Icon name="chevron-right" size={14} />{V.advanced}</summary>
          <div class="row"><span>{A.gameOnly}<small>{A.gameOnlyHint}</small></span><Toggle label={A.gameOnly} checked={s.audio.gameOnly} onchange={(v) => set((x) => (x.audio.gameOnly = v))} /></div>
          <div class="row"><span>{A.mono}</span><Toggle label={A.mono} checked={s.audio.micMono} onchange={(v) => set((x) => (x.audio.micMono = v))} /></div>
        </details>
      </section>

      <section class="sec">
        <h2>{W.title}</h2>
        <div class="row"><span>{W.capture}<small>{W.captureHint}</small></span><Toggle label={W.capture} checked={s.webcam.enabled} onchange={(v) => set((x) => (x.webcam.enabled = v))} /></div>
        <div class="row" class:off={!s.webcam.enabled}><span>{W.device}</span>
          <div class="ctl"><Select label={W.deviceAria} disabled={!s.webcam.enabled} value={s.webcam.device} onchange={(v) => set((x) => (x.webcam.device = v))} options={[{ value: '', label: devices?.webcams.length ? W.choose : W.none }, ...(devices?.webcams ?? []).map((w) => ({ value: w, label: w }))]} /></div>
        </div>
        <div class="row" class:off={!s.webcam.enabled}><span>{W.mode}<small>{W.modeHint(s.games.lol.hotkeys.toggleWebcam)}</small></span>
          <div class="ctl"><Segmented label={W.modeAria} disabled={!s.webcam.enabled} value={s.webcam.mode} onchange={(v) => set((x) => (x.webcam.mode = v))} options={[{ value: 'always', label: W.always }, { value: 'visible', label: W.visible }]} /></div>
        </div>
      </section>
    {:else if page === 'screen'}
      {@const R = t.settings.screen}
      <h1>{R.title}</h1>
      <p class="lead">{R.lead}</p>
      <section class="sec">
        <div class="row"><span>{R.cursor}</span><Toggle label={R.cursor} checked={s.screenRecorder.captureCursor} onchange={(v) => set((x) => (x.screenRecorder.captureCursor = v))} /></div>
        <div class="row"><span>{R.border}<small>{R.borderHint}</small></span><Toggle label={R.border} checked={s.screenRecorder.showBorder} onchange={(v) => set((x) => (x.screenRecorder.showBorder = v))} /></div>
        <div class="row"><span>{R.display}</span>
          <div class="ctl"><Select label={R.display} value={s.screenRecorder.display} onchange={(v) => set((x) => (x.screenRecorder.display = v))} options={(devices?.monitors ?? []).map((m) => ({ value: m.index, label: R.displayName(m.index + 1, m.width, m.height, m.primary) }))} /></div>
        </div>
      </section>
      <div class="callout">
        <span><strong>{R.cantFind}</strong> {R.cantFindText}</span>
        <button class="btn primary sm" onclick={() => api.toggleScreenRecorder()}><Icon name={app.status.screenRecording ? 'stop' : 'record'} size={14} />{app.status.screenRecording ? R.stop : R.start}</button>
      </div>
      <section class="sec">
        <h2>{R.hotkeys}</h2>
        <div class="row"><span>{R.toggle}</span><HotkeyInput label={R.toggleAria} value={s.screenRecorder.hotkeyToggle} onchange={(v) => set((x) => (x.screenRecorder.hotkeyToggle = v))} /></div>
        <div class="row gap"><span>{R.bookmark}</span><HotkeyInput label={R.bookmark} value={s.screenRecorder.hotkeyBookmark} onchange={(v) => set((x) => (x.screenRecorder.hotkeyBookmark = v))} /></div>
        <div class="row"><span>{R.before}</span><div class="ctl"><Select label={R.bookmarkBefore} value={s.screenRecorder.bookmarkBefore} onchange={(v) => set((x) => (x.screenRecorder.bookmarkBefore = v))} options={SECONDS} /></div></div>
        <div class="row"><span>{R.after}</span><div class="ctl"><Select label={R.bookmarkAfter} value={s.screenRecorder.bookmarkAfter} onchange={(v) => set((x) => (x.screenRecorder.bookmarkAfter = v))} options={SECONDS} /></div></div>
      </section>
    {:else if page === 'storage'}
      {@const St = t.settings.storage}
      <h1>{St.title}</h1>
      <p class="lead">{St.lead}</p>
      <div class="usage">
        <div><span class="u-num num">{bytes(app.disk?.library ?? 0)}</span><span class="u-lbl">{St.usedByRecordings}</span></div>
        <div><span class="u-num num">{bytes(app.disk?.free ?? 0)}</span><span class="u-lbl">{St.freeOnDrive}</span></div>
        <div><span class="u-num num">{deletedCount}</span><span class="u-lbl">{St.inDeleted(bytes(deletedBytes))}</span></div>
      </div>
      <section class="sec">
        <h2>{St.keep}</h2>
        <p class="sub-lead">{St.keepLead}</p>
        <div class="row">
          <span>{St.recent}<small>{St.recentHint}</small></span>
          <div class="pair">
            <div class="ctl-sm"><Select label={St.keepFor} disabled={!s.storage.ageLimitEnabled} value={s.storage.ageLimitMonths} onchange={(v) => set((x) => (x.storage.ageLimitMonths = v))} options={[1, 2, 3, 6, 9, 12, 18, 24].map((m) => ({ value: m, label: St.months(m) }))} /></div>
            <Toggle label={St.recent} checked={s.storage.ageLimitEnabled} onchange={(v) => set((x) => (x.storage.ageLimitEnabled = v))} />
          </div>
        </div>
        <div class="row">
          <span>{St.limit}<small>{St.limitHint}</small></span>
          <div class="pair">
            <div class="unit"><input class="field num" type="number" min="5" disabled={s.storage.mode !== 'auto'} value={s.storage.limitGb} onchange={(e) => set((x) => (x.storage.limitGb = Math.max(5, Number((e.currentTarget as HTMLInputElement).value) || 5)))} aria-label={St.limitAria} /><span>GB</span></div>
            <Toggle label={St.limit} checked={s.storage.mode === 'auto'} onchange={(v) => set((x) => (x.storage.mode = v ? 'auto' : 'manual'))} />
          </div>
        </div>
        <div class="row gap">
          <span>{St.emptyAfter}<small>{St.emptyAfterHint}</small></span>
          <div class="ctl-sm"><Select label={St.emptyAria} value={s.storage.keepDeletedDays} onchange={(v) => set((x) => (x.storage.keepDeletedDays = v))} options={[0, 1, 3, 7, 14, 30, 60].map((v) => ({ value: v, label: v === 0 ? St.rightAway : St.days(v) }))} /></div>
        </div>
        {#if !s.storage.ageLimitEnabled && s.storage.mode !== 'auto'}<p class="note">{St.nothingAuto}</p>{/if}
      </section>
      <section class="sec">
        <h2>{St.mediaFolder}</h2>
        <div class="path-row">
          <input class="field path" readonly value={s.storage.mediaFolder} aria-label={St.mediaFolder} />
          <button class="btn sm" onclick={() => api.openMediaFolder()}>{t.common.open}</button>
          <button class="btn sm" onclick={changeFolder}>{St.change}</button>
        </div>
        {#if moveAsk}
          <div class="callout">
            <span>{St.moveAsk(moveAsk)}</span>
            <div class="btns"><button class="btn primary sm" onclick={() => confirmMove(true)}>{St.moveThem}</button><button class="btn sm" onclick={() => confirmMove(false)}>{St.startFresh}</button><button class="btn ghost sm" onclick={() => (moveAsk = null)}>{t.common.cancel}</button></div>
          </div>
        {/if}
        <div class="callout">
          <span>{St.backupNote}</span>
          <button class="btn primary sm" onclick={backup}>{St.createBackup}</button>
        </div>
      </section>
    {:else if page === 'appearance'}
      <Appearance />
    {:else if page === 'language'}
      {@const L = t.settings.language}
      <h1>{L.title}</h1>
      <p class="lead">{L.lead}</p>
      <section class="sec">
        <div class="cards3" role="radiogroup" aria-label={L.title}>
          <button class="mcard" role="radio" aria-checked={s.general.language === 'system'} class:on={s.general.language === 'system'} onclick={() => setLanguage('system')}>
            <span class="radio"></span><strong>{L.system}</strong><small>{L.systemHint(languageName(i18n.system))}</small>
          </button>
          {#each LANGUAGES as l (l.value)}
            <button class="mcard" role="radio" aria-checked={s.general.language === l.value} class:on={s.general.language === l.value} onclick={() => setLanguage(l.value)} lang={l.value === 'zh-CN' ? 'zh-Hans' : 'en'}>
              <span class="radio"></span><strong>{l.label}</strong>{#if L.names[l.value] !== l.label}<small>{L.names[l.value]}</small>{/if}
            </button>
          {/each}
        </div>
        <p class="note">{L.note}</p>
      </section>
    {:else if page === 'updates'}
      {@const U = t.update}
      <h1>{U.title}</h1>
      <p class="lead">{U.lead}</p>
      <section class="sec">
        <div class="row">
          <span class="upd-line"><i style="background:{updateLine.color}"></i>{updateLine.text}<small>{U.version(app.version)}{u.version && updateBlocked() ? ` · ${U.afterGame}` : ''}</small></span>
          <div class="btns">
            {#if u.version}
              <button class="btn sm" onclick={() => openUrl(releaseUrl(u.version!))}>{U.whatsNew}</button>
              <button class="btn primary sm" disabled={updateBlocked() || u.phase === 'downloading' || u.phase === 'installing'} onclick={installUpdate}><Icon name="download" size={15} />{U.now}</button>
            {:else}
              <button class="btn sm" disabled={u.phase === 'checking'} onclick={checkUpdate}><Icon name="refresh" size={15} />{U.check}</button>
            {/if}
          </div>
        </div>
        {#if u.phase === 'error' && u.error}<p class="note err" title={u.error}>{t.update.unreachable}</p>{/if}
        <div class="row"><span>{U.auto}</span><Toggle label={U.auto} checked={s.general.checkUpdates} onchange={(v) => set((x) => (x.general.checkUpdates = v))} /></div>
        <p class="note">{U.note}</p>
      </section>
    {:else if page === 'notifications'}
      {@const N = t.settings.notifications}
      <h1>{N.title}</h1>
      <p class="lead">{N.lead}</p>
      <section class="sec">
        {#each NOTIFY as k (k)}
          <div class="row"><span>{N[k]}</span><Toggle label={N[k]} checked={s.notifications[k]} onchange={(v) => set((x) => (x.notifications[k] = v))} /></div>
        {/each}
      </section>
    {:else}
      {@const g = s.games.lol}
      {@const G = t.settings.lol}
      <h1>{gameName('League of Legends')}</h1>
      <p class="lead">{G.lead}</p>
      <section class="sec">
        <div class="row"><span>{G.openOnEnd}</span><Toggle label={G.openOnEnd} checked={g.openAppOnEnd} onchange={(v) => set((x) => (x.games.lol.openAppOnEnd = v))} /></div>
        <div class="row"><span>{G.dashboard}</span><Toggle label={G.dashboard} checked={g.dashboardOnSecondary} onchange={(v) => set((x) => (x.games.lol.dashboardOnSecondary = v))} /></div>
        <div class="row"><span>{G.apm}</span><Toggle label={G.apm} checked={g.collectApm} onchange={(v) => set((x) => (x.games.lol.collectApm = v))} /></div>
        <div class="row"><span>{G.cursor}</span><Toggle label={G.cursor} checked={g.captureCursor} onchange={(v) => set((x) => (x.games.lol.captureCursor = v))} /></div>
      </section>

      <section class="sec">
        <h2>{G.captureMode}</h2>
        <div class="cards2">
          {#each MODES as m (m.id)}
            <button class="mcard" class:on={g.captureMode === m.id} onclick={() => set((x) => (x.games.lol.captureMode = m.id))}><span class="radio"></span><strong>{m.label}</strong><small>{m.hint}</small></button>
          {/each}
        </div>
      </section>

      <section class="sec">
        <h2>{G.autoEvents}</h2>
        <p class="sub-lead">{g.captureMode === 'highlights' ? G.eachClip : g.captureMode === 'manual' ? G.markedManual : G.markedTimeline}</p>
        <div class="evgrid">
          {#each SETTING_KINDS as k (k)}
            {@const m = meta(k)}
            <button class="evtoggle" class:on={g.events[k]} style="--c:{m.color}" onclick={() => set((x) => (x.games.lol.events[k] = !x.games.lol.events[k]))} aria-pressed={g.events[k]}>
              <Icon name={m.icon} size={17} /><span>{m.label}</span>{#if g.events[k]}<Icon name="check" size={15} />{/if}
            </button>
          {/each}
        </div>
        {#if g.events.ultimate}<p class="note">{G.ultimateNote(g.hotkeys.ultimateKey)}</p>{/if}
        <details class="adv">
          <summary><Icon name="chevron-right" size={14} />{t.settings.video.advanced}</summary>
          <div class="timing-head"><span>{G.secondsBefore}</span><span></span><span>{G.secondsAfter}</span></div>
          {#each SETTING_KINDS as k (k)}
            {@const m = meta(k)}
            {@const tm = g.timing[k] ?? { before: 10, after: 5 }}
            <div class="timing" style="--c:{m.color}">
              <span class="tv num">{tm.before}{t.common.seconds}</span>
              <input type="range" class="rev" min="0" max="30" value={tm.before} aria-label={G.beforeAria(m.label)} oninput={(e) => set((x) => (x.games.lol.timing[k] = { ...tm, before: Number((e.currentTarget as HTMLInputElement).value) }))} />
              <span class="ticon" title={m.label}><Icon name={m.icon} size={16} /></span>
              <input type="range" min="0" max="30" value={tm.after} aria-label={G.afterAria(m.label)} oninput={(e) => set((x) => (x.games.lol.timing[k] = { ...tm, after: Number((e.currentTarget as HTMLInputElement).value) }))} />
              <span class="tv num">{tm.after}{t.common.seconds}</span>
            </div>
          {/each}
        </details>
      </section>

      <section class="sec">
        <h2>{G.hotkeys}</h2>
        <p class="sub-lead">{G.hotkeysLead}</p>
        <div class="row"><span>{G.bookmarking}</span><HotkeyInput label={G.bookmarkingAria} value={g.hotkeys.bookmarkToggle} onchange={(v) => set((x) => (x.games.lol.hotkeys.bookmarkToggle = v))} /></div>
        <div class="row"><span>{t.settings.screen.before}</span><div class="ctl"><Select label={G.bookmarkingBefore} value={g.hotkeys.bookmarkToggleBefore} onchange={(v) => set((x) => (x.games.lol.hotkeys.bookmarkToggleBefore = v))} options={SECONDS} /></div></div>
        <div class="row gap"><span>{t.settings.screen.bookmark}</span><HotkeyInput label={t.settings.screen.bookmark} value={g.hotkeys.placeBookmark} onchange={(v) => set((x) => (x.games.lol.hotkeys.placeBookmark = v))} /></div>
        <div class="row"><span>{t.settings.screen.before}</span><div class="ctl"><Select label={t.settings.screen.bookmarkBefore} value={g.hotkeys.placeBefore} onchange={(v) => set((x) => (x.games.lol.hotkeys.placeBefore = v))} options={SECONDS} /></div></div>
        <div class="row"><span>{t.settings.screen.after}</span><div class="ctl"><Select label={t.settings.screen.bookmarkAfter} value={g.hotkeys.placeAfter} onchange={(v) => set((x) => (x.games.lol.hotkeys.placeAfter = v))} options={SECONDS} /></div></div>
        <div class="row gap"><span>{G.screenshot}</span><HotkeyInput label={G.screenshot} value={g.hotkeys.screenshot} onchange={(v) => set((x) => (x.games.lol.hotkeys.screenshot = v))} /></div>
        <div class="row"><span>{G.toggleApp}</span><HotkeyInput label={G.toggleAppAria} value={g.hotkeys.toggleApp} onchange={(v) => set((x) => (x.games.lol.hotkeys.toggleApp = v))} /></div>
        <div class="row"><span>{G.toggleWebcam}</span><HotkeyInput label={G.toggleWebcamAria} value={g.hotkeys.toggleWebcam} onchange={(v) => set((x) => (x.games.lol.hotkeys.toggleWebcam = v))} /></div>
        <div class="row"><span>{G.toggleDashboard}</span><HotkeyInput label={G.toggleDashboardAria} value={g.hotkeys.toggleDashboard} onchange={(v) => set((x) => (x.games.lol.hotkeys.toggleDashboard = v))} /></div>
        <div class="row"><span>{G.dashboardLayout}</span><HotkeyInput label={G.dashboardLayout} value={g.hotkeys.dashboardLayout} onchange={(v) => set((x) => (x.games.lol.hotkeys.dashboardLayout = v))} /></div>
        <div class="row gap"><span>{G.ultimateKey}<small>{G.ultimateKeyHint}</small></span><HotkeyInput single label={G.ultimateKeyAria} value={g.hotkeys.ultimateKey} onchange={(v) => set((x) => (x.games.lol.hotkeys.ultimateKey = v))} /></div>
      </section>
    {/if}
  </div>
</div>

<style>
  .settings {
    display: grid;
    grid-template-columns: 236px minmax(0, 1fr);
    height: 100%;
    min-height: 0;
  }
  .snav {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 16px 10px;
    background: var(--panel);
    border-right: 1px solid var(--seam);
    overflow: auto;
  }
  .sgroup {
    font-family: var(--font-score);
    font-weight: 600;
    font-size: 11.5px;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--text-3);
    padding: 12px 10px 6px;
  }
  .snav button {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 34px;
    padding: 0 10px;
    border-radius: var(--r-md);
    color: var(--text-2);
    text-align: left;
  }
  .snav button:hover {
    background: var(--panel-2);
    color: var(--text);
  }
  .snav button.on {
    background: var(--panel-3);
    color: var(--text);
    box-shadow: inset 2px 0 0 var(--you);
  }
  .lol-mark {
    display: inline-grid;
    place-items: center;
    width: 18px;
    height: 18px;
    border-radius: 3px;
    background: var(--objective);
    color: var(--you-ink);
    font-family: var(--font-score);
    font-weight: 700;
    font-size: 12px;
  }
  .lol-mark.big {
    width: 40px;
    height: 40px;
    font-size: 22px;
    border-radius: var(--r-md);
  }
  .grow {
    flex: 1;
  }
  .ver {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 8px 10px 0;
    font-size: 12px;
    color: var(--text-3);
  }
  .link {
    color: var(--text-3);
    text-decoration: underline;
    text-underline-offset: 3px;
    font-size: 12px;
  }
  .link:hover {
    color: var(--death);
  }
  .content {
    overflow: auto;
    padding: 28px 40px 64px;
  }
  .content > * {
    max-width: 760px;
  }
  h1 {
    font-size: 24px;
  }
  .lead {
    color: var(--text-3);
    margin: 4px 0 20px;
  }
  h2 {
    font-size: 17px;
  }
  .sub-lead {
    color: var(--text-3);
    font-size: 12.5px;
    margin: 2px 0 10px;
  }
  .sec {
    padding: 22px 0 18px;
    border-top: 1px solid var(--seam);
  }
  .sec:first-of-type {
    border-top: 0;
    padding-top: 4px;
  }
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 24px;
    min-height: 44px;
    padding: 4px 0;
  }
  .row > span:first-child {
    display: grid;
    gap: 2px;
  }
  .row small {
    color: var(--text-3);
    font-size: 12px;
  }
  .row.gap {
    margin-top: 12px;
  }
  .row.off > span:first-child {
    color: var(--text-3);
  }
  .row.sub {
    padding-left: 16px;
  }
  .ctl {
    width: 320px;
    flex: none;
  }
  .ctl.wide {
    width: 420px;
  }
  .field.id {
    width: 240px;
  }
  .note {
    color: var(--text-3);
    font-size: 12.5px;
    margin-top: 10px;
  }
  .note.err {
    color: var(--death);
  }
  .game-card {
    display: grid;
    grid-template-columns: auto 1fr auto auto;
    align-items: center;
    gap: 16px;
    padding: 14px 16px;
    background: var(--panel);
    border: 1px solid var(--seam);
    border-radius: var(--r-lg);
  }
  .gc-text {
    display: grid;
  }
  .gc-text span {
    color: var(--text-3);
    font-size: 12.5px;
  }
  .cards4,
  .cards3,
  .cards2 {
    display: grid;
    gap: 8px;
    margin: 6px 0 14px;
  }
  .cards4 {
    grid-template-columns: repeat(4, 1fr);
  }
  .cards3 {
    grid-template-columns: repeat(3, 1fr);
  }
  .upd-line {
    position: relative;
    padding-left: 18px;
  }
  .upd-line i {
    position: absolute;
    left: 0;
    top: 6px;
    width: 8px;
    height: 8px;
    border-radius: 50%;
  }
  .cards2 {
    grid-template-columns: repeat(2, 1fr);
  }
  .mcard {
    position: relative;
    display: grid;
    gap: 2px;
    align-content: start;
    padding: 12px 14px 12px 36px;
    min-height: 64px;
    border-radius: var(--r-lg);
    background: var(--panel);
    border: 1px solid var(--seam);
    text-align: left;
  }
  .mcard:hover {
    background: var(--panel-2);
  }
  .mcard small {
    color: var(--text-3);
    font-size: 12px;
  }
  .mcard.on {
    background: var(--panel-2);
    border-color: var(--you);
  }
  .radio {
    position: absolute;
    left: 13px;
    top: 15px;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    box-shadow: inset 0 0 0 2px var(--seam-strong);
  }
  .on .radio {
    box-shadow: inset 0 0 0 4px var(--you);
  }
  details.adv {
    margin-top: 8px;
  }
  summary {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    cursor: pointer;
    color: var(--text-2);
    font-weight: 600;
    list-style: none;
    padding: 6px 0;
  }
  summary::-webkit-details-marker {
    display: none;
  }
  summary :global(svg) {
    transition: transform 150ms var(--ease-out);
  }
  details[open] summary :global(svg) {
    transform: rotate(90deg);
  }
  .callout {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 12px 16px;
    margin: 12px 0;
    background: var(--panel);
    border: 1px solid var(--seam);
    border-radius: var(--r-lg);
    color: var(--text-2);
  }
  .callout .btns {
    display: flex;
    gap: 6px;
  }
  .usage {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 1px;
    background: var(--seam);
    border: 1px solid var(--seam);
    border-radius: var(--r-lg);
    overflow: hidden;
    /* The first section after it drops its top padding (.sec:first-of-type). */
    margin-bottom: 20px;
  }
  .usage > div {
    display: grid;
    gap: 2px;
    padding: 12px 16px;
    background: var(--panel);
  }
  .u-num {
    font-size: 22px;
  }
  .u-lbl {
    font-size: 12px;
    color: var(--text-3);
  }
  .pair {
    display: flex;
    align-items: center;
    gap: 14px;
  }
  .ctl-sm {
    width: 160px;
  }
  .unit input:disabled {
    opacity: 0.45;
  }
  .unit {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--text-3);
  }
  .unit input {
    width: 90px;
  }
  .path-row {
    display: flex;
    gap: 8px;
    margin-top: 10px;
  }
  .path {
    flex: 1;
    color: var(--text-2);
  }
  .evgrid {
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    gap: 6px;
  }
  .evtoggle {
    display: grid;
    grid-template-columns: 20px 1fr 16px;
    align-items: center;
    gap: 10px;
    height: 40px;
    padding: 0 12px;
    border-radius: var(--r-md);
    background: var(--panel);
    border: 1px solid var(--seam);
    color: var(--text-3);
    text-align: left;
  }
  .evtoggle:hover {
    background: var(--panel-2);
  }
  .evtoggle.on {
    color: var(--text);
    border-color: color-mix(in oklab, var(--c) 55%, transparent);
  }
  .evtoggle > :global(svg:first-child) {
    color: var(--c);
  }
  .evtoggle:not(.on) > :global(svg:first-child) {
    opacity: 0.45;
  }
  .timing-head {
    display: grid;
    grid-template-columns: 1fr 40px 1fr;
    color: var(--text-3);
    font-size: 12px;
    margin: 6px 0;
  }
  .timing-head span:last-child {
    text-align: right;
  }
  .timing {
    display: grid;
    grid-template-columns: 34px 1fr 40px 1fr 34px;
    align-items: center;
    gap: 8px;
    height: 34px;
  }
  .tv {
    font-size: 12.5px;
    color: var(--text-2);
  }
  .tv:last-child {
    text-align: right;
  }
  .ticon {
    display: grid;
    place-items: center;
    height: 26px;
    border-radius: var(--r-sm);
    background: var(--panel-2);
    color: var(--c);
  }
  .timing input {
    appearance: none;
    background: transparent;
    height: 16px;
    cursor: pointer;
  }
  .timing input.rev {
    direction: rtl;
  }
  .timing input::-webkit-slider-runnable-track {
    height: 4px;
    border-radius: 2px;
    background: var(--panel-3);
  }
  .timing input::-webkit-slider-thumb {
    appearance: none;
    width: 12px;
    height: 12px;
    margin-top: -4px;
    border-radius: 2px;
    background: var(--c);
  }
</style>
