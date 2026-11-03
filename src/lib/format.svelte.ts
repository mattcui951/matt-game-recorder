import { i18n, intlLocale, t } from './i18n.svelte';

export function clock(sec: number, withMs = false): string {
  if (!isFinite(sec) || sec < 0) sec = 0;
  const h = Math.floor(sec / 3600);
  const m = Math.floor((sec % 3600) / 60);
  const s = Math.floor(sec % 60);
  const base = h > 0 ? `${h}:${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}` : `${m}:${String(s).padStart(2, '0')}`;
  if (!withMs) return base;
  return `${base}.${String(Math.floor((sec % 1) * 10))}`;
}

/** Parse "m:ss", "h:mm:ss" or plain seconds (decimals allowed). */
export function parseClock(text: string): number | null {
  const parts = text.trim().split(':').map((p) => p.trim());
  if (parts.some((p) => p === '' || isNaN(Number(p)))) return null;
  return parts.reduce((acc, p) => acc * 60 + Number(p), 0);
}

export function bytes(n: number): string {
  if (!n) return '0 MB';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let i = 0;
  let v = n;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v >= 100 || i < 2 ? Math.round(v) : v.toFixed(1)} ${units[i]}`;
}

export function dayLabel(iso: string): string {
  const d = new Date(iso);
  const today = new Date();
  const start = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const diff = Math.round((start(today) - start(d)) / 86400000);
  if (diff === 0) return t.dates.today;
  if (diff === 1) return t.dates.yesterday;
  return d.toLocaleDateString(intlLocale(), { weekday: diff < 7 ? 'long' : undefined, month: 'short', day: 'numeric', year: d.getFullYear() === today.getFullYear() ? undefined : 'numeric' });
}

export function timeOfDay(iso: string): string {
  return new Date(iso).toLocaleTimeString(intlLocale(), { hour: 'numeric', minute: '2-digit' });
}

/** A game's name in the UI language ("League of Legends" -> "英雄联盟"). */
export function gameName(game?: string | null): string {
  if (!game) return '';
  return t.gameNames[game] ?? game;
}

export function modeName(mode?: string | null): string {
  if (!mode) return '';
  return t.modes[mode] ?? mode.charAt(0) + mode.slice(1).toLowerCase();
}

const CHAMPION_NAMES: Record<string, string> = {
  MonkeyKing: 'Wukong',
  JarvanIV: 'Jarvan IV',
  DrMundo: 'Dr. Mundo',
  Kaisa: "Kai'Sa",
  Khazix: "Kha'Zix",
  Chogath: "Cho'Gath",
  Velkoz: "Vel'Koz",
  Belveth: "Bel'Veth",
  KogMaw: "Kog'Maw",
  RekSai: "Rek'Sai",
  KSante: "K'Sante",
  Leblanc: 'LeBlanc',
  Nunu: 'Nunu & Willump',
  Renata: 'Renata Glasc',
  FiddleSticks: 'Fiddlesticks',
};

/** Champion names in Simplified Chinese by lowercased Data Dragon key, loaded when needed. */
export const championsZh = $state<{ names: Record<string, string> }>({ names: {} });

/**
 * One id per champion however its name was stored: recordings keep the game's name ("Bel'Veth",
 * "Wukong"), Outplayed imports the Data Dragon key ("Belveth", "MonkeyKing"). Both give "belveth".
 */
export function championId(name: string): string {
  const key = Object.entries(CHAMPION_NAMES).find(([, v]) => v === name)?.[0] ?? name;
  return key.replace(/[^A-Za-z]/g, '').toLowerCase();
}

function zhName(name: string): string | undefined {
  return championsZh.names[championId(name)];
}

/** Display name for a champion stored as a Data Dragon key ("MasterYi" -> "Master Yi"). */
export function championName(name?: string | null): string {
  if (!name) return '';
  if (i18n.locale === 'zh-CN') {
    const zh = zhName(name);
    if (zh) return zh;
  }
  if (CHAMPION_NAMES[name]) return CHAMPION_NAMES[name];
  return name.includes(' ') ? name : name.replace(/([a-z])([A-Z])/g, '$1 $2');
}

const NAMED = /^(Match|Session|Screen recording|Project) (\d+)$/;

/**
 * A name the app gave a recording or project ("Match 03", "Triple Kill (2)", "Match 03 clip") in
 * the UI language. Names are stored in English; names the user typed are left alone. Mirrors
 * `i18n::title` in the backend.
 */
export function displayTitle(title: string): string {
  if (i18n.locale === 'en') return title;
  const T = t.titles;
  const dup = /^(.*) \((\d+)\)$/.exec(title);
  if (dup) return `${displayTitle(dup[1])} (${dup[2]})`;
  const named = NAMED.exec(title);
  if (named) {
    const n = named[2];
    return named[1] === 'Match' ? T.match(n) : named[1] === 'Session' ? T.session(n) : named[1] === 'Project' ? T.project(n) : T.screen(n);
  }
  if (title.endsWith(' supercut')) return T.supercutOf(displayTitle(title.slice(0, -' supercut'.length)));
  if (title.endsWith(' clip')) return T.clipOf(displayTitle(title.slice(0, -' clip'.length)));
  return T.names[title] ?? title;
}
