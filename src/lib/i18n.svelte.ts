// The UI language: English or Simplified Chinese. Every string the UI shows comes from a
// catalog in ./locales; `t` reads the active one, so templates update when the language changes.
// The Chinese catalog must have every key the English one has (enforced by its type).

import { en, type Messages } from './locales/en';
import { zhCN } from './locales/zh-CN';

export type Locale = 'en' | 'zh-CN';
export type LanguageSetting = 'system' | Locale;

const CATALOGS: Record<Locale, Messages> = { en, 'zh-CN': zhCN };
const CACHE_KEY = 'mgr.locale';

/** Native names, shown as-is in every language. */
export const LANGUAGES: { value: Locale; label: string }[] = [
  { value: 'en', label: 'English' },
  { value: 'zh-CN', label: '简体中文' },
];

function cached(): Locale {
  try {
    const v = localStorage.getItem(CACHE_KEY);
    if (v === 'en' || v === 'zh-CN') return v;
  } catch {
    /* storage unavailable */
  }
  return navigator.language.toLowerCase().startsWith('zh') ? 'zh-CN' : 'en';
}

export const i18n = $state({ locale: cached(), system: cached() as Locale });

/** The active catalog. `t.library.title`, `t.library.matches(3)`. */
export const t: Messages = new Proxy({} as Messages, {
  get: (_, key) => CATALOGS[i18n.locale][key as keyof Messages],
});

/** The locale a `general.language` setting means. */
export function resolveLanguage(setting: string | undefined): Locale {
  return setting === 'en' || setting === 'zh-CN' ? setting : i18n.system;
}

export function applyLanguage(setting: string | undefined) {
  const locale = resolveLanguage(setting);
  i18n.locale = locale;
  document.documentElement.lang = locale === 'zh-CN' ? 'zh-Hans' : 'en';
  try {
    localStorage.setItem(CACHE_KEY, locale);
  } catch {
    /* storage unavailable */
  }
}

/** Set before the first render from the last session, so the app never flashes the other language. */
export function applyCachedLanguage() {
  document.documentElement.lang = i18n.locale === 'zh-CN' ? 'zh-Hans' : 'en';
}

/** Locale for Intl and toLocale* calls. */
export function intlLocale(): string {
  return i18n.locale === 'zh-CN' ? 'zh-CN' : 'en-US';
}
