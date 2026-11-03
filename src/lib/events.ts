import { championName } from './format.svelte';
import { t } from './i18n.svelte';
import type { RecEvent } from './types';

export type EventMeta = { label: string; color: string; icon: string };

/** Each kind's color comes from the phosphor role that owns it. */
const EVENT_STYLE: Record<string, { color: string; icon: string }> = {
  victory: { color: 'var(--win)', icon: 'trophy' },
  kill: { color: 'var(--you)', icon: 'swords' },
  multikill: { color: 'var(--you)', icon: 'swords' },
  death: { color: 'var(--death)', icon: 'skull' },
  assist: { color: 'var(--assist)', icon: 'assist' },
  ultimate: { color: 'var(--assist)', icon: 'bolt' },
  baron: { color: 'var(--objective)', icon: 'baron' },
  dragon: { color: 'var(--objective)', icon: 'dragon' },
  herald: { color: 'var(--objective)', icon: 'herald' },
  turret: { color: 'var(--objective)', icon: 'turret' },
  inhibitor: { color: 'var(--objective)', icon: 'inhibitor' },
  bookmark: { color: 'var(--mark)', icon: 'flag' },
};

/** Kinds that can be toggled in settings, in Outplayed's order. */
export const SETTING_KINDS = ['victory', 'kill', 'death', 'assist', 'ultimate', 'baron', 'dragon', 'herald', 'turret', 'inhibitor'];

/** Kinds offered by filters (multikill folds into kill), most important first. */
export const FILTER_KINDS = ['kill', 'death', 'assist', 'ultimate', 'turret', 'inhibitor', 'dragon', 'herald', 'baron', 'victory', 'bookmark'];

/** Timeline rows, most important first: fights, then team play, then the map. */
export const LANES: { id: 'fights' | 'team' | 'map'; kinds: string[] }[] = [
  { id: 'fights', kinds: ['kill', 'multikill', 'death'] },
  { id: 'team', kinds: ['assist', 'ultimate'] },
  { id: 'map', kinds: ['turret', 'inhibitor', 'dragon', 'herald', 'baron', 'victory', 'bookmark'] },
];

export function laneOf(kind: string): number {
  const i = LANES.findIndex((l) => l.kinds.includes(kind));
  return i < 0 ? LANES.length - 1 : i;
}

/** How much an event matters, for picking which one stands for a group. */
export function rank(e: RecEvent): number {
  const order = ['multikill', 'kill', 'death', 'baron', 'dragon', 'herald', 'inhibitor', 'turret', 'victory', 'assist', 'ultimate', 'bookmark'];
  const i = order.indexOf(e.kind);
  return (e.multi ?? 0) * 100 + (i < 0 ? 0 : order.length - i);
}

export function meta(kind: string): EventMeta {
  const style = EVENT_STYLE[kind] ?? { color: 'var(--mark)', icon: 'flag' };
  return { ...style, label: t.events[kind] ?? kind };
}

export function filterKey(e: RecEvent): string {
  return e.kind === 'multikill' ? 'kill' : e.kind;
}

// Labels are stored in English by the recorder and the Outplayed import (see lol.rs, import.rs).
const MULTI = ['', '', 'Double Kill', 'Triple Kill', 'Quadra Kill', 'Penta Kill'];
const DRAKES = ['Fire', 'Water', 'Earth', 'Air', 'Hextech', 'Chemtech', 'Elder', 'Infernal', 'Ocean', 'Mountain', 'Cloud'];

/** A name in a label: player names stay, the generic ones are translated. */
function who(name: string): string {
  const L = t.eventLabel;
  return name === 'a turret' ? L.aTurret : name === 'minions' ? L.minions : name === 'the enemy' ? L.theEnemy : name;
}

function objective(text: string): string {
  const L = t.eventLabel;
  const m = /^(.*) Drake$/.exec(text);
  if (m && DRAKES.includes(m[1])) return m[1] === 'Elder' ? L.elder : L.drake(L.drakeTypes[m[1]] ?? m[1]);
  if (text === 'Elder Dragon') return L.elder;
  if (text === 'Dragon') return L.dragon;
  if (text === 'Rift Herald') return L.herald;
  if (text === 'Baron Nashor') return L.baron;
  return text;
}

/**
 * An event's label in the UI language. Kills, deaths and assists name the other champion when
 * it is known (player names are not interesting); a multikill is "Double Kill" etc.
 */
export function eventLabel(e: RecEvent): string {
  const L = t.eventLabel;
  if (e.kind === 'multikill' && e.multi && L.multi[e.multi]) return L.multi[e.multi];
  if (e.champion) {
    const c = championName(e.champion);
    if (e.kind === 'death') return L.killedBy(c);
    if (e.kind === 'assist') return L.assistOn(c);
    if (e.kind === 'kill') return e.label.startsWith('First Blood') ? L.firstBlood(c) : L.killed(c);
  }
  return labelText(e.label);
}

/** A stored English label in the UI language. Unknown labels (typed by the user) are left alone. */
function labelText(label: string): string {
  const L = t.eventLabel;
  let m: RegExpExecArray | null;
  if ((m = /^Killed by (.+)$/.exec(label))) return L.killedBy(who(m[1]));
  if ((m = /^First Blood - (.+)$/.exec(label))) return L.firstBlood(who(m[1]));
  if ((m = /^Killed (.+)$/.exec(label))) return L.killed(who(m[1]));
  if ((m = /^Assist on (.+)$/.exec(label))) return L.assistOn(who(m[1]));
  if ((m = /^(.+) \(stolen\)$/.exec(label))) return L.stolen(objective(m[1]));
  const multi = MULTI.indexOf(label);
  if (multi > 1) return L.multi[multi];
  if (label === 'Turret destroyed') return L.turret;
  if (label === 'Inhibitor destroyed') return L.inhibitor;
  if (label === 'Victory') return L.victory;
  if (label === 'Ultimate') return L.ultimate;
  if (label === 'Manual capture' || label === 'Bookmark') return L.manual;
  // Outplayed imports label plain events by kind.
  if (label === 'Kill' || label === 'Death' || label === 'Assist') return t.events[label.toLowerCase()];
  return objective(label);
}
