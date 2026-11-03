import { emit, listen } from '@tauri-apps/api/event';
import { api } from './api';
import { activeThemeId, applyAppearance, broadcastAppearance, DEFAULT_APPEARANCE, themeColors } from './appearance';
import { championsZh } from './format.svelte';
import { applyLanguage, i18n, t, type LanguageSetting } from './i18n.svelte';
import type { Appearance, CpuAssist, DiskInfo, Job, Recording, Session, Settings, Snapshot, Status, UpdateState } from './types';

export type Route =
  | { name: 'library'; tab: 'sessions' | 'favorites' | 'deleted' }
  | { name: 'session'; sessionId: string; recordingId?: string; at?: number }
  | { name: 'editor'; projectId?: string }
  | { name: 'settings'; page: string };

type Toast = { id: number; text: string; tone: 'info' | 'error' | 'ok'; action?: { label: string; run: () => void } };

const idleStatus: Status = { state: 'idle', elapsed: 0, eventsStatus: 'idle', events: [], clipsSaved: 0, bookmarking: false, screenRecording: false, screenElapsed: 0, apm: 0 };

export const app = $state({
  ready: false,
  settings: null as Settings | null,
  mediaUrl: '',
  version: '',
  status: idleStatus as Status,
  library: { sessions: [], recordings: [] } as Snapshot,
  route: { name: 'library', tab: 'sessions' } as Route,
  history: [] as Route[],
  jobs: {} as Record<string, Job>,
  disk: null as DiskInfo | null,
  toasts: [] as Toast[],
  artReady: {} as Record<string, boolean>,
  cpuAssist: { on: false, available: false, speedup: null, measuring: false } as CpuAssist,
  update: { phase: 'idle', version: null, progress: null, error: null, checkedAt: null } as UpdateState,
  /** "Later" on the update banner hides it for that version until the app restarts. */
  updateDismissed: null as string | null,
});

const RELEASES = 'https://github.com/mattcui951/matt-game-recorder/releases';

export function releaseUrl(version: string): string {
  return `${RELEASES}/tag/v${version}`;
}

/** Updating closes the app: not while League runs or a match is being recorded or saved. */
export function updateBlocked(): boolean {
  return app.status.state === 'recording' || app.status.state === 'saving' || app.status.eventsStatus !== 'idle';
}

export async function checkUpdate() {
  try {
    app.update = await api.checkUpdate();
  } catch (e) {
    toast(errorText(e), 'error');
  }
}

export async function installUpdate() {
  try {
    await api.installUpdate();
  } catch (e) {
    toast(errorText(e), 'error');
  }
}

function readPref<T extends string>(key: string, fallback: T): T {
  try {
    return (localStorage.getItem(key) as T) ?? fallback;
  } catch {
    return fallback;
  }
}

/** Per-viewer conveniences, remembered in localStorage when available. */
export const ui = $state({
  libraryView: readPref<'table' | 'grid'>('mgr.libraryView', 'table'),
  showApm: readPref<'on' | 'off'>('mgr.showApm', 'off') === 'on',
  // Library date filter; kept while you open a match and come back, not across restarts.
  libraryYear: null as number | null,
  libraryMonth: null as number | null,
  libraryDay: null as number | null,
});

$effect.root(() => {
  $effect(() => {
    try {
      localStorage.setItem('mgr.libraryView', ui.libraryView);
      localStorage.setItem('mgr.showApm', ui.showApm ? 'on' : 'off');
    } catch {
      /* storage unavailable */
    }
  });
});

let toastId = 0;
export function toast(text: string, tone: Toast['tone'] = 'info', action?: Toast['action']) {
  const id = ++toastId;
  app.toasts.push({ id, text, tone, action });
  setTimeout(() => (app.toasts = app.toasts.filter((t) => t.id !== id)), tone === 'error' ? 7000 : 4000);
}

export function errorText(e: unknown): string {
  return typeof e === 'string' ? e : e instanceof Error ? e.message : JSON.stringify(e);
}

export function go(route: Route) {
  app.history.push(app.route);
  if (app.history.length > 30) app.history.shift();
  app.route = route;
}

export function back() {
  app.route = app.history.pop() ?? { name: 'library', tab: 'sessions' };
}

export function mediaSrc(r: { relDir: string }, file?: string | null): string {
  if (!file) return '';
  const path = [...r.relDir.split('/'), file].filter(Boolean).map(encodeURIComponent).join('/');
  return `${app.mediaUrl}media/${path}`;
}

export function cacheSrc(rel: string): string {
  return `${app.mediaUrl}media/${rel.split('/').map(encodeURIComponent).join('/')}`;
}

// Plain set (not state): artSrc is called from templates and derived values, which must not write state.
const artRequested = new Set<string>();

export function artSrc(key: string | null | undefined, kind: 'icon' | 'splash' = 'icon'): string {
  if (!key) return '';
  if (!artRequested.has(key)) {
    artRequested.add(key);
    api.ensureArt(key).then((ok) => {
      app.artReady[key] = ok;
    }).catch(() => {});
  }
  if (!app.artReady[key]) return '';
  return `${app.mediaUrl}art/${encodeURIComponent(kind === 'icon' ? `${key}.png` : `${key}_splash.jpg`)}`;
}

export async function refreshLibrary() {
  app.library = await api.getLibrary();
}

export async function refreshDisk() {
  try {
    app.disk = await api.diskInfo();
  } catch {
    /* disk info is best-effort */
  }
}

export async function saveSettings(next: Settings) {
  const prev = app.settings;
  app.settings = next;
  try {
    await api.saveSettings($state.snapshot(next) as Settings);
  } catch (e) {
    app.settings = prev;
    toast(t.common.settingsNotSaved(errorText(e)), 'error');
  }
}

/** Mutate settings in place and persist (debounced so sliders do not spam disk). */
let saveTimer: ReturnType<typeof setTimeout> | undefined;
export function updateSettings(mut: (s: Settings) => void) {
  if (!app.settings) return;
  mut(app.settings);
  clearTimeout(saveTimer);
  saveTimer = setTimeout(() => {
    if (app.settings) api.saveSettings($state.snapshot(app.settings) as Settings).catch((e) => toast(t.common.settingsNotSaved(errorText(e)), 'error'));
  }, 250);
}

/** Change the look: applied at once in every window, saved shortly after. */
export function setAppearance(mut: (a: Appearance) => void) {
  updateSettings((s) => {
    mut(s.appearance);
    s.appearance.windowBg = themeColors(s.appearance, activeThemeId(s.appearance)).bg;
  });
  if (!app.settings) return;
  const a = $state.snapshot(app.settings.appearance) as Appearance;
  applyAppearance(a);
  broadcastAppearance(a);
}

/** Switch the UI language: applied at once in every window, saved shortly after. */
export function setLanguage(language: LanguageSetting) {
  updateSettings((s) => (s.general.language = language));
  useLanguage(language);
  emit('language', language).catch(() => {});
}

function useLanguage(language: string | undefined) {
  applyLanguage(language);
  if (i18n.locale === 'zh-CN' && !Object.keys(championsZh.names).length) {
    api.championNamesZh().then((names) => {
      championsZh.names = Object.fromEntries(Object.entries(names).map(([k, v]) => [k.toLowerCase(), v]));
    }).catch(() => {});
  }
}

export function liveRecordings(): Recording[] {
  return app.library.recordings.filter((r) => !r.deletedAt);
}

export function sessionById(id: string): Session | undefined {
  return app.library.sessions.find((s) => s.id === id);
}

export function recordingsOf(sessionId: string): Recording[] {
  return app.library.recordings.filter((r) => r.sessionId === sessionId && !r.deletedAt).sort((a, b) => a.createdAt.localeCompare(b.createdAt));
}

export async function init() {
  const boot = await api.bootstrap();
  boot.settings.appearance = { ...DEFAULT_APPEARANCE, ...(boot.settings.appearance ?? {}) };
  app.settings = boot.settings;
  i18n.system = boot.systemLanguage;
  useLanguage(boot.settings.general.language);
  applyAppearance(boot.settings.appearance);
  // Windows switching light/dark matters when the theme follows it.
  matchMedia('(prefers-color-scheme: dark)').addEventListener('change', () => {
    if (app.settings?.appearance.followSystem) setAppearance(() => {});
  });
  app.mediaUrl = boot.mediaUrl;
  app.version = boot.version;
  app.update = boot.update;
  if (boot.updatedFrom) toast(t.update.done(boot.version), 'ok');
  app.status = boot.status;
  await refreshLibrary();
  refreshDisk();
  setInterval(refreshDisk, 60_000);
  await listen<Status>('status', (e) => {
    app.status = e.payload;
  });
  await listen('library-changed', () => {
    refreshLibrary();
    refreshDisk();
  });
  await listen<Job>('job', (e) => {
    const j = e.payload;
    app.jobs[j.id] = j;
    if (j.state === 'error') toast(t.common.jobFailed(j.title, j.message ?? t.common.failedLower), 'error');
    if (j.state === 'done' && j.kind === 'import' && j.message) toast(j.message, 'ok');
    if (j.state !== 'running') setTimeout(() => delete app.jobs[j.id], j.state === 'done' ? 6000 : 300);
  });
  app.cpuAssist = await api.importCpuAssist().catch(() => app.cpuAssist);
  await listen<Appearance>('appearance', (e) => {
    if (app.settings) app.settings.appearance = e.payload;
    applyAppearance(e.payload);
  });
  await listen<UpdateState>('update', (e) => {
    app.update = e.payload;
  });
  await listen<LanguageSetting>('language', (e) => {
    if (app.settings) app.settings.general.language = e.payload;
    useLanguage(e.payload);
  });
  await listen<CpuAssist>('import-cpu-assist', (e) => {
    app.cpuAssist = e.payload;
  });
  // Jobs that started before this window opened (e.g. an import resumed at startup).
  for (const j of await api.listJobs().catch(() => [] as Job[])) app.jobs[j.id] = j;
  await listen<string>('open-session', (e) => go({ name: 'session', sessionId: e.payload }));
  app.ready = true;
}
