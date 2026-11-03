<script lang="ts">
  import Icon from '../components/Icon.svelte';
  import ImportPanel from '../components/ImportPanel.svelte';
  import RangeSupercut from '../components/RangeSupercut.svelte';
  import ImportProgress from '../components/ImportProgress.svelte';
  import Popover from '../components/Popover.svelte';
  import RecordingTile from '../components/RecordingTile.svelte';
  import Segmented from '../components/Segmented.svelte';
  import Select from '../components/Select.svelte';
  import { api } from '../lib/api';
  import { app, artSrc, errorText, go, mediaSrc, toast, ui } from '../lib/state.svelte';
  import { bytes, championId, championName, clock, dayLabel, displayTitle, gameName, modeName, timeOfDay } from '../lib/format.svelte';
  import { FILTER_KINDS, filterKey, meta } from '../lib/events';
  import { i18n, intlLocale, t } from '../lib/i18n.svelte';
  import type { Recording, Session } from '../lib/types';

  let { tab }: { tab: 'sessions' | 'favorites' | 'deleted' } = $props();

  let query = $state('');
  let filterOpen = $state(false);
  let importOpen = $state(false);
  let supercutOpen = $state(false);
  let groupBy = $state<'game-date' | 'date' | 'game'>('game-date');
  let sort = $state<'newest' | 'oldest' | 'largest' | 'longest'>('newest');
  let games = $state<string[]>([]);
  let kinds = $state<string[]>([]);
  let result = $state<'all' | 'win' | 'loss'>('all');
  let champions = $state<string[]>([]);
  let selecting = $state(false);
  let selected = $state<string[]>([]);

  const imports = $derived(Object.values(app.jobs).filter((j) => j.kind === 'import' && j.state === 'running'));
  const live = $derived(app.library.recordings.filter((r) => !r.deletedAt));
  const bySession = $derived.by(() => {
    const m = new Map<string, Recording[]>();
    for (const r of live) {
      const list = m.get(r.sessionId) ?? [];
      list.push(r);
      m.set(r.sessionId, list);
    }
    return m;
  });
  const allGames = $derived([...new Set(app.library.sessions.map((s) => s.game))]);
  // One button per champion (by championId), sorted by the name shown.
  const allChampions = $derived.by(() => {
    const byId = new Map<string, string>();
    for (const s of app.library.sessions) if (s.champion && !byId.has(championId(s.champion))) byId.set(championId(s.champion), s.champion);
    return [...byId].map(([id, name]) => ({ id, label: championName(name) })).sort((a, b) => a.label.localeCompare(b.label, intlLocale()));
  });
  const presentKinds = $derived(new Set(live.flatMap((r) => r.events.map(filterKey))));

  function matches(s: Session, recs: Recording[]): boolean {
    if (games.length && !games.includes(s.game)) return false;
    if (result !== 'all' && s.result !== result) return false;
    if (champions.length && !champions.includes(championId(s.champion ?? ''))) return false;
    if (kinds.length && !recs.some((r) => r.events.some((e) => kinds.includes(filterKey(e))))) return false;
    const q = query.trim().toLowerCase();
    // Search matches both languages' names, so "Match 03" and "对局 03" both find it.
    const names = [s.title, displayTitle(s.title), s.champion ?? '', championName(s.champion), s.game, gameName(s.game), modeName(s.gameMode), ...recs.flatMap((r) => [r.title, displayTitle(r.title)])];
    if (q && !names.some((x) => x.toLowerCase().includes(q))) return false;
    return true;
  }

  function sessionStats(s: Session) {
    const recs = bySession.get(s.id) ?? [];
    const full = recs.find((r) => r.kind === 'match' || r.kind === 'session');
    const duration = full?.duration ?? recs.reduce((a, r) => a + r.duration, 0);
    const size = recs.reduce((a, r) => a + r.size, 0);
    const clips = recs.filter((r) => r.kind !== 'match' && r.kind !== 'session').length;
    // A full recording holds every mark; otherwise the clips together do.
    const evs = full ? full.events : recs.flatMap((r) => r.events);
    const count = (k: string) => evs.filter((e) => filterKey(e) === k).length;
    return { duration, size, clips, multis: evs.filter((e) => e.kind === 'multikill').length, objectives: ['turret', 'inhibitor', 'dragon', 'herald', 'baron'].reduce((a, k) => a + count(k), 0) };
  }

  // Everything the search and filters allow; the date bar narrows this further and counts from it.
  const candidates = $derived(app.library.sessions.filter((s) => (bySession.get(s.id)?.length ?? 0) > 0 && matches(s, bySession.get(s.id)!)));
  const MONTHS = $derived((i18n.locale, Array.from({ length: 12 }, (_, i) => new Date(2000, i, 1).toLocaleDateString(intlLocale(), { month: 'short' }))));
  const MONTHS_LONG = $derived((i18n.locale, Array.from({ length: 12 }, (_, i) => new Date(2000, i, 1).toLocaleDateString(intlLocale(), { month: 'long' }))));
  /** A date of the chosen period, e.g. "October 20, 2026" or "2026年10月20日" (parts left out when unknown). */
  function periodDate(y: number | null, m: number, d: number | null): string {
    const opts: Intl.DateTimeFormatOptions = { month: 'long' };
    if (y !== null) opts.year = 'numeric';
    if (d !== null) opts.day = 'numeric';
    return new Date(y ?? 2000, m, d ?? 1).toLocaleDateString(intlLocale(), opts);
  }
  const years = $derived(
    [...new Set(app.library.sessions.filter((s) => (bySession.get(s.id)?.length ?? 0) > 0).map((s) => new Date(s.startedAt).getFullYear()))].sort((a, b) => b - a),
  );
  const yearCounts = $derived.by(() => {
    const m = new Map<number, number>();
    for (const s of candidates) {
      const y = new Date(s.startedAt).getFullYear();
      m.set(y, (m.get(y) ?? 0) + 1);
    }
    return m;
  });
  const monthCounts = $derived.by(() => {
    const c: number[] = new Array(12).fill(0);
    for (const s of candidates) {
      const d = new Date(s.startedAt);
      if (ui.libraryYear === null || d.getFullYear() === ui.libraryYear) c[d.getMonth()]++;
    }
    return c;
  });
  const monthPeak = $derived(Math.max(1, ...monthCounts));
  // Only months you have recordings in (for the chosen year); new imports add theirs as they land.
  const monthsShown = $derived.by(() => {
    const has = new Set<number>();
    for (const s of app.library.sessions) {
      if (!bySession.get(s.id)?.length) continue;
      const d = new Date(s.startedAt);
      if (ui.libraryYear === null || d.getFullYear() === ui.libraryYear) has.add(d.getMonth());
    }
    if (ui.libraryMonth !== null) has.add(ui.libraryMonth);
    return [...has].sort((a, b) => a - b);
  });
  // Days of the chosen month you played on, same rules as the months.
  const inMonth = (d: Date) => d.getMonth() === ui.libraryMonth && (ui.libraryYear === null || d.getFullYear() === ui.libraryYear);
  const dayCounts = $derived.by(() => {
    const m = new Map<number, number>();
    if (ui.libraryMonth === null) return m;
    for (const s of candidates) {
      const d = new Date(s.startedAt);
      if (inMonth(d)) m.set(d.getDate(), (m.get(d.getDate()) ?? 0) + 1);
    }
    return m;
  });
  const daysShown = $derived.by(() => {
    const has = new Set<number>();
    if (ui.libraryMonth === null) return [];
    for (const s of app.library.sessions) {
      if (!bySession.get(s.id)?.length) continue;
      const d = new Date(s.startedAt);
      if (inMonth(d)) has.add(d.getDate());
    }
    if (ui.libraryDay !== null) has.add(ui.libraryDay);
    return [...has].sort((a, b) => a - b);
  });
  // A weekday only means something when the year is known.
  const dayYear = $derived(ui.libraryYear ?? (years.length === 1 ? years[0] : null));
  function weekday(day: number): string {
    return dayYear === null || ui.libraryMonth === null ? '' : new Date(dayYear, ui.libraryMonth, day).toLocaleDateString(intlLocale(), { weekday: 'short' });
  }
  const periodOn = $derived(ui.libraryYear !== null || ui.libraryMonth !== null);
  const periodLabel = $derived.by(() => {
    const y = ui.libraryYear;
    if (ui.libraryMonth === null) return y !== null ? `${y}` : '';
    const when = periodDate(y, ui.libraryMonth, ui.libraryDay);
    return y !== null ? when : t.library.periodAllYears(when);
  });
  function inPeriod(s: Session): boolean {
    const d = new Date(s.startedAt);
    if (ui.libraryYear !== null && d.getFullYear() !== ui.libraryYear) return false;
    if (ui.libraryMonth !== null && d.getMonth() !== ui.libraryMonth) return false;
    return ui.libraryDay === null || d.getDate() === ui.libraryDay;
  }
  function pickYear(y: number | null) {
    ui.libraryYear = y;
    ui.libraryDay = null;
  }
  function pickMonth(m: number) {
    ui.libraryMonth = ui.libraryMonth === m ? null : m;
    ui.libraryDay = null;
  }
  const clearPeriod = () => ((ui.libraryYear = null), (ui.libraryMonth = null), (ui.libraryDay = null));

  const shown = $derived.by(() => {
    const list = candidates.filter(inPeriod);
    const stat = new Map(list.map((s) => [s.id, sessionStats(s)]));
    const by: Record<typeof sort, (a: Session, b: Session) => number> = {
      newest: (a, b) => b.startedAt.localeCompare(a.startedAt),
      oldest: (a, b) => a.startedAt.localeCompare(b.startedAt),
      largest: (a, b) => stat.get(b.id)!.size - stat.get(a.id)!.size,
      longest: (a, b) => stat.get(b.id)!.duration - stat.get(a.id)!.duration,
    };
    return list.sort(by[sort]);
  });
  const shownBytes = $derived(shown.reduce((a, s) => a + sessionStats(s).size, 0));
  const totalSessions = $derived(app.library.sessions.filter((s) => (bySession.get(s.id)?.length ?? 0) > 0).length);

  type Group = { key: string; game?: string; day?: string; sessions: Session[] };
  const groups = $derived.by(() => {
    // Size and length sorts read best as one ranked list.
    if (sort === 'largest' || sort === 'longest') return [{ key: 'all', sessions: shown }] as Group[];
    const out: Group[] = [];
    const index = new Map<string, Group>();
    for (const s of shown) {
      const day = dayLabel(s.startedAt);
      const key = groupBy === 'game' ? s.game : groupBy === 'date' ? day : `${s.game}|${day}`;
      let g = index.get(key);
      if (!g) {
        g = { key, game: groupBy !== 'date' ? s.game : undefined, day: groupBy !== 'game' ? day : undefined, sessions: [] };
        index.set(key, g);
        out.push(g);
      }
      g.sessions.push(s);
    }
    return out;
  });

  function lead(s: Session): Recording | undefined {
    const recs = bySession.get(s.id) ?? [];
    return recs.find((r) => r.kind === 'match' || r.kind === 'session') ?? recs.find((r) => r.thumb);
  }

  const favorites = $derived(live.filter((r) => r.favorite));
  const deleted = $derived(app.library.recordings.filter((r) => r.deletedAt).sort((a, b) => (b.deletedAt ?? '').localeCompare(a.deletedAt ?? '')));
  const keepDays = $derived(app.settings?.storage.keepDeletedDays ?? 3);
  function daysLeft(r: Recording): number {
    const gone = new Date(r.deletedAt!).getTime() + keepDays * 86400000;
    return Math.max(0, Math.ceil((gone - Date.now()) / 86400000));
  }

  const retention = $derived.by(() => {
    const st = app.settings?.storage;
    if (!st) return '';
    const parts = [];
    if (st.ageLimitEnabled) parts.push(t.library.keepMonths(st.ageLimitMonths));
    if (st.mode === 'auto') parts.push(t.library.keepGb(st.limitGb));
    return t.library.keeping(parts);
  });

  async function run(p: Promise<unknown>, ok?: string) {
    try {
      await p;
      if (ok) toast(ok, 'ok');
    } catch (e) {
      toast(errorText(e), 'error');
    }
  }

  async function deleteSessions(ids: string[]) {
    const recIds = ids.flatMap((id) => (bySession.get(id) ?? []).map((r) => r.id));
    const one = app.library.sessions.find((s) => s.id === ids[0])?.title;
    const label = ids.length === 1 ? (one ? displayTitle(one) : t.library.match) : t.library.nMatches(ids.length);
    try {
      await api.deleteSessions(ids);
      selected = selected.filter((s) => !ids.includes(s));
      toast(t.common.movedToDeleted(label), 'info', { label: t.common.undo, run: () => api.restoreRecordings(recIds) });
    } catch (e) {
      toast(errorText(e), 'error');
    }
  }

  function open(s: Session) {
    if (selecting) selected = toggle(selected, s.id);
    else go({ name: 'session', sessionId: s.id });
  }
  function onRowKey(e: KeyboardEvent, s: Session) {
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      open(s);
    }
  }
  const selectedBytes = $derived(selected.reduce((a, id) => a + (bySession.get(id) ?? []).reduce((b, r) => b + r.size, 0), 0));

  const toggle = (list: string[], v: string) => (list.includes(v) ? list.filter((x) => x !== v) : [...list, v]);
  const activeFilters = $derived(games.length + kinds.length + champions.length + (result === 'all' ? 0 : 1));
  const clearFilters = () => ((games = []), (kinds = []), (champions = []), (result = 'all'));
</script>

<section class="lib">
  <header class="head">
    <nav class="tabs" aria-label={t.library.sections}>
      <button class:on={tab === 'sessions'} onclick={() => go({ name: 'library', tab: 'sessions' })}>{t.library.sessions}</button>
      <button class:on={tab === 'favorites'} onclick={() => go({ name: 'library', tab: 'favorites' })}>{t.library.favorites} <span class="num count">{favorites.length}</span></button>
      <button class:on={tab === 'deleted'} onclick={() => go({ name: 'library', tab: 'deleted' })}>{t.library.deleted} <span class="num count">{deleted.length}</span></button>
    </nav>
    {#if tab === 'sessions'}
      <div class="find">
        <Popover bind:open={supercutOpen} width={360}>
          {#snippet anchor(toggleOpen)}
            <button class="btn ghost sm" onclick={toggleOpen}><Icon name="bolt" size={15} />{t.supercut.button}</button>
          {/snippet}
          {#snippet children(close)}
            <RangeSupercut onstarted={close} />
          {/snippet}
        </Popover>
        <Popover bind:open={importOpen} width={340}>
          {#snippet anchor(toggleOpen)}
            <button class="btn ghost sm" onclick={toggleOpen}><Icon name="restore" size={15} />{t.library.import}</button>
          {/snippet}
          {#snippet children(close)}
            <ImportPanel onstarted={close} />
          {/snippet}
        </Popover>
        <div class="search">
          <Icon name="search" size={15} />
          <input class="field" placeholder={t.library.search} bind:value={query} aria-label={t.library.searchAria} />
        </div>
        <div class="sortsel">
          <Select
            label={t.library.sort}
            value={sort}
            onchange={(v) => (sort = v)}
            options={[
              { value: 'newest', label: t.library.newest },
              { value: 'oldest', label: t.library.oldest },
              { value: 'largest', label: t.library.largest },
              { value: 'longest', label: t.library.longest },
            ]}
          />
        </div>
        <Popover bind:open={filterOpen} width={320}>
          {#snippet anchor(toggleOpen)}
            <button class="icon-btn" class:on={activeFilters > 0 || filterOpen} onclick={toggleOpen} aria-label={t.library.filterGroup} title={t.library.filterGroup}>
              <Icon name="filter" size={17} />
            </button>
          {/snippet}
          {#snippet children()}
            <div class="filters">
              <div class="f-title">{t.library.groupBy}</div>
              <div class="f-opts">
                {#each [['game-date', t.library.gameAndDate], ['date', t.library.date], ['game', t.library.game]] as [v, l] (v)}
                  <button class="f-opt" class:on={groupBy === v} onclick={() => (groupBy = v as typeof groupBy)}>{l}</button>
                {/each}
              </div>
              <div class="f-title">{t.library.result}</div>
              <div class="f-opts">
                {#each [['all', t.library.all], ['win', t.library.victories], ['loss', t.library.defeats]] as [v, l] (v)}
                  <button class="f-opt" class:on={result === v} onclick={() => (result = v as typeof result)}>{l}</button>
                {/each}
              </div>
              {#if allChampions.length}
                <div class="f-title">{t.library.champions}</div>
                <div class="f-opts champs">
                  {#each allChampions as c (c.id)}
                    <button class="f-opt" class:on={champions.includes(c.id)} onclick={() => (champions = toggle(champions, c.id))}>{c.label}</button>
                  {/each}
                </div>
              {/if}
              <div class="f-title">{t.library.games}</div>
              <div class="f-opts">
                {#each allGames as g (g)}
                  <button class="f-opt" class:on={games.includes(g)} onclick={() => (games = toggle(games, g))}>{gameName(g)}</button>
                {/each}
              </div>
              <div class="f-title">{t.library.hasEvents}</div>
              <div class="f-opts">
                {#each FILTER_KINDS.filter((k) => presentKinds.has(k)) as k (k)}
                  <button class="f-opt ev" class:on={kinds.includes(k)} style="--c:{meta(k).color}" onclick={() => (kinds = toggle(kinds, k))}>
                    <Icon name={meta(k).icon} size={14} />{meta(k).label}
                  </button>
                {:else}
                  <span class="muted">{t.library.noEventsYet}</span>
                {/each}
              </div>
              {#if activeFilters}<button class="btn ghost sm" onclick={clearFilters}>{t.library.clearFilters}</button>{/if}
            </div>
          {/snippet}
        </Popover>
        <Segmented
          label={t.library.view}
          value={ui.libraryView}
          onchange={(v) => (ui.libraryView = v)}
          options={[
            { value: 'table', label: t.library.list, hint: t.library.listHint },
            { value: 'grid', label: t.library.grid, hint: t.library.gridHint },
          ]}
        />
        <button class="btn sm" class:sel-on={selecting} onclick={() => ((selecting = !selecting), (selected = []))}><Icon name="check" size={14} />{selecting ? t.common.done : t.library.select}</button>
      </div>
    {:else if tab === 'deleted' && deleted.length}
      <button class="btn danger sm" onclick={() => run(api.emptyRecentlyDeleted(), t.library.emptied)}>{t.library.deleteAllForever}</button>
    {/if}
  </header>

  <div class="body">
    {#each imports as job (job.id)}
      <div class="import-slot"><ImportProgress {job} /></div>
    {/each}
    {#if tab === 'sessions'}
      {#if years.length}
        <div class="period" role="group" aria-label={t.library.showFrom}>
          <div class="years">
            <button class="yr" class:on={ui.libraryYear === null} aria-pressed={ui.libraryYear === null} onclick={() => pickYear(null)}>
              {t.library.allYears}<span class="num n">{candidates.length}</span>
            </button>
            {#each years as y (y)}
              <button class="yr" class:on={ui.libraryYear === y} aria-pressed={ui.libraryYear === y} onclick={() => pickYear(ui.libraryYear === y ? null : y)}>
                <span class="num">{y}</span><span class="num n">{yearCounts.get(y) ?? 0}</span>
              </button>
            {/each}
          </div>
          <div class="cal">
            <div class="months">
              {#each monthsShown as i (i)}
                {@const m = MONTHS[i]}
                {@const n = monthCounts[i]}
                <button
                  class="mo"
                  class:on={ui.libraryMonth === i}
                  disabled={!n && ui.libraryMonth !== i}
                  aria-pressed={ui.libraryMonth === i}
                  aria-label={t.library.monthAria(MONTHS_LONG[i], n)}
                  onclick={() => pickMonth(i)}
                >
                  <span class="m">{m}</span>
                  <span class="num n">{n || '-'}</span>
                  <span class="bar" style="--fill:{n / monthPeak}"></span>
                </button>
              {/each}
            </div>
            {#if daysShown.length}
              <div class="days" role="group" aria-label={t.library.daysIn(MONTHS_LONG[ui.libraryMonth ?? 0])}>
                {#each daysShown as d (d)}
                  {@const n = dayCounts.get(d) ?? 0}
                  <button
                    class="dy"
                    class:on={ui.libraryDay === d}
                    disabled={!n && ui.libraryDay !== d}
                    aria-pressed={ui.libraryDay === d}
                    aria-label={t.library.dayAria(periodDate(null, ui.libraryMonth ?? 0, d), n)}
                    onclick={() => (ui.libraryDay = ui.libraryDay === d ? null : d)}
                  >
                    <span class="num dn">{d}</span>
                    {#if weekday(d)}<span class="wd">{weekday(d)}</span>{/if}
                    <span class="num n">{n || '-'}</span>
                  </button>
                {/each}
              </div>
            {/if}
          </div>
        </div>
      {/if}
      <div class="summary">
        <span>
          <span class="num">{t.library.count(shown.length, totalSessions)}</span>{#if periodLabel}{' '}{t.library.inPeriod(periodLabel, ui.libraryDay !== null)} <button class="link" onclick={clearPeriod}>{t.library.showAllDates}</button>{/if} · <span class="num">{bytes(shownBytes)}</span>
          {#if app.disk} · <span class="num">{t.library.freeOnDrive(bytes(app.disk.free))}</span>{/if}
        </span>
        <button class="link" onclick={() => go({ name: 'settings', page: 'storage' })}>{retention} · {t.library.storageLink}</button>
      </div>
      {#if selecting}
        <div class="selbar">
          <span><span class="num">{t.library.selected(selected.length)}</span>{#if selected.length} · <span class="num">{bytes(selectedBytes)}</span>{/if}</span>
          <button class="btn ghost sm" onclick={() => (selected = selected.length === shown.length ? [] : shown.map((s) => s.id))}>
            {selected.length === shown.length && shown.length ? t.library.selectNone : t.library.selectAll(shown.length)}
          </button>
          <span class="grow"></span>
          <button class="btn danger sm" disabled={!selected.length} onclick={() => deleteSessions(selected)}><Icon name="trash" size={14} />{t.library.deleteN(selected.length)}</button>
        </div>
      {/if}
      {#each groups as g (g.key)}
        <section class="group">
          {#if g.game || g.day}
            <h2 class="g-head">
              {#if g.game}<span>{gameName(g.game)}</span>{/if}
              {#if g.game && g.day}<span class="g-sep"></span>{/if}
              {#if g.day}<span class="g-day">{g.day}</span>{/if}
            </h2>
          {/if}
          {#if ui.libraryView === 'table'}
            <div class="table">
              {#each g.sessions as s (s.id)}
                {@const st = sessionStats(s)}
                {@const lr = lead(s)}
                {@const icon = artSrc(s.championKey)}
                <div
                  class="row"
                  class:picked={selected.includes(s.id)}
                  class:selecting
                  role="button"
                  tabindex="0"
                  aria-pressed={selecting ? selected.includes(s.id) : undefined}
                  onclick={() => open(s)}
                  onkeydown={(e) => onRowKey(e, s)}
                >
                  {#if selecting}<span class="check" aria-hidden="true">{#if selected.includes(s.id)}<Icon name="check" size={14} />{/if}</span>{/if}
                  <span class="r-thumb">{#if lr?.thumb}<img src={mediaSrc(lr, lr.thumb)} alt="" loading="lazy" draggable="false" />{/if}</span>
                  <span class="r-champ">
                    {#if icon}<img src={icon} alt="" draggable="false" />{:else}<span class="ph">{(s.champion ?? s.game).slice(0, 1)}</span>{/if}
                  </span>
                  <span class="r-title">
                    <span class="t">{displayTitle(s.title)}</span>
                    <span class="s">
                      {championName(s.champion) || (s.captureMode === 'screen' ? t.library.screenRecording : gameName(s.game))}{s.gameMode ? ` · ${modeName(s.gameMode)}` : ''} · {g.day ? timeOfDay(s.startedAt) : `${dayLabel(s.startedAt)} ${timeOfDay(s.startedAt)}`}
                    </span>
                  </span>
                  <span class="r-result {s.result ?? 'none'}">{s.result === 'win' ? t.common.victory : s.result === 'loss' ? t.common.defeat : ''}</span>
                  <span class="r-kda num" aria-label={t.library.kda}>
                    {#if s.kda}
                      <span class="k">{s.kda.kills}</span><i>/</i><span class="d">{s.kda.deaths}</span><i>/</i><span class="a">{s.kda.assists}</span>
                    {:else}<span class="muted">-</span>{/if}
                  </span>
                  <span class="r-tags">
                    {#if st.multis}<span class="tag you"><Icon name="swords" size={12} />{t.library.multi(st.multis)}</span>{/if}
                    {#if st.objectives}<span class="tag obj"><Icon name="turret" size={12} />{st.objectives}</span>{/if}
                    {#if st.clips}<span class="tag"><Icon name="scissors" size={12} />{t.library.clips(st.clips)}</span>{/if}
                  </span>
                  <span class="r-dur num">{clock(st.duration)}</span>
                  <span class="r-size num">{bytes(st.size)}</span>
                  <button class="del" onclick={(e) => (e.stopPropagation(), deleteSessions([s.id]))} onkeydown={(e) => e.stopPropagation()} aria-label={t.library.deleteThis(displayTitle(s.title))} title={t.common.delete}>
                    <Icon name="trash" size={16} />
                  </button>
                </div>
              {/each}
            </div>
          {:else}
            <div class="grid">
              {#each g.sessions as s (s.id)}
                {@const st = sessionStats(s)}
                {@const lr = lead(s)}
                <div
                  class="card"
                  class:picked={selected.includes(s.id)}
                  role="button"
                  tabindex="0"
                  aria-pressed={selecting ? selected.includes(s.id) : undefined}
                  onclick={() => open(s)}
                  onkeydown={(e) => onRowKey(e, s)}
                >
                  <span class="c-thumb">
                    {#if lr?.thumb}<img src={mediaSrc(lr, lr.thumb)} alt="" loading="lazy" draggable="false" />{/if}
                    <span class="c-band {s.result ?? 'none'}"></span>
                    {#if selecting}<span class="check c-check" aria-hidden="true">{#if selected.includes(s.id)}<Icon name="check" size={14} />{/if}</span>{/if}
                    <span class="c-dur num">{clock(st.duration)}</span>
                    <button class="del c-del" onclick={(e) => (e.stopPropagation(), deleteSessions([s.id]))} onkeydown={(e) => e.stopPropagation()} aria-label={t.library.deleteThis(displayTitle(s.title))} title={t.common.delete}>
                      <Icon name="trash" size={15} />
                    </button>
                  </span>
                  <span class="c-meta">
                    <span class="t">{displayTitle(s.title)}</span>
                    <span class="num c-kda">{#if s.kda}<span class="k">{s.kda.kills}</span>/<span class="d">{s.kda.deaths}</span>/<span class="a">{s.kda.assists}</span>{/if}</span>
                  </span>
                  <span class="c-sub">
                    <span>{championName(s.champion) || gameName(s.game)}</span>
                    {#if s.result}<span class="r-result {s.result}">{s.result === 'win' ? t.common.victory : t.common.defeat}</span>{/if}
                    <span>{g.day ? timeOfDay(s.startedAt) : dayLabel(s.startedAt)}</span>
                    <span class="num">{bytes(st.size)}</span>
                  </span>
                </div>
              {/each}
            </div>
          {/if}
        </section>
      {:else}
        <div class="empty">
          {#if query || activeFilters || periodOn}
            <h3>{t.library.noMatch}</h3>
            <p>{periodLabel && !query && !activeFilters ? t.library.nothingIn(periodLabel) : t.library.tryDifferent}</p>
            <button class="btn sm" onclick={() => ((query = ''), clearFilters(), clearPeriod())}>{t.library.showEverything}</button>
          {:else}
            <h3>{t.library.noRecordings}</h3>
            <p>
              {t.library.noRecordingsText(
                app.settings?.games.lol.captureMode === 'highlights' ? t.library.modeHighlights : app.settings?.games.lol.captureMode === 'manual' ? t.library.modeManual : t.library.modeMatch,
              )}
            </p>
            <button class="btn sm" onclick={() => go({ name: 'settings', page: 'lol' })}>{t.library.recordingSettings}</button>
          {/if}
        </div>
      {/each}
    {:else if tab === 'favorites'}
      {#if favorites.length}
        <div class="grid">
          {#each favorites as r (r.id)}
            <RecordingTile rec={r} onclick={() => go({ name: 'session', sessionId: r.sessionId, recordingId: r.id })} />
          {/each}
        </div>
      {:else}
        <div class="empty"><h3>{t.library.noFavorites}</h3><p>{t.library.noFavoritesText}</p></div>
      {/if}
    {:else}
      {#if deleted.length}
        <p class="hint">
          {t.library.keptFor(keepDays)}
          <button class="link" onclick={() => go({ name: 'settings', page: 'storage' })}>{t.library.change}</button>
        </p>
        <div class="void-list">
          {#each deleted as r (r.id)}
            <div class="void">
              <span class="v-thumb">{#if r.thumb}<img src={mediaSrc(r, r.thumb)} alt="" loading="lazy" />{/if}<span class="stamp">{t.library.voidStamp}</span></span>
              <span class="v-title"><span class="t">{displayTitle(r.title)}</span><span class="s">{gameName(r.originalDir?.split('/')[0])} · {bytes(r.size)} · {clock(r.duration)}</span></span>
              <span class="v-left num">{t.library.daysLeft(daysLeft(r))}</span>
              <button class="btn sm" onclick={() => run(api.restoreRecordings([r.id]), t.library.restored(displayTitle(r.title)))}><Icon name="restore" size={15} />{t.library.restore}</button>
              <button class="btn danger sm" onclick={() => run(api.purgeRecordings([r.id]))}>{t.library.deleteForever}</button>
            </div>
          {/each}
        </div>
      {:else}
        <div class="empty">
          <h3>{t.library.nothingHere}</h3>
          <p>{t.library.waitHere(keepDays)}</p>
          <button class="btn sm" onclick={() => go({ name: 'settings', page: 'storage' })}>{t.library.changeRetention}</button>
        </div>
      {/if}
    {/if}
  </div>
</section>

<style>
  .lib {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 0 24px;
    height: 52px;
    border-bottom: 1px solid var(--seam);
    flex: none;
  }
  .tabs {
    display: flex;
    gap: 4px;
    height: 100%;
  }
  .tabs button {
    position: relative;
    padding: 0 12px;
    color: var(--text-2);
    font-family: var(--font-score);
    font-weight: 600;
    font-size: 14.5px;
  }
  .tabs button:hover {
    color: var(--text);
  }
  .tabs button.on {
    color: var(--text);
  }
  .tabs button.on::after {
    content: '';
    position: absolute;
    left: 12px;
    right: 12px;
    bottom: -1px;
    height: 2px;
    background: var(--you);
  }
  .count {
    margin-left: 4px;
    color: var(--text-3);
    font-size: 12.5px;
  }
  .find {
    display: flex;
    gap: 6px;
    align-items: center;
  }
  .search {
    position: relative;
    color: var(--text-3);
  }
  .search :global(svg) {
    position: absolute;
    left: 10px;
    top: 8.5px;
  }
  .search input {
    width: 220px;
    padding-left: 32px;
  }
  .sortsel {
    width: 150px;
  }
  .sel-on {
    background: var(--you);
    color: var(--you-ink);
  }
  .import-slot {
    margin-top: 14px;
  }
  /* Date bar: years on the left, a month histogram you can click on the right. */
  .period {
    display: flex;
    align-items: stretch;
    gap: 18px;
    margin-top: 14px;
    padding: 8px 10px;
    background: var(--panel);
    border: 1px solid var(--seam);
    border-radius: var(--r-lg);
  }
  .years {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 128px;
    padding-right: 14px;
    border-right: 1px solid var(--seam);
    max-height: 76px;
    overflow-y: auto;
  }
  .yr {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 12px;
    padding: 3px 8px;
    border-radius: var(--r-sm);
    color: var(--text-2);
    font-family: var(--font-score);
    font-weight: 600;
    font-size: 13px;
    text-align: left;
  }
  .yr .n {
    color: var(--text-3);
    font-size: 12px;
    font-weight: 500;
  }
  .yr:hover {
    color: var(--text);
    background: var(--panel-2);
  }
  .yr.on {
    color: var(--you);
    background: var(--panel-3);
  }
  .yr.on .n {
    color: var(--text-2);
  }
  .cal {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-width: 0;
  }
  .months {
    display: flex;
    gap: 4px;
    height: 60px;
  }
  .days {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    padding-top: 8px;
    border-top: 1px solid var(--seam);
  }
  .dy {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 28px;
    padding: 0 10px;
    border-radius: var(--r-sm);
    background: var(--panel-2);
    color: var(--text-2);
  }
  .dy .dn {
    font-family: var(--font-score);
    font-weight: 600;
    font-size: 14px;
    color: var(--text);
  }
  .dy .wd {
    font-size: 11.5px;
    color: var(--text-3);
  }
  .dy .n {
    font-size: 12px;
    color: var(--you);
  }
  .dy:hover:not(:disabled) {
    background: var(--panel-3);
  }
  .dy:disabled {
    cursor: default;
    opacity: 0.55;
  }
  .dy:disabled .n {
    color: var(--text-3);
  }
  .dy.on {
    background: var(--panel-3);
    box-shadow: inset 0 0 0 1px var(--you);
  }
  .dy.on .dn {
    color: var(--you);
  }
  .mo {
    flex: 0 1 88px;
    min-width: 0;
    position: relative;
    display: grid;
    grid-template-rows: auto auto 1fr;
    justify-items: center;
    gap: 1px;
    padding: 6px 2px 0;
    border-radius: var(--r-sm);
    color: var(--text-2);
    overflow: hidden;
  }
  .mo .m {
    font-size: 11.5px;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--text-3);
  }
  .mo .n {
    font-family: var(--font-score);
    font-weight: 600;
    font-size: 15px;
    line-height: 1.2;
  }
  .mo .bar {
    align-self: end;
    width: 100%;
    height: calc(3px + var(--fill) * 15px);
    background: var(--you);
    opacity: 0.28;
    border-radius: 2px 2px 0 0;
    transition: opacity 0.15s var(--ease-out);
  }
  .mo:hover:not(:disabled) {
    background: var(--panel-2);
    color: var(--text);
  }
  .mo:hover:not(:disabled) .bar {
    opacity: 0.5;
  }
  .mo:disabled {
    cursor: default;
  }
  .mo:disabled .n {
    color: var(--text-3);
    opacity: 0.6;
  }
  .mo:disabled .bar {
    opacity: 0.12;
  }
  .mo.on {
    background: var(--panel-3);
    box-shadow: inset 0 0 0 1px var(--you);
    color: var(--you);
  }
  .mo.on .m {
    color: var(--text-2);
  }
  .mo.on .bar {
    opacity: 1;
  }
  .summary {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 16px;
    margin-top: 14px;
    color: var(--text-3);
    font-size: 12.5px;
  }
  .link {
    color: var(--you);
    font-size: 12.5px;
  }
  .link:hover {
    text-decoration: underline;
    text-underline-offset: 3px;
  }
  .selbar {
    position: sticky;
    top: 0;
    z-index: 5;
    display: flex;
    align-items: center;
    gap: 10px;
    margin-top: 10px;
    padding: 8px 12px;
    background: var(--panel-2);
    border: 1px solid var(--you);
    border-radius: var(--r-lg);
  }
  .grow {
    flex: 1;
  }
  .champs {
    max-height: 120px;
    overflow: auto;
  }
  .filters {
    display: grid;
    gap: 8px;
  }
  .f-title {
    font-size: 11.5px;
    font-weight: 600;
    color: var(--text-3);
    margin-top: 4px;
  }
  .f-opts {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .f-opt {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 28px;
    padding: 0 10px;
    border-radius: var(--r-sm);
    background: var(--panel-2);
    color: var(--text-2);
    font-size: 12.5px;
  }
  .f-opt:hover {
    color: var(--text);
  }
  .f-opt.on {
    background: var(--panel-3);
    color: var(--you);
    box-shadow: inset 0 0 0 1px var(--you);
  }
  .f-opt.ev :global(svg) {
    color: var(--c);
  }
  .f-opt.ev.on {
    color: var(--text);
    box-shadow: inset 0 0 0 1px var(--c);
  }
  .muted {
    color: var(--text-3);
    font-size: 12.5px;
  }
  .body {
    flex: 1;
    overflow: auto;
    scrollbar-gutter: stable;
    padding: 8px 24px 32px;
  }
  .group {
    margin-top: 18px;
  }
  .g-head {
    display: flex;
    align-items: center;
    gap: 10px;
    font-size: 13px;
    color: var(--text-2);
    margin-bottom: 8px;
    letter-spacing: 0.03em;
  }
  .g-sep {
    width: 4px;
    height: 4px;
    background: var(--seam-strong);
  }
  .g-day {
    color: var(--text-3);
  }

  /* Score table: one row per run. */
  .table {
    display: grid;
    border: 1px solid var(--seam);
    border-radius: var(--r-lg);
    overflow: hidden;
  }
  .row {
    display: grid;
    grid-template-columns: 128px 44px minmax(180px, 1.4fr) 92px 150px minmax(120px, 1fr) 76px 76px 32px;
    cursor: pointer;
    align-items: center;
    gap: 16px;
    padding: 8px 18px 8px 8px;
    text-align: left;
    background: var(--panel);
    border-top: 1px solid var(--seam);
    transition: background 100ms;
  }
  .row:first-child {
    border-top: 0;
  }
  .row:hover {
    background: var(--panel-2);
  }
  .row.selecting {
    grid-template-columns: 22px 128px 44px minmax(180px, 1.4fr) 92px 150px minmax(120px, 1fr) 76px 76px 32px;
  }
  .row.picked,
  .card.picked {
    background: color-mix(in oklab, var(--you) 10%, var(--panel));
    box-shadow: inset 0 0 0 1px var(--you);
  }
  .check {
    width: 20px;
    height: 20px;
    display: grid;
    place-items: center;
    border-radius: var(--r-sm);
    box-shadow: inset 0 0 0 2px var(--seam-strong);
    color: var(--you-ink);
  }
  .picked .check {
    background: var(--you);
    box-shadow: none;
  }
  .del {
    width: 30px;
    height: 30px;
    display: grid;
    place-items: center;
    border-radius: var(--r-md);
    color: var(--text-3);
    opacity: 0;
    transition: opacity 120ms, background 120ms, color 120ms;
  }
  .row:hover .del,
  .row:focus-within .del,
  .card:hover .del,
  .card:focus-within .del {
    opacity: 1;
  }
  .del:hover,
  .del:focus-visible {
    background: color-mix(in oklab, var(--death) 18%, transparent);
    color: var(--death);
  }
  .selecting .del {
    visibility: hidden;
  }
  .r-thumb {
    aspect-ratio: 16 / 9;
    border-radius: var(--r-sm);
    overflow: hidden;
    background: var(--bg-sunk);
  }
  .r-thumb img,
  .c-thumb img,
  .v-thumb img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }
  .r-champ {
    width: 44px;
    height: 44px;
    border-radius: var(--r-md);
    overflow: hidden;
    background: var(--panel-3);
    display: grid;
    place-items: center;
  }
  .r-champ img {
    width: 100%;
    height: 100%;
  }
  .ph {
    font-family: var(--font-score);
    font-weight: 700;
    color: var(--text-3);
    font-size: 18px;
  }
  .r-title {
    display: grid;
    min-width: 0;
  }
  .t {
    font-family: var(--font-score);
    font-weight: 600;
    font-size: 15.5px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .s {
    color: var(--text-3);
    font-size: 12.5px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .r-result {
    font-family: var(--font-score);
    font-weight: 700;
    font-size: 12.5px;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }
  .r-result.win {
    color: var(--win);
  }
  .r-result.loss {
    color: var(--death);
  }
  .r-kda {
    font-size: 24px;
    letter-spacing: 0.01em;
    white-space: nowrap;
  }
  .r-kda i {
    font-style: normal;
    color: var(--text-3);
    margin: 0 4px;
    font-size: 18px;
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
  .r-tags {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
    min-width: 0;
  }
  .tag {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    height: 22px;
    padding: 0 7px;
    border-radius: var(--r-sm);
    background: var(--panel-3);
    color: var(--text-2);
    font-size: 11.5px;
    white-space: nowrap;
  }
  .tag.you {
    color: var(--you);
  }
  .tag.obj {
    color: var(--objective);
  }
  .r-dur,
  .r-size {
    text-align: right;
    color: var(--text-2);
    font-size: 14px;
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
    gap: 12px;
  }
  .card {
    position: relative;
    cursor: pointer;
    display: grid;
    gap: 6px;
    padding: 6px 6px 10px;
    border-radius: var(--r-lg);
    background: var(--panel);
    text-align: left;
    border: 1px solid var(--seam);
  }
  .card:hover {
    background: var(--panel-2);
  }
  .c-thumb {
    position: relative;
    aspect-ratio: 16 / 9;
    border-radius: var(--r-md);
    overflow: hidden;
    background: var(--bg-sunk);
  }
  .c-dur {
    position: absolute;
    right: 6px;
    bottom: 8px;
    padding: 1px 6px;
    border-radius: var(--r-sm);
    background: var(--scrim);
    font-size: 11.5px;
  }
  .c-del {
    position: absolute;
    top: 6px;
    right: 6px;
    background: var(--scrim);
    color: var(--text-2);
  }
  .c-check {
    position: absolute;
    top: 8px;
    left: 8px;
    background: var(--scrim);
  }
  .c-band {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    height: 3px;
  }
  .c-band.win {
    background: var(--win);
  }
  .c-band.loss {
    background: var(--death);
  }
  .c-meta {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 8px;
    padding: 0 4px;
  }
  .c-kda {
    font-size: 17px;
  }
  .c-sub .r-result {
    font-size: 11px;
  }
  .c-sub {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
  }
  .c-sub > span + span::before {
    content: '·';
    margin: 0 6px;
    color: var(--text-3);
  }
  .c-sub {
    padding: 0 4px;
    color: var(--text-3);
    font-size: 12px;
  }

  .hint {
    color: var(--text-3);
    margin: 12px 0;
  }
  .void-list {
    display: grid;
    gap: 6px;
  }
  .void {
    display: grid;
    grid-template-columns: 128px 1fr auto auto auto;
    align-items: center;
    gap: 16px;
    padding: 8px 12px 8px 8px;
    border-radius: var(--r-lg);
    background: var(--panel);
    border: 1px solid var(--seam);
  }
  .v-thumb {
    position: relative;
    aspect-ratio: 16 / 9;
    border-radius: var(--r-sm);
    overflow: hidden;
    background: var(--bg-sunk);
  }
  .v-thumb img {
    opacity: 0.35;
    filter: grayscale(1);
  }
  .stamp {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    font-family: var(--font-score);
    font-weight: 700;
    letter-spacing: 0.2em;
    color: var(--death);
    transform: rotate(-8deg);
  }
  /* Spacing meant for Latin capitals; Chinese characters need almost none. */
  .stamp:lang(zh-Hans) {
    letter-spacing: 0.04em;
  }
  .v-title {
    display: grid;
    min-width: 0;
  }
  .v-title .t {
    text-decoration: line-through;
    text-decoration-color: color-mix(in oklab, var(--death) 70%, transparent);
    color: var(--text-2);
  }
  .v-left {
    color: var(--warn);
    font-size: 13px;
  }

  @media (max-width: 1280px) {
    .row {
      grid-template-columns: 112px 40px minmax(160px, 1.4fr) 80px 132px minmax(0, 1fr) 64px 32px;
    }
    .row.selecting {
      grid-template-columns: 22px 112px 40px minmax(160px, 1.4fr) 80px 132px minmax(0, 1fr) 64px 32px;
    }
    .r-size {
      display: none;
    }
  }
</style>
