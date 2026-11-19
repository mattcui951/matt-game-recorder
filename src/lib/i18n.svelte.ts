// The UI language. Every string the UI shows comes from a catalog in ./locales; `t` reads the
// active one, so templates update when the language changes. Every catalog must have every key
// the English one has (enforced by its `Messages` type). The backend has its own list
// (src-tauri/src/i18n.rs); add a language in both.

import { en, type Messages } from './locales/en';
import { es } from './locales/es';
import { fr } from './locales/fr';
import { ja } from './locales/ja';
import { ko } from './locales/ko';
import { zhCN } from './locales/zh-CN';
import { zhTW } from './locales/zh-TW';

export type Locale = 'en' | 'zh-CN' | 'zh-TW' | 'es' | 'fr' | 'ja' | 'ko';
export type LanguageSetting = 'system' | Locale;

/** `label` is the native name, shown as-is in every language; `lang` goes on <html lang>. */
const LOCALES: Record<Locale, { label: string; lang: string; intl: string; messages: Messages }> = {
  en: { label: 'English', lang: 'en', intl: 'en-US', messages: en },
  'zh-CN': { label: '简体中文', lang: 'zh-Hans', intl: 'zh-CN', messages: zhCN },
  'zh-TW': { label: '繁體中文', lang: 'zh-Hant', intl: 'zh-TW', messages: zhTW },
  es: { label: 'Español', lang: 'es', intl: 'es-ES', messages: es },
  fr: { label: 'Français', lang: 'fr', intl: 'fr-FR', messages: fr },
  ja: { label: '日本語', lang: 'ja', intl: 'ja-JP', messages: ja },
  ko: { label: '한국어', lang: 'ko', intl: 'ko-KR', messages: ko },
};
const CACHE_KEY = 'mgr.locale';

export const LANGUAGES = (Object.keys(LOCALES) as Locale[]).map((value) => ({ value, label: LOCALES[value].label, lang: LOCALES[value].lang }));

function isLocale(v: unknown): v is Locale {
  return typeof v === 'string' && v in LOCALES;
}

/** The locale for a BCP 47 tag such as navigator.language ("zh-HK" -> "zh-TW", "fr-CA" -> "fr"). */
function fromTag(tag: string): Locale {
  const t = tag.toLowerCase();
  if (t.startsWith('zh')) return /hant|tw|hk|mo/.test(t) ? 'zh-TW' : 'zh-CN';
  const base = t.split('-')[0];
  return isLocale(base) ? base : 'en';
}

function cached(): Locale {
  try {
    const v = localStorage.getItem(CACHE_KEY);
    if (isLocale(v)) return v;
  } catch {
    /* storage unavailable */
  }
  return fromTag(navigator.language);
}

export const i18n = $state({ locale: cached(), system: cached() as Locale });

/** The active catalog. `t.library.title`, `t.library.matches(3)`. */
export const t: Messages = new Proxy({} as Messages, {
  get: (_, key) => LOCALES[i18n.locale].messages[key as keyof Messages],
});

/** The locale a `general.language` setting means. */
export function resolveLanguage(setting: string | undefined): Locale {
  return isLocale(setting) ? setting : i18n.system;
}

export function applyLanguage(setting: string | undefined) {
  i18n.locale = resolveLanguage(setting);
  applyCachedLanguage();
  try {
    localStorage.setItem(CACHE_KEY, i18n.locale);
  } catch {
    /* storage unavailable */
  }
}

/** Set before the first render from the last session, so the app never flashes the other language. */
export function applyCachedLanguage() {
  document.documentElement.lang = LOCALES[i18n.locale].lang;
}

/** Locale for Intl and toLocale* calls. */
export function intlLocale(): string {
  return LOCALES[i18n.locale].intl;
}

/** A language's name in the UI language ("Japanese", "日语"). */
export function languageName(locale: Locale): string {
  try {
    return new Intl.DisplayNames([intlLocale()], { type: 'language' }).of(LOCALES[locale].lang) ?? LOCALES[locale].label;
  } catch {
    return LOCALES[locale].label;
  }
}
