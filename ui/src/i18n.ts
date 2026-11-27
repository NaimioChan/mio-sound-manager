import { createSignal } from 'solid-js'
import { en, type MessageKey } from './locales/en'
import { zh } from './locales/zh'
import { ja } from './locales/ja'
import type { EditKind } from './ipc'

/**
 * Localisation: three locales, one flat key set, no runtime dependency.
 *
 * Scope is deliberate. What is translated is the app's own chrome — labels,
 * menus, dialogs, notices. What is not: the product name, category names from the
 * index, file names, sample rates. Those are data, and a translated category name
 * would stop matching the strings the classifier stores in the database.
 *
 * Keys are flat dotted strings rather than nested objects. A nested dictionary
 * reads nicely but makes "which key is missing" a runtime question; with a flat
 * `Record<MessageKey, string>` per locale, `tsc` answers it at build time and
 * `tools/check-i18n.mjs` catches the one thing types cannot — a placeholder that
 * a translation dropped.
 *
 * The locale is a signal, so a change repaints the whole UI without a reload, and
 * it is persisted under `mio:locale` (the same `mio:` prefix as the theme, which
 * `theme.ts` owns for its own key).
 */

export type Locale = 'en' | 'zh' | 'ja'

export const LOCALES: { id: Locale; label: string; htmlLang: string }[] = [
  // Each label is written in its own language: a language picker that shows
  // "Chinese" to someone who cannot read English is the one menu that must not be
  // translated into the current locale.
  { id: 'en', label: 'English', htmlLang: 'en' },
  { id: 'zh', label: '简体中文', htmlLang: 'zh-CN' },
  { id: 'ja', label: '日本語', htmlLang: 'ja' }
]

const DICTS: Record<Locale, Record<MessageKey, string>> = { en, zh, ja }

const STORAGE_KEY = 'mio:locale'

/** Values a placeholder can take. Numbers are formatted by the caller. */
export type Params = Record<string, string | number>

/**
 * Pick the initial locale.
 *
 * The stored choice wins. Otherwise the first of the OS's preferred languages we
 * ship: a Japanese Windows with an English install of the app should open in
 * Japanese, and `navigator.language` is the only signal available without an IPC
 * round trip. Falling back to English rather than to the OS's exact tag means an
 * unsupported locale (`fr-FR`) gets English instead of a half-translated UI.
 */
export function detectLocale(): Locale {
  try {
    const stored = localStorage.getItem(STORAGE_KEY)
    if (stored === 'en' || stored === 'zh' || stored === 'ja') return stored
  } catch {
    // A locked profile costs the preference, not the session.
  }
  const tags = typeof navigator === 'undefined' ? [] : (navigator.languages ?? [navigator.language])
  for (const tag of tags) {
    const base = String(tag).toLowerCase()
    if (base.startsWith('zh')) return 'zh'
    if (base.startsWith('ja')) return 'ja'
    if (base.startsWith('en')) return 'en'
  }
  return 'en'
}

const [locale, setLocaleSignal] = createSignal<Locale>(detectLocale())

export { locale }

/** Switch locale, persist it, and keep `<html lang>` honest for the IME and fonts. */
export function setLocale(next: Locale): void {
  setLocaleSignal(next)
  try {
    localStorage.setItem(STORAGE_KEY, next)
  } catch {
    // Same as above: the UI is already switched, only the memory is lost.
  }
  const entry = LOCALES.find((l) => l.id === next)
  if (entry && typeof document !== 'undefined') {
    document.documentElement.lang = entry.htmlLang
  }
}

/**
 * Translate a key.
 *
 * Reads the `locale` signal, so any component that calls `t()` inside its render
 * body re-renders when the language changes — no subscription to wire up and no
 * context provider to thread through eight components.
 *
 * A missing key cannot happen at compile time, and the runtime fallback to English
 * covers a locale file that somehow ships short: showing the English string beats
 * showing a key like `dock.stopLabel` to a user.
 */
export function t(key: MessageKey, params?: Params): string {
  const dict = DICTS[locale()]
  const template = dict[key] ?? en[key]
  if (!params) return template
  return template.replace(/\{(\w+)\}/g, (match, name: string) => {
    const value = params[name]
    return value === undefined ? match : String(value)
  })
}

/** Localised channel label. The count itself is data; only the word is a message. */
export function channelLabel(channels: number | null | undefined): string {
  if (channels === null || channels === undefined) return '—'
  if (channels === 1) return t('channel.mono')
  if (channels === 2) return t('channel.stereo')
  return t('channel.n', { count: channels })
}

/**
 * A category name as it should be shown.
 *
 * Category names come from the index and are English words stored in the database
 * (`Weapons`, `Foley`). They are data: translating them in the UI would leave the
 * facet filter sending a string the database has never seen.
 *
 * `Uncategorized` is the exception. It is not a classifier guess but a sentinel the
 * schema defaults to, so it is the one category that is a message rather than a
 * value — display it translated, keep comparing against the stored string.
 */
export const UNCATEGORIZED = 'Uncategorized'

export function categoryName(category: string): string {
  return category === UNCATEGORIZED ? t('side.uncategorized') : category
}

/**
 * The words for an undoable edit, for the undo tooltip and the notice.
 *
 * The backend sends the *kind* (`favorite`, `setCategory`, …), never a label: it
 * has no idea which language the window is in. The capitalisation differs by use —
 * 「加入收藏」 reads the same mid-sentence in Chinese and Japanese, but English
 * needs `Add to Favorites` in a tooltip and `add to favorites` after "Undid" — so
 * the sentence form is lowercased only for locales whose script has case at all.
 */
export function editKindName(kind: EditKind, form: 'title' | 'sentence' = 'title'): string {
  const key = `edit.${kind}` as const
  const text = t(key)
  if (form === 'title') return text
  // Lowercasing is an English (and partly German/Greek) concern. `toLowerCase` on
  // CJK is a no-op, so this is safe everywhere, but keeping the branch explicit
  // documents why the sentence form exists at all.
  return locale() === 'en' ? text.charAt(0).toLowerCase() + text.slice(1) : text
}
